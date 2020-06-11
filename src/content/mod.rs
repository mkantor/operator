mod content_item;

use crate::lib::*;
use content_item::*;
use handlebars::Handlebars;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;
use walkdir::{DirEntry, WalkDir};

const HANDLEBARS_FILE_EXTENSION: &str = "hbs";

#[derive(Error, Debug)]
#[error(
  "Failed to parse template{} from content directory path '{}'.",
  .source.template_name.as_ref().map(|known_name| format!(" '{}'", known_name)).unwrap_or_default(),
  .content_directory_path.display()
)]
pub struct RegisteredTemplateParseError {
    source: handlebars::TemplateError,
    content_directory_path: PathBuf,
}

#[derive(Error, Debug)]
#[error(
  "Failed to parse template{}.",
  .source.template_name.as_ref().map(|known_name| format!(" '{}'", known_name)).unwrap_or_default(),
)]
pub struct UnregisteredTemplateParseError {
    source: handlebars::TemplateError,
}

#[derive(Error, Debug)]
pub enum ContentLoadingError {
    #[error(transparent)]
    TemplateParseError(#[from] RegisteredTemplateParseError),

    #[error(
        "Input/output error when loading{} from content directory path '{}'.",
        .name.as_ref().map(|known_name| format!(" '{}'", known_name)).unwrap_or(String::from(" content")),
        .content_directory_path.display()
    )]
    IOError {
        source: io::Error,
        content_directory_path: PathBuf,
        name: Option<String>,
    },

    #[error("You've encountered a bug! This should never happen: {}", .message)]
    Bug { message: String },
}

#[derive(Error, Debug)]
#[error(
  "Rendering failed for template{}.",
  .source.template_name.as_ref().map(|known_name| format!(" '{}'", known_name)).unwrap_or_default(),
)]
pub struct TemplateRenderError {
    #[from]
    source: handlebars::RenderError,
}

pub struct ContentEngine<'a> {
    template_registry: Handlebars<'a>,
}

impl<'a> ContentEngine<'a> {
    pub fn from_content_directory(
        content_directory_path: &'a Path,
    ) -> Result<Self, ContentLoadingError> {
        let mut engine = ContentEngine {
            template_registry: Handlebars::new(),
        };
        engine.register_content_directory(content_directory_path)?;

        Ok(engine)
    }

    pub fn new_content(
        &self,
        handlebars_source: &str,
    ) -> Result<ContentItem, UnregisteredTemplateParseError> {
        ContentItem::new_template(&self.template_registry, handlebars_source)
    }

    pub fn get(&self, address: &'a str) -> Option<ContentItem> {
        ContentItem::from_registry(&self.template_registry, address)
    }

    fn register_content_directory(
        &mut self,
        content_directory_path: &'a Path,
    ) -> Result<(), ContentLoadingError> {
        let entries = WalkDir::new(content_directory_path)
            .min_depth(1)
            .into_iter()
            .collect::<Result<Vec<DirEntry>, walkdir::Error>>()
            .map_err(|walkdir_error| {
                let name = walkdir_error
                    .path()
                    .map(|path| String::from(path.to_string_lossy()));
                ContentLoadingError::IOError {
                    source: io::Error::from(walkdir_error),
                    content_directory_path: content_directory_path.to_path_buf(),
                    name,
                }
            })?
            .into_iter()
            .filter(|entry| entry.path().is_file())
            .filter(|entry| {
                let is_hidden = entry.file_name().to_string_lossy().starts_with('.');
                !is_hidden
            });

        for entry in entries {
            let path = entry.path();
            match path.extension() {
                Some(extension) if extension == OsStr::new(HANDLEBARS_FILE_EXTENSION) => {
                    let name_in_registry = path
                        .strip_prefix(content_directory_path)
                        .map_err(|strip_prefix_error| ContentLoadingError::Bug {
                            message: format!("Unable to determine template name for registry: {}", strip_prefix_error),
                        })?
                        .file_stem()
                        .ok_or_else(|| ContentLoadingError::Bug {
                            message: format!("Unable to determine template name for registry: no file name for '{}'", path.display()),
                        })?;

                    self.template_registry
                        .register_template_file(&name_in_registry.to_string_lossy(), &path)
                        .map_err(|template_render_error| match template_render_error {
                            handlebars::TemplateFileError::TemplateError(source) => {
                                ContentLoadingError::TemplateParseError(
                                    RegisteredTemplateParseError {
                                        source,
                                        content_directory_path: PathBuf::from(
                                            content_directory_path,
                                        ),
                                    },
                                )
                            }
                            handlebars::TemplateFileError::IOError(source, original_name) => {
                                // Handlebars-rust will use an empty string when the error does not
                                // correspond to a specific path.
                                let name = if original_name.is_empty() {
                                    None
                                } else {
                                    Some(original_name)
                                };
                                ContentLoadingError::IOError {
                                    source,
                                    content_directory_path: PathBuf::from(content_directory_path),
                                    name,
                                }
                            }
                        })?;
                }
                _ => {
                    // Ignore non-template files for now.
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lib::*;

    #[test]
    fn content_engine_can_be_created_from_valid_content_directory() {
        for &path in &CONTENT_DIRECTORY_PATHS_WITH_VALID_CONTENTS {
            if let Err(error) = ContentEngine::from_content_directory(Path::new(path)) {
                panic!(
                    "Content engine could not be created from {}: {}",
                    path, error
                );
            }
        }
    }

    #[test]
    fn content_engine_cannot_be_created_from_invalid_content_directory() {
        for &path in &CONTENT_DIRECTORY_PATHS_WITH_INVALID_CONTENTS {
            assert!(
                ContentEngine::from_content_directory(Path::new(path)).is_err(),
                "Content engine was successfully created from {}, but this should have failed",
                path
            );
        }
    }

    #[test]
    fn new_templates_can_be_rendered() {
        let engine = ContentEngine::from_content_directory(
            arbitrary_content_directory_path_with_valid_content(),
        )
        .expect("Content engine could not be created");

        for &(template, expected_output) in &VALID_TEMPLATES {
            let new_content = engine
                .new_content(template)
                .expect("Template could not be parsed");
            let rendered = new_content
                .render(GluonVersion("0.0.0"))
                .expect(&format!("Template rendering failed for `{}`", template,));
            assert_eq!(
                rendered,
                expected_output,
                "Template rendering for `{}` did not produce the expected output (\"{}\"), instead got \"{}\"",
                template,
                expected_output,
                rendered,
            );
        }
    }

    #[test]
    fn new_content_fails_for_invalid_templates() {
        let engine = ContentEngine::from_content_directory(
            arbitrary_content_directory_path_with_valid_content(),
        )
        .expect("Content engine could not be created");

        for &template in &INVALID_TEMPLATES {
            let result = engine.new_content(template);

            assert!(
                result.is_err(),
                "Content was successfully created for invalid template `{}`, but it should have failed",
                template,
            );
        }
    }

    #[test]
    fn new_templates_can_reference_partials_from_content_directory() {
        let content_directory_path = example_path("valid/partials");
        let engine = ContentEngine::from_content_directory(&content_directory_path)
            .expect("Content engine could not be created");

        let template = "this is partial: {{> ab}}";
        let expected_output = "this is partial: a\nb\n\n";

        let new_content = engine
            .new_content(template)
            .expect("Template could not be parsed");
        let rendered = new_content
            .render(GluonVersion("0.0.0"))
            .expect(&format!("Template rendering failed for `{}`", template));
        assert_eq!(
            rendered,
            expected_output,
            "Template rendering for `{}` did not produce the expected output (\"{}\"), instead got \"{}\"",
            template,
            expected_output,
            rendered,
        );
    }

    #[test]
    fn content_can_be_retrieved() {
        let content_directory_path = example_path("valid/partials");
        let engine = ContentEngine::from_content_directory(&content_directory_path)
            .expect("Content engine could not be created");

        let address = "ab";
        let expected_output = "a\nb\n\n";

        let content = engine.get(address).expect("Content could not be found");
        let rendered = content.render(GluonVersion("0.0.0")).expect(&format!(
            "Template rendering failed for content at '{}'",
            address
        ));
        assert_eq!(
            rendered,
            expected_output,
            "Rendering content at '{}' did not produce the expected output (\"{}\"), instead got \"{}\"",
            address,
            expected_output,
            rendered,
        );
    }

    #[test]
    fn content_may_not_exist_at_address() {
        let content_directory_path = example_path("valid/hello-world");
        let engine = ContentEngine::from_content_directory(&content_directory_path)
            .expect("Content engine could not be created");

        let address = "this-address-does-not-refer-to-any-content";

        assert!(
            engine.get(address).is_none(),
            "Content was found at '{}', but it was not expected to be",
            address
        );
    }

    #[test]
    fn content_directory_path_must_exist() {
        let content_directory_path = example_path("this/does/not/actually/exist");
        let result = ContentEngine::from_content_directory(&content_directory_path);

        assert!(
            result.is_err(),
            "Content engine was successfully created from {}, but this should have failed",
            content_directory_path.display(),
        );
    }
}
