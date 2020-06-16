mod content_item;

use crate::directory::Directory;
use crate::lib::*;
use content_item::*;
use handlebars::Handlebars;
use serde::Serialize;
use std::ffi::OsStr;
use std::io;
use std::path::PathBuf;
use thiserror::Error;
use walkdir::DirEntry;

const HANDLEBARS_FILE_EXTENSION: &str = "hbs";

#[derive(Serialize)]
struct GluonRenderData {
    version: GluonVersion,
}

#[derive(Serialize)]
pub struct RenderData {
    gluon: GluonRenderData,
}

#[derive(Error, Debug)]
#[error(
  "Failed to parse template{} from content directory path '{}'.",
  .source.template_name.as_ref().map(|known_name| format!(" '{}'", known_name)).unwrap_or_default(),
  .content_directory_root.display()
)]
pub struct RegisteredTemplateParseError {
    source: handlebars::TemplateError,
    content_directory_root: PathBuf,
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
        .name.as_ref().map(|known_name| format!(" '{}'", known_name)).unwrap_or_else(|| String::from(" content")),
        .content_directory_root.display()
    )]
    IOError {
        source: io::Error,
        content_directory_root: PathBuf,
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
    content_directory_root: PathBuf,
    template_registry: Handlebars<'a>,
}

impl<'a> ContentEngine<'a> {
    pub fn from_content_directory(
        content_directory: Directory,
    ) -> Result<Self, ContentLoadingError> {
        let content_directory_root = content_directory.root().clone();
        let content_item_entries = content_directory
            .into_iter()
            .filter(|entry| entry.path().is_file())
            .filter(|entry| {
                let is_hidden = entry.file_name().to_string_lossy().starts_with('.');
                !is_hidden
            });

        let mut engine = ContentEngine {
            content_directory_root,
            template_registry: Handlebars::new(),
        };
        engine.register_content_directory(content_item_entries)?;

        Ok(engine)
    }

    pub fn get_render_data(&self, gluon_version: GluonVersion) -> RenderData {
        RenderData {
            gluon: GluonRenderData {
                version: gluon_version,
            },
        }
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

    fn register_content_directory<T>(
        &mut self,
        content_item_entries: T,
    ) -> Result<(), ContentLoadingError>
    where
        T: IntoIterator<Item = DirEntry>,
    {
        for entry in content_item_entries {
            let path = entry.path();
            match path.extension() {
                Some(extension) if extension == OsStr::new(HANDLEBARS_FILE_EXTENSION) => {
                    let relative_path_no_extension = path
                        .strip_prefix(&self.content_directory_root)
                        .map_err(|strip_prefix_error| ContentLoadingError::Bug {
                            message: format!(
                                "Unable to determine template name for registry: {}",
                                strip_prefix_error
                            ),
                        })?
                        .with_extension("");

                    let name_in_registry = relative_path_no_extension.to_string_lossy();

                    self.template_registry
                        .register_template_file(&name_in_registry, &path)
                        .map_err(|template_render_error| match template_render_error {
                            handlebars::TemplateFileError::TemplateError(source) => {
                                ContentLoadingError::TemplateParseError(
                                    RegisteredTemplateParseError {
                                        source,
                                        content_directory_root: self.content_directory_root.clone(),
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
                                    content_directory_root: self.content_directory_root.clone(),
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

    fn dummy_render_data() -> RenderData {
        RenderData {
            gluon: GluonRenderData {
                version: GluonVersion("0.0.0"),
            },
        }
    }

    #[test]
    fn content_engine_can_be_created_from_valid_content_directory() {
        for directory in content_directories_with_valid_contents() {
            let root = directory.root().clone();
            if let Err(error) = ContentEngine::from_content_directory(directory) {
                panic!(
                    "Content engine could not be created from {}: {}",
                    root.display(),
                    error,
                );
            }
        }
    }

    #[test]
    fn content_engine_cannot_be_created_from_invalid_content_directory() {
        for directory in content_directories_with_invalid_contents() {
            let root = directory.root().clone();
            assert!(
                ContentEngine::from_content_directory(directory).is_err(),
                "Content engine was successfully created from {}, but this should have failed",
                root.display(),
            );
        }
    }

    #[test]
    fn new_templates_can_be_rendered() {
        let engine =
            ContentEngine::from_content_directory(arbitrary_content_directory_with_valid_content())
                .expect("Content engine could not be created");

        for &(template, expected_output) in &VALID_TEMPLATES {
            let new_content = engine
                .new_content(template)
                .expect("Template could not be parsed");
            let rendered = new_content
                .render(&dummy_render_data())
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
        let engine =
            ContentEngine::from_content_directory(arbitrary_content_directory_with_valid_content())
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
        let directory = Directory::from_root(&example_path("valid/partials")).unwrap();
        let engine = ContentEngine::from_content_directory(directory)
            .expect("Content engine could not be created");

        let template = "this is partial: {{> abc}}";
        let expected_output = "this is partial: a\nb\n\nc\n\n";

        let new_content = engine
            .new_content(template)
            .expect("Template could not be parsed");
        let rendered = new_content
            .render(&dummy_render_data())
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
        let directory = Directory::from_root(&example_path("valid/partials")).unwrap();
        let engine = ContentEngine::from_content_directory(directory)
            .expect("Content engine could not be created");

        let address = "abc";
        let expected_output = "a\nb\n\nc\n\n";

        let content = engine.get(address).expect("Content could not be found");
        let rendered = content.render(&dummy_render_data()).expect(&format!(
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
        let directory = Directory::from_root(&example_path("valid/hello-world")).unwrap();
        let engine = ContentEngine::from_content_directory(directory)
            .expect("Content engine could not be created");

        let address = "this-address-does-not-refer-to-any-content";

        assert!(
            engine.get(address).is_none(),
            "Content was found at '{}', but it was not expected to be",
            address
        );
    }
}
