use super::content_directory::{ContentDirectory, ContentFile};
use super::content_index::*;
use super::content_item::*;
use super::content_registry::*;
use super::handlebars_helpers::*;
use super::*;
use handlebars::{self, Handlebars};
use mime_guess::MimeGuess;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::{Arc, RwLock};
use thiserror::Error;

/// A handlebars template had invalid syntax.
#[derive(Error, Debug)]
#[error(
  "Failed to parse handlebars template{}.",
  .source.template_name.as_ref().map(|known_name| format!(" '{}'", known_name)).unwrap_or_default(),
)]
pub struct TemplateParseError {
    #[from]
    source: handlebars::TemplateError,
}

/// There was a problem loading content from the filesystem.
#[derive(Error, Debug)]
pub enum ContentLoadingError {
    #[error(transparent)]
    TemplateParseError(#[from] TemplateParseError),

    #[error(
        "Input/output error when loading{} from content directory.",
        .name.as_ref().map(|known_name| format!(" '{}'", known_name)).unwrap_or_else(|| String::from(" content")),
    )]
    IOError {
        source: io::Error,
        name: Option<String>,
    },

    #[error("Content file name is not supported: {}", .0)]
    ContentFileNameError(String),

    #[error("There are multiple content files for route /{} with the same media type ({}).", .route, .media_type)]
    DuplicateContent {
        route: String,
        media_type: MediaType,
    },

    #[error("Content file has an unknown media type: {}", .0)]
    UnknownFileType(String),

    #[error("Failed to create route index while loading content directory.")]
    ContentIndexError {
        #[from]
        source: ContentIndexUpdateError,
    },

    #[error("You've encountered a bug! This should never happen: {}", .0)]
    Bug(String),
}

pub trait ContentEngine<ServerInfo, ErrorCode>
where
    Self: Sized,
    ServerInfo: Clone + Serialize,
    ErrorCode: Clone + Serialize,
{
    fn get_render_context(&self, request_route: &str)
        -> RenderContext<ServerInfo, ErrorCode, Self>;

    fn new_template(
        &self,
        template_source: &str,
        media_type: MediaType,
    ) -> Result<UnregisteredTemplate, TemplateParseError>;

    fn get(&self, route: &str) -> Option<&ContentRepresentations>;

    fn handlebars_registry(&self) -> &Handlebars;
}

/// A [`ContentEngine`](trait.ContentEngine.html) that serves files from a
/// [`ContentDirectory`](struct.ContentDirectory.html).
pub struct FilesystemBasedContentEngine<'engine, ServerInfo, ErrorCode>
where
    ErrorCode: Clone + Serialize,
    ServerInfo: Clone + Serialize,
{
    server_info: ServerInfo,
    index: ContentIndex,
    content_registry: ContentRegistry,
    handlebars_registry: Handlebars<'engine>,
    error_code_type: PhantomData<ErrorCode>,
}

impl<'engine, ServerInfo, ErrorCode> FilesystemBasedContentEngine<'engine, ServerInfo, ErrorCode>
where
    ErrorCode: 'static + Clone + Serialize + Send + Sync,
    ServerInfo: 'static + Clone + Serialize + Send + Sync,
{
    const HANDLEBARS_FILE_EXTENSION: &'static str = "hbs";

    pub fn from_content_directory(
        content_directory: ContentDirectory,
        server_info: ServerInfo,
    ) -> Result<Arc<RwLock<Self>>, ContentLoadingError> {
        let content_item_entries = content_directory
            .into_iter()
            .filter(|entry| !entry.is_hidden());

        let (index_entries, content_registry, handlebars_registry) =
            Self::set_up_registries(content_item_entries)?;

        let content_engine = FilesystemBasedContentEngine {
            server_info,
            index: ContentIndex::Directory(index_entries),
            content_registry,
            handlebars_registry,
            error_code_type: PhantomData,
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

    fn set_up_registries<'a, E: IntoIterator<Item = ContentFile>>(
        content_item_entries: E,
    ) -> Result<(ContentIndexEntries, ContentRegistry, Handlebars<'a>), ContentLoadingError> {
        let mut index = ContentIndexEntries::new();
        let mut handlebars_registry = Handlebars::new();
        let mut content_registry = ContentRegistry::new();
        for entry in content_item_entries {
            let extensions = entry.extensions().to_owned();
            match extensions.as_slice() {
                [single_extension] => Self::register_file_with_one_extension(
                    entry,
                    single_extension,
                    &mut index,
                    &mut content_registry,
                )?,
                [first_extension, second_extension] => Self::register_file_with_two_extensions(
                    entry,
                    first_extension,
                    second_extension,
                    &mut index,
                    &mut content_registry,
                    &mut handlebars_registry,
                )?,
                [_, _, _, ..] => {
                    return Err(ContentLoadingError::ContentFileNameError(format!(
                        "Content file name '{}' has too many extensions.",
                        entry.relative_path()
                    )))
                }
                [] => {
                    return Err(ContentLoadingError::ContentFileNameError(format!(
                        "Content file names must have extensions, but '{}' does not.",
                        entry.relative_path()
                    )))
                }
            }
        }

        Ok((index, content_registry, handlebars_registry))
    }

    /// Content files with one extension indicate static content (e.g. an image
    /// or plain text file). They must not have the executable bit set.
    fn register_file_with_one_extension(
        file: ContentFile,
        extension: &str,
        index: &mut ContentIndexEntries,
        content_registry: &mut ContentRegistry,
    ) -> Result<(), ContentLoadingError> {
        if file.is_executable() {
            return Err(ContentLoadingError::ContentFileNameError(format!(
                "The content file '{}' is executable, but only has one extension ('{}'). \
                    Executables must have two extensions: \
                    the first indicates the media type of its output, and the second is arbitrary \
                    but can be used to indicate the executable type ('.sh', '.exe', '.py', etc).",
                file.relative_path(),
                extension,
            )));
        }

        let route = Route::new(file.relative_path_without_extensions());
        let mime =
            MimeGuess::from_ext(extension)
                .first()
                .ok_or_else(|| ContentLoadingError::UnknownFileType(
                    format!(
                        "The filename extension for the file at '{}' ('{}') does not map to any known media type.",
                        file.relative_path(),
                        extension,
                    ),
                ))?;
        let media_type = MediaType::from_media_range(mime).ok_or_else(|| {
            ContentLoadingError::Bug(String::from("Mime guess was not a concrete media type!"))
        })?;

        Self::register_content(content_registry, index, route, media_type.clone(), || {
            RegisteredContent::StaticContentItem(StaticContentItem::new(
                file.into_file(),
                media_type,
            ))
        })
    }

    /// Content files with two extensions are either templates or executables
    /// (depending on the final extension and whether the executable bit is
    /// set). In both cases the first extension indicates the media type that
    /// will be produced when the content is rendered.
    fn register_file_with_two_extensions(
        file: ContentFile,
        first_extension: &str,
        second_extension: &str,
        index: &mut ContentIndexEntries,
        content_registry: &mut ContentRegistry,
        handlebars_registry: &mut Handlebars,
    ) -> Result<(), ContentLoadingError> {
        let route = Route::new(file.relative_path_without_extensions());
        match [first_extension, second_extension] {
            // Handlebars templates are named like foo.html.hbs and do not
            // have the executable bit set. When rendered they are evaluated by
            // soliton.
            [first_extension, Self::HANDLEBARS_FILE_EXTENSION] => {
                if file.is_executable() {
                    return Err(ContentLoadingError::ContentFileNameError(
                        format!(
                            "The content file '{}' appears to be a handlebars file (because it ends in '.{}'), \
                            but it is also executable. It must be one or the other.",
                            file.relative_path(),
                            Self::HANDLEBARS_FILE_EXTENSION,
                        ),
                    ));
                }

                let mime = MimeGuess::from_ext(first_extension)
                    .first()
                    .ok_or_else(|| ContentLoadingError::UnknownFileType(
                        format!(
                            "The first filename extension for the handlebars template at '{}' ('{}') \
                            does not map to any known media type.",
                            file.relative_path(),
                            first_extension,
                        ),
                    ))?;
                let media_type = MediaType::from_media_range(mime).ok_or_else(|| {
                    ContentLoadingError::Bug(String::from(
                        "Mime guess was not a concrete media type!",
                    ))
                })?;

                // Note that templates are keyed by relative path + extensions
                // in the handlebars registry, not the extensionless routes
                // used elsewhere. This is necessary to allow alternative
                // representations for templates (foo.html.hbs and foo.md.hbs
                // need to both live in the handlebars registry under distinct
                // names).
                let template_name = file.relative_path();
                let mut contents = file.file();
                if handlebars_registry.has_template(&template_name) {
                    return Err(ContentLoadingError::Bug(format!(
                        "More than one handlebars template has the name '{}'.",
                        template_name,
                    )));
                }
                handlebars_registry
                    .register_template_source(&template_name, &mut contents)
                    .map_err(|template_render_error| match template_render_error {
                        handlebars::TemplateFileError::TemplateError(source) => {
                            ContentLoadingError::TemplateParseError(TemplateParseError { source })
                        }
                        handlebars::TemplateFileError::IOError(source, original_name) => {
                            // Handlebars-rust will use an empty string when the
                            // error does not correspond to a specific path.
                            let name = if original_name.is_empty() {
                                None
                            } else {
                                Some(original_name)
                            };
                            ContentLoadingError::IOError { source, name }
                        }
                    })?;

                Self::register_content(content_registry, index, route, media_type.clone(), || {
                    RegisteredContent::RegisteredTemplate(RegisteredTemplate::new(
                        template_name,
                        media_type,
                    ))
                })
            }

            // Executable programs are named like foo.html.py and must have the
            // executable bit set in their file permissions. When rendered they
            // will executed by the OS in a separate process.
            [first_extension, _arbitrary_second_extension] if file.is_executable() => {
                let mime =
                    MimeGuess::from_ext(first_extension)
                        .first()
                        .ok_or_else(|| ContentLoadingError::UnknownFileType(
                            format!(
                                "The first filename extension for the executable at '{}' ('{}') does not map to any known media type.",
                                file.relative_path(),
                                first_extension,
                            ),
                        ))?;
                let media_type = MediaType::from_media_range(mime).ok_or_else(|| {
                    ContentLoadingError::Bug(String::from(
                        "Mime guess was not a concrete media type!",
                    ))
                })?;

                // The working directory for the executable is the immediate
                // parent directory it resides in (which may be a child of the
                // content directory).
                let working_directory =
                    Path::new(file.absolute_path()).parent().ok_or_else(|| {
                        // This indicates a bug because it can only occur if
                        // `entry.absolute_path()` is the filesystem root, but
                        // we should have already verified that `entry` is a
                        // file (not a directory). If it's the filesystem root
                        // then it is a directory.
                        ContentLoadingError::Bug(format!(
                            "Failed to get a parent directory for the executable at '{}'.",
                            file.absolute_path(),
                        ))
                    })?;

                Self::register_content(content_registry, index, route, media_type.clone(), || {
                    RegisteredContent::Executable(Executable::new(
                        file.absolute_path(),
                        working_directory,
                        media_type,
                    ))
                })
            }

            [first_unsupported_extension, second_unsupported_extension] => {
                Err(ContentLoadingError::ContentFileNameError(format!(
                    "The content file '{}' has two extensions ('{}.{}'), but is \
                        neither a handlebars template nor an executable.",
                    file.relative_path(),
                    first_unsupported_extension,
                    second_unsupported_extension
                )))
            }
        }
    }

    fn register_content<F>(
        content_registry: &mut ContentRegistry,
        content_index: &mut ContentIndexEntries,
        route: Route,
        media_type: MediaType,
        create_content: F,
    ) -> Result<(), ContentLoadingError>
    where
        F: FnOnce() -> RegisteredContent,
    {
        content_index.try_add(&route)?;
        let representations = content_registry
            .entry(route.clone())
            .or_insert_with(HashMap::new);

        match representations.entry(media_type) {
            Entry::Occupied(entry) => {
                let (media_type, _) = entry.remove_entry();
                Err(ContentLoadingError::DuplicateContent {
                    route: String::from(route.as_ref()),
                    media_type,
                })
            }
            Entry::Vacant(entry) => {
                entry.insert(create_content());
                Ok(())
            }
        }
    }
}

impl<'engine, ServerInfo, ErrorCode> ContentEngine<ServerInfo, ErrorCode>
    for FilesystemBasedContentEngine<'engine, ServerInfo, ErrorCode>
where
    ErrorCode: Clone + Serialize,
    ServerInfo: Clone + Serialize,
{
    fn get_render_context(
        &self,
        request_route: &str,
    ) -> RenderContext<ServerInfo, ErrorCode, Self> {
        RenderContext {
            content_engine: self,
            data: RenderData {
                server_info: self.server_info.clone(),
                index: self.index.clone(),
                request_route: String::from(request_route),
                target_media_type: None,
                error_code: None,
            },
        }
    }

    fn new_template(
        &self,
        handlebars_source: &str,
        media_type: MediaType,
    ) -> Result<UnregisteredTemplate, TemplateParseError> {
        UnregisteredTemplate::from_source(handlebars_source, media_type)
    }

    fn get(&self, route: &str) -> Option<&ContentRepresentations> {
        self.content_registry.get(&Route::new(route))
    }

    fn handlebars_registry(&self) -> &Handlebars {
        &self.handlebars_registry
    }
}

#[cfg(test)]
mod tests {
    use super::test_lib::*;
    use super::*;
    use crate::test_lib::*;
    use ::mime;

    type TestContentEngine<'a> = FilesystemBasedContentEngine<'a, (), ()>;

    // FIXME: It's not ideal to rely on specific example directories in these
    // tests. It would be better to mock out contents in each of the tests.

    #[test]
    fn content_engine_can_be_created_from_valid_content_directory() {
        for directory in example_content_directories_with_valid_contents() {
            if let Err(error) = TestContentEngine::from_content_directory(directory, ()) {
                panic!("Content engine could not be created: {}", error);
            }
        }
    }

    #[test]
    fn content_engine_cannot_be_created_from_invalid_content_directory() {
        for directory in example_content_directories_with_invalid_contents() {
            assert!(
                TestContentEngine::from_content_directory(directory, ()).is_err(),
                "Content engine was successfully created, but this should have failed",
            );
        }
    }

    #[test]
    fn new_templates_can_be_rendered() {
        let shared_content_engine = TestContentEngine::from_content_directory(
            arbitrary_content_directory_with_valid_content(),
            (),
        )
        .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        for &(template, expected_output) in &VALID_TEMPLATES {
            let renderable = content_engine
                .new_template(
                    template,
                    MediaType::from_media_range(mime::TEXT_HTML).unwrap(),
                )
                .expect("Template could not be parsed");
            let mut rendered = renderable
                .render(content_engine.get_render_context(""), &[mime::TEXT_HTML])
                .expect(&format!("Template rendering failed for `{}`", template));
            let actual_output = media_to_string(&mut rendered);

            assert_eq!(
                actual_output,
                expected_output,
                "Template rendering for `{}` did not produce the expected output (\"{}\"), instead got \"{}\"",
                template,
                expected_output,
                actual_output,
            );
        }
    }

    #[test]
    fn new_template_fails_for_invalid_templates() {
        let shared_content_engine = TestContentEngine::from_content_directory(
            arbitrary_content_directory_with_valid_content(),
            (),
        )
        .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        for &template in &INVALID_TEMPLATES {
            let result = content_engine.new_template(
                template,
                MediaType::from_media_range(mime::TEXT_HTML).unwrap(),
            );

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
        let shared_content_engine = TestContentEngine::from_content_directory(directory, ())
            .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let template = "this is partial: {{> abc.html.hbs}}";
        let expected_output =
            "this is partial: a\nb\n\nc\n\nsubdirectory entries:\nsubdirectory/c\n";

        let renderable = content_engine
            .new_template(
                template,
                MediaType::from_media_range(mime::TEXT_HTML).unwrap(),
            )
            .expect("Template could not be parsed");
        let mut rendered = renderable
            .render(content_engine.get_render_context(""), &[mime::TEXT_HTML])
            .expect(&format!("Template rendering failed for `{}`", template));
        let actual_output = media_to_string(&mut rendered);

        assert_eq!(
            actual_output,
            expected_output,
            "Template rendering for `{}` did not produce the expected output (\"{}\"), instead got \"{}\"",
            template,
            expected_output,
            actual_output,
        );
    }

    #[test]
    fn content_can_be_retrieved() {
        let directory = ContentDirectory::from_root(&example_path("partials")).unwrap();
        let shared_content_engine = TestContentEngine::from_content_directory(directory, ())
            .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let route = "abc";
        let expected_output = "a\nb\n\nc\n\nsubdirectory entries:\nsubdirectory/c\n";

        let content = content_engine
            .get(route)
            .expect("Content could not be found");
        let mut rendered = content
            .render(content_engine.get_render_context(""), &[mime::TEXT_HTML])
            .expect(&format!(
                "Template rendering failed for content at '{}'",
                route
            ));
        let actual_output = media_to_string(&mut rendered);

        assert_eq!(
            actual_output,
            expected_output,
            "Rendering content at '{}' did not produce the expected output (\"{}\"), instead got \"{}\"",
            route,
            expected_output,
            actual_output,
        );
    }

    #[test]
    fn content_may_not_exist_at_route() {
        let directory = ContentDirectory::from_root(&example_path("hello-world")).unwrap();
        let shared_content_engine = TestContentEngine::from_content_directory(directory, ())
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
    fn handlebars_extension_agrees_with_mime_guess() {
        let mime_guess_handlebars_extension =
            mime_guess::get_extensions("text", "x-handlebars-template")
                .unwrap()
                .first()
                .unwrap();
        let content_engine_handlebars_extension = TestContentEngine::HANDLEBARS_FILE_EXTENSION;

        assert_eq!(
            mime_guess_handlebars_extension,
            &content_engine_handlebars_extension,
        );
    }

    #[test]
    fn get_helper_is_available() {
        let directory = ContentDirectory::from_root(&example_path("partials")).unwrap();
        let shared_content_engine = TestContentEngine::from_content_directory(directory, ())
            .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let template = "i got stuff: {{get [/].b}}";
        let expected_output = "i got stuff: b\n";

        let renderable = content_engine
            .new_template(
                template,
                MediaType::from_media_range(mime::TEXT_HTML).unwrap(),
            )
            .expect("Template could not be parsed");
        let mut rendered = renderable
            .render(content_engine.get_render_context(""), &[mime::TEXT_HTML])
            .expect(&format!("Template rendering failed for `{}`", template));
        let actual_output = media_to_string(&mut rendered);

        assert_eq!(
            actual_output,
            expected_output,
            "Template rendering for `{}` did not produce the expected output (\"{}\"), instead got \"{}\"",
            template,
            expected_output,
            actual_output,
        );
    }

    #[test]
    fn get_helper_requires_a_route_argument() {
        let directory = ContentDirectory::from_root(&example_path("partials")).unwrap();
        let shared_content_engine = TestContentEngine::from_content_directory(directory, ())
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
                .new_template(
                    template,
                    MediaType::from_media_range(mime::TEXT_HTML).unwrap(),
                )
                .expect("Template could not be parsed");
            let result =
                renderable.render(content_engine.get_render_context(""), &[mime::TEXT_HTML]);
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
        let shared_content_engine = TestContentEngine::from_content_directory(directory, ())
            .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let routes = ["cannot-become-html", "template-cannot-become-html"];

        for route in routes.iter() {
            match content_engine.get(route) {
                None => panic!("No content was found at '{}'", route),
                Some(renderable) => {
                    let result = renderable
                        .render(content_engine.get_render_context(""), &[mime::TEXT_HTML]);
                    assert!(
                        result.is_err(),
                        "Content was successfully rendered for `{}`, but this should have failed \
                        because its media type cannot become html",
                        route,
                    );
                }
            }
        }
    }

    #[test]
    fn anonymous_template_cannot_be_rendered_with_unacceptable_target_media_type() {
        let shared_content_engine = TestContentEngine::from_content_directory(
            arbitrary_content_directory_with_valid_content(),
            (),
        )
        .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let template = content_engine
            .new_template(
                "<p>hi</p>",
                MediaType::from_media_range(mime::TEXT_HTML).unwrap(),
            )
            .expect("Template could not be created");
        let result = template.render(content_engine.get_render_context(""), &[mime::TEXT_PLAIN]);

        assert!(
            result.is_err(),
            "Template was successfully rendered with unacceptable media type",
        );
    }

    #[test]
    fn nesting_incompatible_media_types_fails_at_render_time() {
        let content_directory_path = &example_path("media-types");
        let directory = ContentDirectory::from_root(content_directory_path).unwrap();
        let shared_content_engine = TestContentEngine::from_content_directory(directory, ())
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
                        .render(content_engine.get_render_context(""), &[target_media_type]);
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
    fn target_media_type_is_correct_for_templates_rendered_directly() {
        let shared_content_engine = TestContentEngine::from_content_directory(
            ContentDirectory::from_root(&example_path("media-types")).unwrap(),
            (),
        )
        .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        // Test both registered and unregistered templates.
        let test_cases = [
            (
                media_to_string(
                    &mut content_engine
                        .new_template(
                            "{{target-media-type}}",
                            MediaType::from_media_range(mime::TEXT_PLAIN).unwrap(),
                        )
                        .expect("Test template was invalid")
                        .render(content_engine.get_render_context(""), &[mime::TEXT_PLAIN])
                        .expect("Failed to render unregistered template"),
                ),
                mime::TEXT_PLAIN.essence_str(),
            ),
            (
                media_to_string(
                    &mut content_engine
                        .get("echo-target-media-type")
                        .expect("Test template does not exist")
                        .render(content_engine.get_render_context(""), &[mime::TEXT_HTML])
                        .expect("Failed to render registered template"),
                ),
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
        let shared_content_engine = TestContentEngine::from_content_directory(directory, ())
            .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let route = "count-cli-args";
        let expected_output = "0\n";

        let content = content_engine
            .get(route)
            .expect("Content could not be found");
        let mut rendered = content
            .render(content_engine.get_render_context(""), &[mime::TEXT_PLAIN])
            .expect(&format!("Rendering failed for content at '{}'", route));
        let actual_output = media_to_string(&mut rendered);

        assert_eq!(
            actual_output,
            expected_output,
            "Rendering content at '{}' did not produce the expected output (\"{}\"), instead got \"{}\"",
            route,
            expected_output,
            actual_output,
        );
    }

    #[test]
    fn executables_are_executed_with_correct_working_directory() {
        let directory = ContentDirectory::from_root(&example_path("executables")).unwrap();
        let shared_content_engine = TestContentEngine::from_content_directory(directory, ())
            .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let route1 = "pwd";
        let expected_output1 = format!("{}/src/examples/executables\n", PROJECT_DIRECTORY);

        let content = content_engine
            .get(route1)
            .expect("Content could not be found");
        let mut rendered = content
            .render(content_engine.get_render_context(""), &[mime::TEXT_PLAIN])
            .expect(&format!("Rendering failed for content at '{}'", route1));
        let actual_output = media_to_string(&mut rendered);

        assert_eq!(
            actual_output,
            expected_output1,
            "Rendering content at '{}' did not produce the expected output (\"{}\"), instead got \"{}\"",
            route1,
            expected_output1,
            actual_output,
        );

        let route2 = "subdirectory/pwd";
        let expected_output2 = format!(
            "{}/src/examples/executables/subdirectory\n",
            PROJECT_DIRECTORY
        );

        let content = content_engine
            .get(route2)
            .expect("Content could not be found");
        let mut rendered = content
            .render(content_engine.get_render_context(""), &[mime::TEXT_PLAIN])
            .expect(&format!("Rendering failed for content at '{}'", route2));
        let actual_output = media_to_string(&mut rendered);

        assert_eq!(
            actual_output,
            expected_output2,
            "Rendering content at '{}' did not produce the expected output (\"{}\"), instead got \"{}\"",
            route2,
            expected_output2,
            actual_output,
        );
    }

    #[test]
    fn executables_have_a_media_type() {
        let directory = ContentDirectory::from_root(&example_path("executables")).unwrap();
        let shared_content_engine = TestContentEngine::from_content_directory(directory, ())
            .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let route = "SKIP-SNAPSHOT-system-info"; // This outputs text/html.
        let content = content_engine
            .get(route)
            .expect("Content could not be found");

        let result1 = content.render(content_engine.get_render_context(""), &[mime::TEXT_PLAIN]); // Not text/html!
        assert!(
            result1.is_err(),
            "Rendering content at '{}' succeeded when it should have failed",
            route,
        );

        let result2 = content.render(content_engine.get_render_context(""), &[mime::TEXT_HTML]);
        assert!(
            result2.is_ok(),
            "Rendering content at '{}' failed when it should have succeeded",
            route,
        );
    }

    #[test]
    fn executables_can_output_arbitrary_bytes() {
        let directory = ContentDirectory::from_root(&example_path("executables")).unwrap();
        let shared_content_engine = TestContentEngine::from_content_directory(directory, ())
            .expect("Content engine could not be created");
        let content_engine = shared_content_engine.read().unwrap();

        let route = "SKIP-SNAPSHOT-random";
        let content = content_engine
            .get(route)
            .expect("Content could not be found");

        let media = content
            .render(
                content_engine.get_render_context(""),
                &[mime::APPLICATION_OCTET_STREAM],
            )
            .expect(&format!(
                "Rendering content at '{}' failed when it should have succeeded",
                route
            ));

        assert!(
            media.media_type
                == MediaType::from_media_range(mime::APPLICATION_OCTET_STREAM).unwrap(),
            "Media type was not correct"
        );
    }

    #[test]
    fn templates_can_get_executable_output() {
        let directory = ContentDirectory::from_root(&example_path("executables")).unwrap();
        let shared_content_engine = TestContentEngine::from_content_directory(directory, ())
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
        let mut rendered = content
            .render(content_engine.get_render_context(""), &[mime::TEXT_PLAIN])
            .expect(&format!("Rendering failed for content at '{}'", route));
        let actual_output = media_to_string(&mut rendered);

        assert_eq!(
            actual_output,
            expected_output,
            "Rendering content at '{}' did not produce the expected output (\"{}\"), instead got \"{}\"",
            route,
            expected_output,
            actual_output,
        );
    }

    #[test]
    fn content_can_be_hidden() {
        let directory = ContentDirectory::from_root(&example_path("hidden-content")).unwrap();
        let shared_content_engine = TestContentEngine::from_content_directory(directory, ())
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
