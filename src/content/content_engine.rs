use super::content_index::*;
use super::content_item::*;
use super::*;
use crate::directory::Directory;
use crate::lib::*;
use handlebars::Handlebars;
use std::io;
use std::path::PathBuf;
use thiserror::Error;

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
    #[from]
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

    #[error(
        "Failed to create index while loading content directory '{}'.",
        .content_directory_root.display()
    )]
    ContentIndexError {
        source: ContentIndexUpdateError,
        content_directory_root: PathBuf,
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

pub struct ContentEngine<'engine> {
    index: ContentIndex,
    template_registry: Handlebars<'engine>,
}

impl<'engine> ContentEngine<'engine> {
    pub fn from_content_directory(
        content_directory: Directory,
    ) -> Result<Self, ContentLoadingError> {
        let content_directory_root = content_directory.root().clone();
        let content_item_entries = content_directory
            .into_iter()
            .filter(|entry| entry.metadata().is_file())
            .filter(|entry| {
                let is_hidden = match entry.relative_path_components().last() {
                    Some(base_name) => base_name.starts_with('.'),
                    None => true,
                };

                !is_hidden
            });

        let mut addresses = ContentIndexEntries::new();
        let mut template_registry = Handlebars::new();
        for entry in content_item_entries {
            match entry
                .relative_path()
                .strip_suffix(HANDLEBARS_FILE_EXTENSION)
            {
                Some(relative_path_without_extension) => {
                    addresses
                        .try_add(relative_path_without_extension)
                        .map_err(|source| ContentLoadingError::ContentIndexError {
                            content_directory_root: content_directory_root.clone(),
                            source,
                        })?;

                    let name_in_registry = String::from(relative_path_without_extension);
                    let mut contents = entry.file_contents().ok_or(ContentLoadingError::Bug {
                        message: format!(
                            "Expected entry for '{}' to be a file, but file contents did not exist",
                            name_in_registry
                        ),
                    })?;

                    template_registry
                        .register_template_source(&name_in_registry, &mut contents)
                        .map_err(|template_render_error| match template_render_error {
                            handlebars::TemplateFileError::TemplateError(source) => {
                                ContentLoadingError::TemplateParseError(
                                    RegisteredTemplateParseError {
                                        source,
                                        content_directory_root: content_directory_root.clone(),
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
                                    content_directory_root: content_directory_root.clone(),
                                    name,
                                }
                            }
                        })?;
                }
                None => {
                    // Ignore non-template files for now.
                    panic!("Non-template files are not supported yet!")
                }
            }
        }

        Ok(ContentEngine {
            index: ContentIndex::Directory(addresses),
            template_registry,
        })
    }

    pub fn get_render_data(&self, soliton_version: SolitonVersion) -> RenderData {
        RenderData {
            soliton: SolitonRenderData {
                version: soliton_version,
            },
            content: self.index.clone(),
        }
    }

    pub fn new_content(
        &self,
        handlebars_source: &str,
    ) -> Result<ContentItem, UnregisteredTemplateParseError> {
        ContentItem::new_template(&self.template_registry, handlebars_source)
    }

    pub fn get(&self, address: &'engine str) -> Option<ContentItem> {
        ContentItem::from_registry(&self.template_registry, address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lib::*;

    fn dummy_render_data(engine: &ContentEngine) -> RenderData {
        engine.get_render_data(SolitonVersion("0.0.0"))
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
                .render(&dummy_render_data(&engine))
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

        let template = "this is partial: {{> (content.abc)}}";
        let expected_output = "this is partial: a\nb\n\nc\n\n";

        let new_content = engine
            .new_content(template)
            .expect("Template could not be parsed");
        let rendered = new_content
            .render(&dummy_render_data(&engine))
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
        let rendered = content.render(&dummy_render_data(&engine)).expect(&format!(
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
