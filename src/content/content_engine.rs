use super::content_index::*;
use super::content_item::*;
use super::handlebars_helpers::*;
use super::*;
use crate::content_directory::{ContentDirectory, ContentFile};
use crate::lib::*;
use handlebars::{self, Handlebars};
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, RwLock};
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

    #[error("Content file name is not supported: {}", .message)]
    ContentFileNameError { message: String },

    #[error("Failed to create index while loading content directory.")]
    ContentIndexError { source: ContentIndexUpdateError },

    #[error("You've encountered a bug! This should never happen: {}", .message)]
    Bug { message: String },
}

enum RegisteredContent {
    /// A static (non-template) file.
    StaticContentItem(StaticContentItem),

    /// A named template that exists in the registry.
    RegisteredTemplate(RegisteredTemplate),
}
type ContentRegistry = HashMap<CanonicalAddress, RegisteredContent>;

pub struct ContentEngine<'engine> {
    soliton_version: SolitonVersion,
    index: ContentIndex,
    content_registry: ContentRegistry,

    // This has module visibility; template content items need to reference it.
    pub(super) handlebars_registry: Handlebars<'engine>,
}

impl<'engine> ContentEngine<'engine> {
    pub fn from_content_directory(
        content_directory: ContentDirectory,
        soliton_version: SolitonVersion,
    ) -> Result<Arc<RwLock<Self>>, ContentLoadingError> {
        let content_item_entries = content_directory
            .into_iter()
            .filter(|entry| !entry.is_hidden());

        let (addresses, content_registry, handlebars_registry) =
            Self::create_registries(content_item_entries)?;

        let engine = ContentEngine {
            soliton_version,
            index: ContentIndex::Directory(addresses),
            content_registry,
            handlebars_registry,
        };

        let shared_engine = Arc::new(RwLock::new(engine));

        let get_helper = GetHelper::new(shared_engine.clone());
        shared_engine
            .write()
            .expect("RwLock for ContentEngine has been poisoned")
            .handlebars_registry
            .register_helper("get", Box::new(get_helper));

        Ok(shared_engine)
    }

    fn create_registries<'a, E: IntoIterator<Item = ContentFile>>(
        content_item_entries: E,
    ) -> Result<(ContentIndexEntries, ContentRegistry, Handlebars<'a>), ContentLoadingError> {
        let mut addresses = ContentIndexEntries::new();
        let mut handlebars_registry = Handlebars::new();
        let mut static_files = ContentRegistry::new();
        for entry in content_item_entries {
            match entry.extensions() {
                [single_extension] => match single_extension.as_str() {
                    HTML_FILE_EXTENSION => {
                        addresses
                            .try_add(entry.relative_path_without_extensions())
                            .map_err(|source| ContentLoadingError::ContentIndexError { source })?;

                        let canonical_address =
                            CanonicalAddress::new(entry.relative_path_without_extensions());
                        let static_content_item = RegisteredContent::StaticContentItem(
                            StaticContentItem::new(entry.file_contents()),
                        );
                        let was_duplicate = static_files
                            .insert(canonical_address, static_content_item)
                            .is_some();
                        if was_duplicate {
                            return Err(ContentLoadingError::Bug {
                                message: String::from(
                                    "There were two or more static files with the same address.",
                                ),
                            });
                        }
                    }

                    unsupported_extension => {
                        return Err(ContentLoadingError::ContentFileNameError {
                            message: format!(
                                "The content file '{}' has an unsupported extension ('{}').",
                                entry.relative_path(),
                                unsupported_extension
                            ),
                        });
                    }
                },

                [first_extension, second_extension] => {
                    match [first_extension.as_str(), second_extension.as_str()] {
                        [HTML_FILE_EXTENSION, HANDLEBARS_FILE_EXTENSION] => {
                            addresses
                                .try_add(entry.relative_path_without_extensions())
                                .map_err(|source| ContentLoadingError::ContentIndexError {
                                    source,
                                })?;

                            let canonical_address =
                                String::from(entry.relative_path_without_extensions());
                            let mut contents = entry.file_contents();

                            handlebars_registry
                                .register_template_source(&canonical_address, &mut contents)
                                .map_err(|template_render_error| match template_render_error {
                                    handlebars::TemplateFileError::TemplateError(source) => {
                                        ContentLoadingError::TemplateParseError(
                                            RegisteredTemplateParseError { source },
                                        )
                                    }
                                    handlebars::TemplateFileError::IOError(
                                        source,
                                        original_name,
                                    ) => {
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

                        [first_unsupported_extension, second_unsupported_extension] => {
                            return Err(ContentLoadingError::ContentFileNameError {
                                message: format!(
                                    "The content file '{}' has a unsupported extensions ('{}.{}').",
                                    entry.relative_path(),
                                    first_unsupported_extension,
                                    second_unsupported_extension
                                ),
                            });
                        }
                    }
                }

                [_, _, _, ..] => {
                    return Err(ContentLoadingError::ContentFileNameError {
                        message: format!(
                            "Content file name '{}' has too many extensions.",
                            entry.relative_path()
                        ),
                    })
                }
                [] => {
                    return Err(ContentLoadingError::ContentFileNameError {
                        message: format!(
                            "Content file names must have an extension, but '{}' does not.",
                            entry.relative_path()
                        ),
                    })
                }
            }
        }

        // Create the complete registry from both templates and static files.
        let mut content_registry = handlebars_registry
            .get_templates()
            .keys()
            .map(|address| {
                Ok((
                    CanonicalAddress::new(address),
                    RegisteredContent::RegisteredTemplate(RegisteredTemplate::new(address)),
                ))
            })
            .collect::<Result<ContentRegistry, ContentLoadingError>>()?;
        content_registry.extend(static_files);

        Ok((addresses, content_registry, handlebars_registry))
    }

    pub fn get_render_context(&self) -> RenderContext {
        RenderContext {
            engine: self,
            data: RenderData {
                soliton: SolitonRenderData {
                    version: self.soliton_version,
                },
                content: self.index.clone(),
            },
        }
    }

    pub fn new_content(
        &self,
        handlebars_source: &str,
    ) -> Result<
        Box<dyn Render<RenderArgs = RenderContext, Error = ContentRenderingError>>,
        UnregisteredTemplateParseError,
    > {
        match UnregisteredTemplate::from_source(handlebars_source) {
            Ok(content) => Ok(Box::new(content)),
            Err(error) => Err(error),
        }
    }

    pub fn get(
        &self,
        address: &str,
    ) -> Option<&dyn Render<RenderArgs = RenderContext, Error = ContentRenderingError>> {
        match self.content_registry.get(&CanonicalAddress::new(address)) {
            Some(RegisteredContent::StaticContentItem(renderable)) => Some(renderable),
            Some(RegisteredContent::RegisteredTemplate(renderable)) => Some(renderable),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lib::*;

    const VERSION: SolitonVersion = SolitonVersion("0.0.0");

    #[test]
    fn content_engine_can_be_created_from_valid_content_directory() {
        for directory in content_directories_with_valid_contents() {
            if let Err(error) = ContentEngine::from_content_directory(directory, VERSION) {
                panic!("Content engine could not be created: {}", error);
            }
        }
    }

    #[test]
    fn content_engine_cannot_be_created_from_invalid_content_directory() {
        for directory in content_directories_with_invalid_contents() {
            assert!(
                ContentEngine::from_content_directory(directory, VERSION).is_err(),
                "Content engine was successfully created, but this should have failed",
            );
        }
    }

    #[test]
    fn new_templates_can_be_rendered() {
        let locked_engine = ContentEngine::from_content_directory(
            arbitrary_content_directory_with_valid_content(),
            VERSION,
        )
        .expect("Content engine could not be created");
        let engine = locked_engine.read().unwrap();

        for &(template, expected_output) in &VALID_TEMPLATES {
            let new_content = engine
                .new_content(template)
                .expect("Template could not be parsed");
            let rendered = new_content
                .render(&engine.get_render_context())
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
        let locked_engine = ContentEngine::from_content_directory(
            arbitrary_content_directory_with_valid_content(),
            VERSION,
        )
        .expect("Content engine could not be created");
        let engine = locked_engine.read().unwrap();

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
        let directory = ContentDirectory::from_root(&example_path("valid/partials")).unwrap();
        let locked_engine = ContentEngine::from_content_directory(directory, VERSION)
            .expect("Content engine could not be created");
        let engine = locked_engine.read().unwrap();

        let template = "this is partial: {{> (content.abc)}}";
        let expected_output = "this is partial: a\nb\n\nc\n\n";

        let new_content = engine
            .new_content(template)
            .expect("Template could not be parsed");
        let rendered = new_content
            .render(&engine.get_render_context())
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
        let directory = ContentDirectory::from_root(&example_path("valid/partials")).unwrap();
        let locked_engine = ContentEngine::from_content_directory(directory, VERSION)
            .expect("Content engine could not be created");
        let engine = locked_engine.read().unwrap();

        let address = "abc";
        let expected_output = "a\nb\n\nc\n\n";

        let content = engine.get(address).expect("Content could not be found");
        let rendered = content
            .render(&engine.get_render_context())
            .expect(&format!(
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
        let directory = ContentDirectory::from_root(&example_path("valid/hello-world")).unwrap();
        let locked_engine = ContentEngine::from_content_directory(directory, VERSION)
            .expect("Content engine could not be created");
        let engine = locked_engine.read().unwrap();

        let address = "this-address-does-not-refer-to-any-content";

        assert!(
            engine.get(address).is_none(),
            "Content was found at '{}', but it was not expected to be",
            address
        );
    }

    #[test]
    fn get_helper_is_available() {
        let directory = ContentDirectory::from_root(&example_path("valid/partials")).unwrap();
        let locked_engine = ContentEngine::from_content_directory(directory, VERSION)
            .expect("Content engine could not be created");
        let engine = locked_engine.read().unwrap();

        let template = "i got stuff: {{get content.b}}";
        let expected_output = "i got stuff: b\n";

        let new_content = engine
            .new_content(template)
            .expect("Template could not be parsed");
        let rendered = new_content
            .render(&engine.get_render_context())
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
    fn get_helper_requires_an_address_argument() {
        let directory = ContentDirectory::from_root(&example_path("valid/partials")).unwrap();
        let locked_engine = ContentEngine::from_content_directory(directory, VERSION)
            .expect("Content engine could not be created");
        let engine = locked_engine.read().unwrap();

        let templates = [
            "no argument: {{get}}",
            "not a string: {{get 3}}",
            "empty string: {{get \"\"}}",
            "unknown address: {{get \"no/content/at/this/address\"}}",
            "non-existent variables: {{get complete garbage}}",
        ];

        for template in templates.iter() {
            let new_content = engine
                .new_content(template)
                .expect("Template could not be parsed");
            let result = new_content.render(&engine.get_render_context());
            assert!(
                result.is_err(),
                "Content was successfully rendered for invalid template `{}`, but it should have failed",
                template,
            );
        }
    }
}
