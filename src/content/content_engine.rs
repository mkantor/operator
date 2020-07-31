use super::content_index::*;
use super::content_item::*;
use super::handlebars_helpers::*;
use super::*;
use crate::content_directory::{ContentDirectory, ContentFile};
use crate::lib::*;
use handlebars::{self, Handlebars};
use mime::{self, Mime};
use mime_guess::MimeGuess;
use std::collections::HashMap;
use std::io;
use std::path::Path;
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

    #[error("Content file has an unknown media type: {}", .message)]
    UnknownFileType { message: String },

    #[error("Failed to create route index while loading content directory.")]
    ContentIndexError {
        #[from]
        source: ContentIndexUpdateError,
    },

    #[error("You've encountered a bug! This should never happen: {}", .message)]
    Bug { message: String },
}

pub trait ContentEngine
where
    Self: Sized,
{
    fn get_render_context(&self) -> RenderContext<Self>;

    fn new_template(
        &self,
        template_source: &str,
        media_type: Mime,
    ) -> Result<UnregisteredTemplate, UnregisteredTemplateParseError>;

    fn get(&self, route: &str) -> Option<&RegisteredContent>;

    fn handlebars_registry(&self) -> &Handlebars;
}

pub enum RegisteredContent {
    /// A static (non-template) file.
    StaticContentItem(StaticContentItem),

    /// A named template that exists in the registry.
    RegisteredTemplate(RegisteredTemplate),

    /// A program that can be executed by the operating system.
    Executable(Executable),
}
impl Render for RegisteredContent {
    fn render<E: ContentEngine>(
        &self,
        context: RenderContext<E>,
        acceptable_media_ranges: &[Mime],
    ) -> Result<String, ContentRenderingError> {
        match self {
            Self::StaticContentItem(renderable) => {
                renderable.render(context, acceptable_media_ranges)
            }
            Self::RegisteredTemplate(renderable) => {
                renderable.render(context, acceptable_media_ranges)
            }
            Self::Executable(renderable) => renderable.render(context, acceptable_media_ranges),
        }
    }
}

type ContentRegistry = HashMap<CanonicalRoute, RegisteredContent>;

pub struct FilesystemBasedContentEngine<'engine> {
    soliton_version: SolitonVersion,
    index: ContentIndex,
    content_registry: ContentRegistry,
    handlebars_registry: Handlebars<'engine>,
}

impl<'engine> FilesystemBasedContentEngine<'engine> {
    pub fn from_content_directory(
        content_directory: ContentDirectory,
        soliton_version: SolitonVersion,
    ) -> Result<Arc<RwLock<Self>>, ContentLoadingError> {
        let content_item_entries = content_directory
            .into_iter()
            .filter(|entry| !entry.is_hidden());

        let (routes, content_registry, handlebars_registry) =
            Self::create_registries(content_item_entries)?;

        let content_engine = FilesystemBasedContentEngine {
            soliton_version,
            index: ContentIndex::Directory(routes),
            content_registry,
            handlebars_registry,
        };

        let shared_content_engine = Arc::new(RwLock::new(content_engine));

        let get_helper = GetHelper::new(shared_content_engine.clone());
        shared_content_engine
            .write()
            .expect("RwLock for ContentEngine has been poisoned")
            .handlebars_registry
            .register_helper("get", Box::new(get_helper));

        Ok(shared_content_engine)
    }

    fn create_registries<'a, E: IntoIterator<Item = ContentFile>>(
        content_item_entries: E,
    ) -> Result<(ContentIndexEntries, ContentRegistry, Handlebars<'a>), ContentLoadingError> {
        let mut routes = ContentIndexEntries::new();
        let mut handlebars_registry = Handlebars::new();
        let mut content_registry = ContentRegistry::new();
        for entry in content_item_entries {
            match entry.extensions() {
                [single_extension] => {
                    if entry.is_executable() {
                        return Err(ContentLoadingError::ContentFileNameError {
                            message: format!(
                                "The content file '{}' is executable, but only has one extension ('{}'). Executables must have two extensions: the first indicates the media type if its output and the second is arbitrary, but can be used to indicate the executable type ('.sh', '.exe', '.py', etc).",
                                entry.relative_path(),
                                single_extension,
                            ),
                        });
                    }

                    routes.try_add(entry.relative_path_without_extensions())?;

                    let canonical_route =
                        CanonicalRoute::new(entry.relative_path_without_extensions());
                    let media_type =
                        MimeGuess::from_ext(single_extension)
                            .first()
                            .ok_or_else(|| ContentLoadingError::UnknownFileType {
                                message: format!(
                                    "The filename extension for the file at '{}' ('{}') does not map to any known media type.",
                                    entry.relative_path(),
                                    single_extension,
                                ),
                            })?;
                    let content_item = RegisteredContent::StaticContentItem(
                        StaticContentItem::new(entry.file_contents(), media_type),
                    );
                    let was_duplicate = content_registry
                        .insert(canonical_route, content_item)
                        .is_some();
                    if was_duplicate {
                        return Err(ContentLoadingError::Bug {
                            message: String::from(
                                "There were two or more content files with the same route.",
                            ),
                        });
                    }
                }

                [first_extension, second_extension] => {
                    match [first_extension.as_str(), second_extension.as_str()] {
                        [first_extension, HANDLEBARS_FILE_EXTENSION] => {
                            if entry.is_executable() {
                                return Err(ContentLoadingError::ContentFileNameError {
                                    message: format!(
                                        "The content file '{}' appears to be a handlebars file (because it ends in '.{}'), but it is also executable. It must be one or the other.",
                                        entry.relative_path(),
                                        HANDLEBARS_FILE_EXTENSION,
                                    ),
                                });
                            }
                            routes.try_add(entry.relative_path_without_extensions())?;

                            let route_string =
                                String::from(entry.relative_path_without_extensions());
                            let canonical_route =
                                CanonicalRoute::new(entry.relative_path_without_extensions());

                            let media_type = MimeGuess::from_ext(first_extension).first().ok_or_else(|| {
                                ContentLoadingError::UnknownFileType {
                                    message: format!(
                                        "The first filename extension for the template at '{}' ('{}') does not map to any known media type.",
                                        entry.relative_path(),
                                        first_extension,
                                    ),
                                }
                            })?;
                            let mut contents = entry.file_contents();

                            handlebars_registry
                                .register_template_source(&route_string, &mut contents)
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

                            let content_item = RegisteredContent::RegisteredTemplate(
                                RegisteredTemplate::new(route_string, media_type),
                            );
                            let was_duplicate = content_registry
                                .insert(canonical_route, content_item)
                                .is_some();
                            if was_duplicate {
                                return Err(ContentLoadingError::Bug {
                                    message: String::from(
                                        "There were two or more content files with the same route.",
                                    ),
                                });
                            }
                        }

                        [first_extension, _arbitrary_second_extension] if entry.is_executable() => {
                            routes.try_add(entry.relative_path_without_extensions())?;

                            let canonical_route =
                                CanonicalRoute::new(entry.relative_path_without_extensions());
                            let media_type =
                                MimeGuess::from_ext(first_extension)
                                    .first()
                                    .ok_or_else(|| ContentLoadingError::UnknownFileType {
                                        message: format!(
                                            "The first filename extension for the executable at '{}' ('{}') does not map to any known media type.",
                                            entry.relative_path(),
                                            first_extension,
                                        ),
                                    })?;

                            // The working directory for the executable is the
                            // immediate parent directory it resides in (which
                            // may be a child of the content directory).
                            let working_directory = Path::new(entry.absolute_path()).parent().ok_or_else(|| {
                                // This indicates a bug because it can only
                                // occur if `entry.absolute_path()` is the
                                // filesystem root, but we should have already
                                // verified that `entry` is a file (not a
                                // directory). If it's the filesystem root then
                                // it is a directory.
                                ContentLoadingError::Bug {
                                    message: format!(
                                        "Failed to get a parent directory for the executable at '{}'.",
                                        entry.absolute_path(),
                                    ),
                                }
                            })?;
                            let content_item = RegisteredContent::Executable(Executable::new(
                                entry.absolute_path(),
                                working_directory,
                                media_type,
                            ));

                            let was_duplicate = content_registry
                                .insert(canonical_route, content_item)
                                .is_some();
                            if was_duplicate {
                                return Err(ContentLoadingError::Bug {
                                    message: String::from(
                                        "There were two or more content files with the same route.",
                                    ),
                                });
                            }
                        }

                        [first_unsupported_extension, second_unsupported_extension] => {
                            return Err(ContentLoadingError::ContentFileNameError {
                                message: format!(
                                    "The content file '{}' has two extensions ('{}.{}'), but is neither a handlebars template nor an executable.",
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
                            "Content file names must have extensions, but '{}' does not.",
                            entry.relative_path()
                        ),
                    })
                }
            }
        }

        Ok((routes, content_registry, handlebars_registry))
    }
}

impl<'engine> ContentEngine for FilesystemBasedContentEngine<'engine> {
    fn get_render_context(&self) -> RenderContext<Self> {
        RenderContext {
            content_engine: self,
            data: RenderData {
                soliton: SolitonRenderData {
                    version: self.soliton_version,
                },
                content: self.index.clone(),
                source_media_type_of_parent: None,
            },
        }
    }

    fn new_template(
        &self,
        handlebars_source: &str,
        media_type: Mime,
    ) -> Result<UnregisteredTemplate, UnregisteredTemplateParseError> {
        UnregisteredTemplate::from_source(handlebars_source, media_type)
    }

    fn get(&self, route: &str) -> Option<&RegisteredContent> {
        self.content_registry.get(&CanonicalRoute::new(route))
    }

    fn handlebars_registry(&self) -> &Handlebars {
        &self.handlebars_registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lib::*;

    const VERSION: SolitonVersion = SolitonVersion("0.0.0");

    // FIXME: It's not ideal to rely on specific example directories in these
    // tests. It would be better to mock out contents in each of the tests.

    #[test]
    fn content_engine_can_be_created_from_valid_content_directory() {
        let content_directories_with_valid_contents = vec![
            example_content_directory("hello-world"),
            example_content_directory("partials"),
            example_content_directory("empty"),
            example_content_directory("static-content"),
            example_content_directory("media-types"),
            example_content_directory("changing-context"),
            example_content_directory("executables"),
            example_content_directory("hidden-content"),
        ];
        for directory in content_directories_with_valid_contents {
            if let Err(error) =
                FilesystemBasedContentEngine::from_content_directory(directory, VERSION)
            {
                panic!("Content engine could not be created: {}", error);
            }
        }
    }

    #[test]
    fn content_engine_cannot_be_created_from_invalid_content_directory() {
        let content_directories_with_invalid_contents = vec![
            example_content_directory("invalid-templates"),
            example_content_directory("invalid-unsupported-static-file"),
            example_content_directory("invalid-single-extension-executable"),
            example_content_directory("invalid-two-extensions-not-template-or-executable"),
            example_content_directory("invalid-template-that-is-executable"),
            example_content_directory("invalid-three-extensions-not-executable"),
            example_content_directory("invalid-three-extensions-executable"),
        ];
        for directory in content_directories_with_invalid_contents {
            assert!(
                FilesystemBasedContentEngine::from_content_directory(directory, VERSION).is_err(),
                "Content engine was successfully created, but this should have failed",
            );
        }
    }

    #[test]
    fn new_templates_can_be_rendered() {
        let shared_content_engine = FilesystemBasedContentEngine::from_content_directory(
            arbitrary_content_directory_with_valid_content(),
            VERSION,
        )
        .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        for &(template, expected_output) in &VALID_TEMPLATES {
            let renderable = content_engine
                .new_template(template, mime::TEXT_HTML)
                .expect("Template could not be parsed");
            let rendered = renderable
                .render(content_engine.get_render_context(), &[mime::TEXT_HTML])
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
    fn new_template_fails_for_invalid_templates() {
        let shared_content_engine = FilesystemBasedContentEngine::from_content_directory(
            arbitrary_content_directory_with_valid_content(),
            VERSION,
        )
        .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        for &template in &INVALID_TEMPLATES {
            let result = content_engine.new_template(template, mime::TEXT_HTML);

            assert!(
                result.is_err(),
                "Content was successfully created for invalid template `{}`, but it should have failed",
                template,
            );
        }
    }

    #[test]
    fn new_templates_can_reference_partials_from_content_directory() {
        let directory = ContentDirectory::from_root(&example_path("partials")).unwrap();
        let shared_content_engine =
            FilesystemBasedContentEngine::from_content_directory(directory, VERSION)
                .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let template = "this is partial: {{> (content.abc)}}";
        let expected_output =
            "this is partial: a\nb\n\nc\n\nsubdirectory entries:\nsubdirectory/c\n";

        let renderable = content_engine
            .new_template(template, mime::TEXT_HTML)
            .expect("Template could not be parsed");
        let rendered = renderable
            .render(content_engine.get_render_context(), &[mime::TEXT_HTML])
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
        let directory = ContentDirectory::from_root(&example_path("partials")).unwrap();
        let shared_content_engine =
            FilesystemBasedContentEngine::from_content_directory(directory, VERSION)
                .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let route = "abc";
        let expected_output = "a\nb\n\nc\n\nsubdirectory entries:\nsubdirectory/c\n";

        let content = content_engine
            .get(route)
            .expect("Content could not be found");
        let rendered = content
            .render(content_engine.get_render_context(), &[mime::TEXT_HTML])
            .expect(&format!(
                "Template rendering failed for content at '{}'",
                route
            ));
        assert_eq!(
            rendered,
            expected_output,
            "Rendering content at '{}' did not produce the expected output (\"{}\"), instead got \"{}\"",
            route,
            expected_output,
            rendered,
        );
    }

    #[test]
    fn content_may_not_exist_at_route() {
        let directory = ContentDirectory::from_root(&example_path("hello-world")).unwrap();
        let shared_content_engine =
            FilesystemBasedContentEngine::from_content_directory(directory, VERSION)
                .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let route = "this-route-does-not-refer-to-any-content";

        assert!(
            content_engine.get(route).is_none(),
            "Content was found at '{}', but it was not expected to be",
            route
        );
    }

    #[test]
    fn get_helper_is_available() {
        let directory = ContentDirectory::from_root(&example_path("partials")).unwrap();
        let shared_content_engine =
            FilesystemBasedContentEngine::from_content_directory(directory, VERSION)
                .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let template = "i got stuff: {{get content.b}}";
        let expected_output = "i got stuff: b\n";

        let renderable = content_engine
            .new_template(template, mime::TEXT_HTML)
            .expect("Template could not be parsed");
        let rendered = renderable
            .render(content_engine.get_render_context(), &[mime::TEXT_HTML])
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
    fn get_helper_requires_an_route_argument() {
        let directory = ContentDirectory::from_root(&example_path("partials")).unwrap();
        let shared_content_engine =
            FilesystemBasedContentEngine::from_content_directory(directory, VERSION)
                .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let templates = [
            "no argument: {{get}}",
            "not a string: {{get 3}}",
            "empty string: {{get \"\"}}",
            "unknown route: {{get \"no/content/at/this/route\"}}",
            "non-existent variables: {{get complete garbage}}",
        ];

        for template in templates.iter() {
            let renderable = content_engine
                .new_template(template, mime::TEXT_HTML)
                .expect("Template could not be parsed");
            let result = renderable.render(content_engine.get_render_context(), &[mime::TEXT_HTML]);
            assert!(
                result.is_err(),
                "Content was successfully rendered for invalid template `{}`, but it should have failed",
                template,
            );
        }
    }

    #[test]
    fn registered_content_cannot_be_rendered_with_unacceptable_target_media_type() {
        let content_directory_path = &example_path("media-types");
        let directory = ContentDirectory::from_root(content_directory_path).unwrap();
        let shared_content_engine =
            FilesystemBasedContentEngine::from_content_directory(directory, VERSION)
                .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let routes = ["cannot-become-html", "template-cannot-become-html"];

        for route in routes.iter() {
            match content_engine.get(route) {
                None => panic!("No content was found at '{}'", route),
                Some(renderable) => {
                    let result =
                        renderable.render(content_engine.get_render_context(), &[mime::TEXT_HTML]);
                    assert!(
                        result.is_err(),
                        "Content was successfully rendered for `{}`, but this should have failed because its media type cannot become html",
                        route,
                    );
                }
            }
        }
    }

    #[test]
    fn anonymous_template_cannot_be_rendered_with_unacceptable_target_media_type() {
        let shared_content_engine = FilesystemBasedContentEngine::from_content_directory(
            arbitrary_content_directory_with_valid_content(),
            VERSION,
        )
        .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let template = content_engine
            .new_template("<p>hi</p>", mime::TEXT_HTML)
            .expect("Template could not be created");
        let result = template.render(content_engine.get_render_context(), &[mime::TEXT_PLAIN]);

        assert!(
            result.is_err(),
            "Template was successfully rendered with unacceptable media type",
        );
    }

    #[test]
    fn nesting_incompatible_media_types_fails_at_render_time() {
        let content_directory_path = &example_path("media-types");
        let directory = ContentDirectory::from_root(content_directory_path).unwrap();
        let shared_content_engine =
            FilesystemBasedContentEngine::from_content_directory(directory, VERSION)
                .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let inputs = vec![
            (mime::TEXT_PLAIN, "nesting/txt-that-includes-html"),
            (mime::TEXT_HTML, "nesting/html-that-includes-txt"),
        ];

        for (target_media_type, route) in inputs {
            match content_engine.get(route) {
                None => panic!("No content was found at '{}'", route),
                Some(renderable) => {
                    let result = renderable
                        .render(content_engine.get_render_context(), &[target_media_type]);
                    assert!(
                        result.is_err(),
                        "Content was successfully rendered for `{}`, but this should have failed",
                        route,
                    );
                }
            }
        }
    }

    #[test]
    fn source_media_type_of_parent_is_target_media_type_when_there_is_no_parent() {
        let shared_content_engine = FilesystemBasedContentEngine::from_content_directory(
            ContentDirectory::from_root(&example_path("media-types")).unwrap(),
            VERSION,
        )
        .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        // Test both registered an unregistered templates.
        let test_cases = [
            (
                content_engine
                    .new_template("{{source-media-type-of-parent}}", mime::TEXT_PLAIN)
                    .expect("Test template was invalid")
                    .render(content_engine.get_render_context(), &[mime::TEXT_PLAIN])
                    .expect("Failed to render unregistered template"),
                mime::TEXT_PLAIN.essence_str(),
            ),
            (
                content_engine
                    .get("echo-source-media-type-of-parent")
                    .expect("Test template does not exist")
                    .render(content_engine.get_render_context(), &[mime::TEXT_HTML])
                    .expect("Failed to render registered template"),
                mime::TEXT_HTML.essence_str(),
            ),
        ];

        for (output, expected_output) in test_cases.iter() {
            assert_eq!(
                output, expected_output,
                "Test case did not produce the expected output (\"{}\"), instead got \"{}\"",
                expected_output, output,
            );
        }
    }

    #[test]
    fn executables_are_given_zero_args() {
        let directory = ContentDirectory::from_root(&example_path("executables")).unwrap();
        let shared_content_engine =
            FilesystemBasedContentEngine::from_content_directory(directory, VERSION)
                .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let route = "count-cli-args";
        let expected_output = "0\n";

        let content = content_engine
            .get(route)
            .expect("Content could not be found");
        let rendered = content
            .render(content_engine.get_render_context(), &[mime::TEXT_PLAIN])
            .expect(&format!("Rendering failed for content at '{}'", route));
        assert_eq!(
            rendered,
            expected_output,
            "Rendering content at '{}' did not produce the expected output (\"{}\"), instead got \"{}\"",
            route,
            expected_output,
            rendered,
        );
    }

    #[test]
    fn executables_are_executed_with_correct_working_directory() {
        let directory = ContentDirectory::from_root(&example_path("executables")).unwrap();
        let shared_content_engine =
            FilesystemBasedContentEngine::from_content_directory(directory, VERSION)
                .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let route1 = "pwd";
        let expected_output1 = format!("{}/src/examples/executables\n", PROJECT_DIRECTORY);

        let content = content_engine
            .get(route1)
            .expect("Content could not be found");
        let rendered = content
            .render(content_engine.get_render_context(), &[mime::TEXT_PLAIN])
            .expect(&format!("Rendering failed for content at '{}'", route1));
        assert_eq!(
            rendered,
            expected_output1,
            "Rendering content at '{}' did not produce the expected output (\"{}\"), instead got \"{}\"",
            route1,
            expected_output1,
            rendered,
        );

        let route2 = "subdirectory/pwd";
        let expected_output2 = format!(
            "{}/src/examples/executables/subdirectory\n",
            PROJECT_DIRECTORY
        );

        let content = content_engine
            .get(route2)
            .expect("Content could not be found");
        let rendered = content
            .render(content_engine.get_render_context(), &[mime::TEXT_PLAIN])
            .expect(&format!("Rendering failed for content at '{}'", route2));
        assert_eq!(
            rendered,
            expected_output2,
            "Rendering content at '{}' did not produce the expected output (\"{}\"), instead got \"{}\"",
            route2,
            expected_output2,
            rendered,
        );
    }

    #[test]
    fn executables_have_a_media_type() {
        let directory = ContentDirectory::from_root(&example_path("executables")).unwrap();
        let shared_content_engine =
            FilesystemBasedContentEngine::from_content_directory(directory, VERSION)
                .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let route = "system-info-SKIP-SNAPSHOT"; // This outputs text/html.
        let content = content_engine
            .get(route)
            .expect("Content could not be found");

        let result1 = content.render(content_engine.get_render_context(), &[mime::TEXT_PLAIN]); // Not text/html!
        assert!(
            result1.is_err(),
            "Rendering content at '{}' succeeded when it should have failed",
            route,
        );

        let result2 = content.render(content_engine.get_render_context(), &[mime::TEXT_HTML]);
        assert!(
            result2.is_ok(),
            "Rendering content at '{}' failed when it should have succeeded",
            route,
        );
    }

    #[test]
    fn templates_can_get_executable_output() {
        let directory = ContentDirectory::from_root(&example_path("executables")).unwrap();
        let shared_content_engine =
            FilesystemBasedContentEngine::from_content_directory(directory, VERSION)
                .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let route = "template";
        let expected_output = format!(
            "this is pwd from subdirectory:\n{}/src/examples/executables/subdirectory\n",
            PROJECT_DIRECTORY
        );

        let content = content_engine
            .get(route)
            .expect("Content could not be found");
        let rendered = content
            .render(content_engine.get_render_context(), &[mime::TEXT_PLAIN])
            .expect(&format!("Rendering failed for content at '{}'", route));
        assert_eq!(
            rendered,
            expected_output,
            "Rendering content at '{}' did not produce the expected output (\"{}\"), instead got \"{}\"",
            route,
            expected_output,
            rendered,
        );
    }

    #[test]
    fn content_can_be_hidden() {
        let directory = ContentDirectory::from_root(&example_path("hidden-content")).unwrap();
        let shared_content_engine =
            FilesystemBasedContentEngine::from_content_directory(directory, VERSION)
                .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let routes = [
            "hidden-file",
            ".hidden-file",
            "hidden-directory",
            ".hidden-directory",
            "hidden-directory/hidden-file",
            ".hidden-directory/hidden-file",
            "hidden-directory/.hidden-file",
            ".hidden-directory/.hidden-file",
            "hidden-directory/non-hidden-file",
            ".hidden-directory/non-hidden-file",
            "hidden-directory/.non-hidden-file",
            ".hidden-directory/.non-hidden-file",
        ];

        for route in routes.iter() {
            assert!(
                content_engine.get(route).is_none(),
                "Content was successfully retrieved for hidden item `{}`, but `get` should have returned None",
                route,
            );
        }
    }
}
