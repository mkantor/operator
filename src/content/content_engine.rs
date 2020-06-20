use super::content_index::*;
use super::content_item::*;
use super::*;
use crate::directory::{Directory, DirectoryEntry};
use crate::lib::*;
use handlebars::Handlebars;
use std::collections::HashMap;
use std::io;
use std::rc::Rc;
use thiserror::Error;

#[derive(Error, Debug)]
#[error(
  "Failed to parse template{} from content directory.",
  .source.template_name.as_ref().map(|known_name| format!(" '{}'", known_name)).unwrap_or_default()
)]
pub struct RegisteredTemplateParseError {
    source: handlebars::TemplateError,
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
        "Input/output error when loading{} from content directory.",
        .name.as_ref().map(|known_name| format!(" '{}'", known_name)).unwrap_or_else(|| String::from(" content")),
    )]
    IOError {
        source: io::Error,
        name: Option<String>,
    },

    #[error("Failed to create index while loading content directory.")]
    ContentIndexError { source: ContentIndexUpdateError },

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

type ContentRegistry<'a> = HashMap<CanonicalAddress, ContentItem<'a>>;
pub struct ContentEngine<'engine> {
    index: ContentIndex,
    content_registry: ContentRegistry<'engine>,
    handlebars_registry: Rc<Handlebars<'engine>>,
}

impl<'engine> ContentEngine<'engine> {
    pub fn from_content_directory(
        content_directory: Directory,
    ) -> Result<Self, ContentLoadingError> {
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

        let (addresses, content_registry, handlebars_registry) =
            Self::create_registry(content_item_entries)?;
        Ok(ContentEngine {
            index: ContentIndex::Directory(addresses),
            content_registry,
            handlebars_registry,
        })
    }

    fn create_registry<'a, E: IntoIterator<Item = DirectoryEntry>>(
        content_item_entries: E,
    ) -> Result<(ContentIndexEntries, ContentRegistry<'a>, Rc<Handlebars<'a>>), ContentLoadingError>
    {
        let mut addresses = ContentIndexEntries::new();
        let mut handlebars_registry = Handlebars::new();
        for entry in content_item_entries {
            match entry
                .relative_path()
                .strip_suffix(HANDLEBARS_FILE_EXTENSION)
            {
                Some(relative_path_without_extension) => {
                    addresses
                        .try_add(relative_path_without_extension)
                        .map_err(|source| ContentLoadingError::ContentIndexError { source })?;

                    let canonical_address = String::from(relative_path_without_extension);
                    let mut contents = entry.file_contents().ok_or(ContentLoadingError::Bug {
                        message: format!(
                            "Expected entry for '{}' to be a file, but file contents did not exist",
                            canonical_address
                        ),
                    })?;

                    handlebars_registry
                        .register_template_source(&canonical_address, &mut contents)
                        .map_err(|template_render_error| match template_render_error {
                            handlebars::TemplateFileError::TemplateError(source) => {
                                ContentLoadingError::TemplateParseError(
                                    RegisteredTemplateParseError { source },
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
                                ContentLoadingError::IOError { source, name }
                            }
                        })?;
                }
                None => {
                    // Ignore non-template files for now.
                    panic!("Non-template files are not supported yet!")
                }
            }
        }

        let reference_counted_handlebars_registry = Rc::new(handlebars_registry);
        let content_registry = reference_counted_handlebars_registry
            .get_templates()
            .keys()
            .map(|address| {
                let registered_template = RegisteredTemplate::from_registry(
                    Rc::clone(&reference_counted_handlebars_registry),
                    address.clone(),
                ).ok_or_else(|| ContentLoadingError::Bug {
                    message: format!(
                        "Handlebars registry lookup for '{}' failed, even though that template name came from the registry.",
                        address,
                    )
                })?;
                Ok((
                    CanonicalAddress::new(address),
                    ContentItem::RegisteredTemplate(registered_template),
                ))
            })
            .collect::<Result<ContentRegistry, ContentLoadingError>>()?;

        Ok((
            addresses,
            content_registry,
            reference_counted_handlebars_registry,
        ))
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
        UnregisteredTemplate::from_source(&self.handlebars_registry, handlebars_source)
            .map(ContentItem::UnregisteredTemplate)
    }

    pub fn get(&self, address: &'engine str) -> Option<&'engine ContentItem> {
        self.content_registry.get(&CanonicalAddress::new(address))
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
            if let Err(error) = ContentEngine::from_content_directory(directory) {
                panic!("Content engine could not be created: {}", error);
            }
        }
    }

    #[test]
    fn content_engine_cannot_be_created_from_invalid_content_directory() {
        for directory in content_directories_with_invalid_contents() {
            assert!(
                ContentEngine::from_content_directory(directory).is_err(),
                "Content engine was successfully created, but this should have failed",
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
