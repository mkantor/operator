use super::*;
use handlebars::{self, Handlebars, Renderable as _};
use std::fs::{self, File};
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{ChildStdout, Command, Stdio};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContentRenderingError {
    #[error(transparent)]
    RenderingFailure(RenderingFailure),

    #[error("Unable to provide the requested content in an acceptable media type.")]
    CannotProvideAcceptableMediaType,
}

#[derive(Error, Debug)]
pub enum RenderingFailure {
    #[error(
        "Rendering failed for template: {}",
        .source
    )]
    TemplateRenderError {
        #[from]
        source: handlebars::RenderError,
    },

    #[error(
        "Executable '{}' with working directory '{}' could not be successfully executed: {}",
        .program,
        .working_directory.display(),
        .message,
    )]
    ExecutableError {
        program: String,
        working_directory: PathBuf,
        message: String,
    },

    #[error(
        "Executable '{}' with working directory '{}' exited with code {}{}",
        .program,
        .working_directory.display(),
        .exit_code,
        .stderr_contents.as_ref().map(|message| format!(": {}", message)).unwrap_or_default(),
    )]
    ExecutableExitedWithNonzero {
        program: String,
        working_directory: PathBuf,
        exit_code: i32,
        stderr_contents: Option<String>,
    },

    #[error("Input/output error during rendering")]
    IOError {
        #[from]
        source: io::Error,
    },
}

/// A static file from the content directory (such as an image or a text file).
pub struct StaticContentItem {
    contents: fs::File,
    media_type: MediaType,
}
impl StaticContentItem {
    pub fn new(contents: fs::File, media_type: MediaType) -> Self {
        StaticContentItem {
            contents,
            media_type,
        }
    }

    fn render_to_native_media_type(&self) -> Result<Media<File>, RenderingFailure> {
        // We clone the file handle and operate on that to avoid taking
        // self as mut. Note that all clones share a cursor, so seeking
        // back to the beginning is necessary to ensure we read the
        // entire file.
        let mut file = self.contents.try_clone()?;
        file.seek(SeekFrom::Start(0))?;
        Ok(Media::new(self.media_type.clone(), file))
    }
}
impl Render for StaticContentItem {
    type Output = File;
    fn render<'a, E: ContentEngine, A: IntoIterator<Item = &'a MediaRange>>(
        &self,
        _context: RenderContext<E>,
        acceptable_media_ranges: A,
    ) -> Result<Media<Self::Output>, ContentRenderingError> {
        negotiate_content(acceptable_media_ranges, |target| {
            if !&self.media_type.is_within_media_range(target) {
                None
            } else {
                Some(self.render_to_native_media_type())
            }
        })
    }
}

/// A handlebars template that came from the content directory.
pub struct RegisteredTemplate {
    name_in_registry: String,
    rendered_media_type: MediaType,
}
impl RegisteredTemplate {
    pub fn new<S: AsRef<str>>(name_in_registry: S, rendered_media_type: MediaType) -> Self {
        RegisteredTemplate {
            name_in_registry: String::from(name_in_registry.as_ref()),
            rendered_media_type,
        }
    }

    fn render_to_native_media_type(
        &self,
        handlebars_registry: &Handlebars,
        render_data: RenderData,
    ) -> Result<Media<Cursor<String>>, RenderingFailure> {
        let render_data = RenderData {
            target_media_type: Some(self.rendered_media_type.clone()),
            ..render_data
        };
        let rendered_content = handlebars_registry.render(&self.name_in_registry, &render_data)?;
        Ok(Media::new(
            self.rendered_media_type.clone(),
            Cursor::new(rendered_content),
        ))
    }
}
impl Render for RegisteredTemplate {
    type Output = Cursor<String>;
    fn render<'a, E: ContentEngine, A: IntoIterator<Item = &'a MediaRange>>(
        &self,
        context: RenderContext<E>,
        acceptable_media_ranges: A,
    ) -> Result<Media<Self::Output>, ContentRenderingError> {
        negotiate_content(acceptable_media_ranges, |target| {
            if !&self.rendered_media_type.is_within_media_range(target) {
                None
            } else {
                Some(self.render_to_native_media_type(
                    context.content_engine.handlebars_registry(),
                    context.data.clone(),
                ))
            }
        })
    }
}

/// An anonymous handlebars template that is not from the content directory.
pub struct UnregisteredTemplate {
    template: handlebars::Template,
    rendered_media_type: MediaType,
}
impl UnregisteredTemplate {
    pub fn from_source<S: AsRef<str>>(
        handlebars_source: S,
        rendered_media_type: MediaType,
    ) -> Result<Self, UnregisteredTemplateParseError> {
        let template = handlebars::Template::compile2(handlebars_source, true)?;
        Ok(UnregisteredTemplate {
            template,
            rendered_media_type,
        })
    }

    fn render_to_native_media_type(
        &self,
        handlebars_registry: &Handlebars,
        render_data: RenderData,
    ) -> Result<Media<Cursor<String>>, RenderingFailure> {
        let render_data = RenderData {
            target_media_type: Some(self.rendered_media_type.clone()),
            ..render_data
        };
        let handlebars_context = handlebars::Context::wraps(&render_data)?;
        let mut handlebars_render_context = handlebars::RenderContext::new(None);
        let rendered_content = self.template.renders(
            handlebars_registry,
            &handlebars_context,
            &mut handlebars_render_context,
        )?;
        Ok(Media::new(
            self.rendered_media_type.clone(),
            Cursor::new(rendered_content),
        ))
    }
}
impl Render for UnregisteredTemplate {
    type Output = Cursor<String>;
    fn render<'a, E: ContentEngine, A: IntoIterator<Item = &'a MediaRange>>(
        &self,
        context: RenderContext<E>,
        acceptable_media_ranges: A,
    ) -> Result<Media<Self::Output>, ContentRenderingError> {
        negotiate_content(acceptable_media_ranges, |target| {
            if !&self.rendered_media_type.is_within_media_range(target) {
                None
            } else {
                Some(self.render_to_native_media_type(
                    context.content_engine.handlebars_registry(),
                    context.data.clone(),
                ))
            }
        })
    }
}

/// A program that can be run by the operating system, e.g. a shell script.
///
/// If the executed program terminates with a nonzero exit code, rendering
/// output is the contents of standard output. Otherwise a rendering failure
/// occurs.
///
/// Currently the render context is not accessible from the program. A future
/// version could provide it via environment variables or some other mechanism.
pub struct Executable {
    program: String,
    working_directory: PathBuf,
    output_media_type: MediaType,
}
impl Executable {
    pub fn new<P: AsRef<str>, W: AsRef<Path>>(
        program: P,
        working_directory: W,
        output_media_type: MediaType,
    ) -> Self {
        Executable {
            program: String::from(program.as_ref()),
            working_directory: PathBuf::from(working_directory.as_ref()),
            output_media_type,
        }
    }

    fn render_to_native_media_type(&self) -> Result<Media<ChildStdout>, RenderingFailure> {
        let mut command = Command::new(self.program.clone());

        let mut child = command
            .current_dir(self.working_directory.clone())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|io_error| RenderingFailure::ExecutableError {
                message: format!("Unable to execute program: {}", io_error),
                program: self.program.clone(),
                working_directory: self.working_directory.clone(),
            })?;

        let exit_status = child
            .wait()
            .map_err(|error| RenderingFailure::ExecutableError {
                message: format!("Could not get exit status from program: {}", error),
                program: self.program.clone(),
                working_directory: self.working_directory.clone(),
            })?;

        let stdout = child
            .stdout
            .ok_or_else(|| RenderingFailure::ExecutableError {
                message: String::from("Could not capture stdout from program."),
                program: self.program.clone(),
                working_directory: self.working_directory.clone(),
            })?;

        if !exit_status.success() {
            Err(match exit_status.code() {
                Some(exit_code) => {
                    let stderr_contents = {
                        child.stderr.and_then(|mut stderr| {
                            let mut error_message = String::new();
                            match stderr.read_to_string(&mut error_message) {
                                Err(_) | Ok(0) => None,
                                Ok(_) => Some(error_message),
                            }
                        })
                    };
                    RenderingFailure::ExecutableExitedWithNonzero {
                        stderr_contents,
                        program: self.program.clone(),
                        exit_code,
                        working_directory: self.working_directory.clone(),
                    }
                }

                None => RenderingFailure::ExecutableError {
                    message: String::from(
                        "Program exited with failure, but its exit code was not available. It may have been killed by a signal."
                    ),
                    program: self.program.clone(),
                    working_directory: self.working_directory.clone(),
                }
            })
        } else {
            Ok(Media::new(self.output_media_type.clone(), stdout))
        }
    }
}
impl Render for Executable {
    type Output = ChildStdout;
    fn render<'a, E: ContentEngine, A: IntoIterator<Item = &'a MediaRange>>(
        &self,
        _context: RenderContext<E>,
        acceptable_media_ranges: A,
    ) -> Result<Media<Self::Output>, ContentRenderingError> {
        negotiate_content(acceptable_media_ranges, |target| {
            if !&self.output_media_type.is_within_media_range(target) {
                None
            } else {
                Some(self.render_to_native_media_type())
            }
        })
    }
}

/// Attempt to render some content into a media type that satisfies one of the
/// given `acceptable_media_ranges`.
///
/// The provided `attempt_render` function may be called multiple times. It
/// should return `None` if the passed media range cannot be satisfied, or
/// `Some(Err(_))` if there was another problem. Negotiation will keep trying
/// media ranges until one can be successfully rendered or all acceptable
/// ranges are exhausted.
fn negotiate_content<'a, T, F, A>(
    acceptable_media_ranges: A,
    attempt_render: F,
) -> Result<T, ContentRenderingError>
where
    F: Fn(&MediaRange) -> Option<Result<T, RenderingFailure>>,
    A: IntoIterator<Item = &'a MediaRange>,
{
    let mut errors = Vec::new();
    for acceptable_media_range in acceptable_media_ranges {
        match attempt_render(acceptable_media_range) {
            Some(Ok(rendered)) => return Ok(rendered),
            Some(Err(error)) => {
                log::warn!("Rendering failure: {}", error);
                errors.push(error)
            }
            None => (),
        };
    }

    // If there are no errors then we must've gotten all Nones (meaning that
    // none of the available media types were acceptable). Otherwise, just
    // use the first error.
    Err(match errors.into_iter().next() {
        None => ContentRenderingError::CannotProvideAcceptableMediaType,
        Some(first_error) => ContentRenderingError::RenderingFailure(first_error),
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_lib::*;
    use super::super::*;
    use super::*;
    use crate::test_lib::*;
    use ::mime;
    use std::fs;
    use std::io::Write;
    use std::str;
    use tempfile::tempfile;

    enum Renderable {
        StaticContentItem(StaticContentItem),
        Executable(Executable),
        RegisteredTemplate(RegisteredTemplate),
        UnregisteredTemplate(UnregisteredTemplate),
    }
    impl Renderable {
        fn box_media<'o, O: Read + 'o>(media: Media<O>) -> Media<Box<dyn Read + 'o>> {
            Media {
                content: Box::new(media.content),
                media_type: media.media_type,
            }
        }
    }
    impl Render for Renderable {
        type Output = Box<dyn Read>;
        fn render<'a, E: ContentEngine, A: IntoIterator<Item = &'a MediaRange>>(
            &self,
            context: RenderContext<E>,
            acceptable_media_ranges: A,
        ) -> Result<Media<Self::Output>, ContentRenderingError> {
            match self {
                Self::StaticContentItem(renderable) => renderable
                    .render(context, acceptable_media_ranges)
                    .map(Renderable::box_media),
                Self::Executable(renderable) => renderable
                    .render(context, acceptable_media_ranges)
                    .map(Renderable::box_media),
                Self::RegisteredTemplate(renderable) => renderable
                    .render(context, acceptable_media_ranges)
                    .map(Renderable::box_media),
                Self::UnregisteredTemplate(renderable) => renderable
                    .render(context, acceptable_media_ranges)
                    .map(Renderable::box_media),
            }
        }
    }

    /// Test fixtures. All of these will render to an empty string with media
    /// type text/plain.
    fn example_renderables() -> (impl ContentEngine, Vec<Renderable>) {
        let text_plain_type = MediaType::from_media_range(mime::TEXT_PLAIN).unwrap();
        let mut content_engine = MockContentEngine::new();
        content_engine
            .register_template("registered-template", "")
            .unwrap();
        let empty_file = tempfile().expect("Failed to create temporary file");
        (
            content_engine,
            vec![
                Renderable::StaticContentItem(StaticContentItem::new(
                    empty_file,
                    text_plain_type.clone(),
                )),
                Renderable::Executable(Executable::new(
                    "true",
                    PROJECT_DIRECTORY,
                    text_plain_type.clone(),
                )),
                Renderable::RegisteredTemplate(RegisteredTemplate::new(
                    "registered-template",
                    text_plain_type.clone(),
                )),
                Renderable::UnregisteredTemplate(
                    UnregisteredTemplate::from_source("", text_plain_type.clone())
                        .expect("Failed to create test template"),
                ),
            ],
        )
    }

    #[test]
    fn static_content_is_stringified_when_rendered() {
        let mut file = tempfile().expect("Failed to create temporary file");
        write!(file, "hello world").expect("Failed to write to temporary file");
        let static_content = StaticContentItem {
            media_type: MediaType::from_media_range(mime::TEXT_PLAIN).unwrap(),
            contents: file,
        };
        let mut output = static_content
            .render(
                MockContentEngine::new().get_render_context(),
                &[mime::TEXT_PLAIN],
            )
            .expect("Render failed");

        assert_eq!(media_to_string(&mut output), String::from("hello world"));
    }

    #[test]
    fn static_content_can_be_arbitrary_bytes() {
        let non_utf8_bytes = &[0xfe, 0xfe, 0xff, 0xff];
        assert!(str::from_utf8(non_utf8_bytes).is_err());

        let mut file = tempfile().expect("Failed to create temporary file");
        file.write(non_utf8_bytes)
            .expect("Failed to write to temporary file");
        let static_content = StaticContentItem {
            media_type: MediaType::from_media_range(mime::APPLICATION_OCTET_STREAM).unwrap(),
            contents: file,
        };
        let mut output = static_content
            .render(
                MockContentEngine::new().get_render_context(),
                &[mime::APPLICATION_OCTET_STREAM],
            )
            .expect("Render failed");

        assert_eq!(media_to_bytes(&mut output), non_utf8_bytes);
    }

    #[test]
    fn static_content_must_match_media_type_to_render() {
        let source_media_type = MediaType::from_media_range(mime::TEXT_XML).unwrap();
        let target_media_type = mime::IMAGE_PNG;

        let mut file = tempfile().expect("Failed to create temporary file");
        write!(file, "hello world").expect("Failed to write to temporary file");
        let static_content = StaticContentItem {
            media_type: source_media_type,
            contents: file,
        };
        let render_result = static_content.render(
            MockContentEngine::new().get_render_context(),
            &[target_media_type],
        );

        assert!(render_result.is_err());
    }

    #[test]
    fn rendering_with_empty_acceptable_media_ranges_should_fail() {
        let (mock_engine, renderables) = example_renderables();
        for (index, renderable) in renderables.iter().enumerate() {
            let render_result = renderable.render(mock_engine.get_render_context(), &[]);
            assert!(
                render_result.is_err(),
                "Rendering item {} with an empty list of acceptable media types did not fail as expected",
                index,
            )
        }
    }

    #[test]
    fn rendering_with_only_unacceptable_media_ranges_should_fail() {
        let (mock_engine, renderables) = example_renderables();
        for (index, renderable) in renderables.iter().enumerate() {
            let render_result = renderable.render(
                mock_engine.get_render_context(),
                &[mime::IMAGE_GIF, mime::APPLICATION_PDF, mime::TEXT_CSS],
            );
            assert!(
                render_result.is_err(),
                "Rendering item {} with unacceptable media types did not fail as expected",
                index,
            )
        }
    }

    #[test]
    fn rendering_with_acceptable_media_range_that_is_not_most_preferred_should_succeed() {
        let (mock_engine, renderables) = example_renderables();
        for (index, renderable) in renderables.iter().enumerate() {
            let render_result = renderable.render(
                mock_engine.get_render_context(),
                &[mime::IMAGE_GIF, mime::TEXT_PLAIN, mime::TEXT_CSS],
            );
            assert!(
                render_result.is_ok(),
                "Rendering item {} with acceptable media type did not succeed as expected: {}",
                index,
                render_result.err().unwrap(),
            );
            assert!(
                render_result.unwrap().media_type
                    == MediaType::from_media_range(mime::TEXT_PLAIN).unwrap(),
                "Rendering item {} did not produce expected media type",
                index,
            );
        }
    }

    #[test]
    fn executables_execute_when_rendered() {
        let path = format!("{}/src", PROJECT_DIRECTORY);
        let working_directory =
            fs::canonicalize(path).expect("Could not canonicalize path for test");
        let executable = Executable::new(
            "pwd",
            working_directory.clone(),
            MediaType::from_media_range(mime::TEXT_PLAIN).unwrap(),
        );
        let mut output = executable
            .render(
                MockContentEngine::new().get_render_context(),
                &[mime::TEXT_PLAIN],
            )
            .expect("Executable failed but it should have succeeded");

        assert_eq!(
            media_to_string(&mut output),
            format!("{}\n", working_directory.display())
        );
    }

    #[test]
    fn executables_exiting_with_nonzero_are_err() {
        let executable = Executable::new(
            "false",
            PROJECT_DIRECTORY,
            MediaType::from_media_range(mime::TEXT_PLAIN).unwrap(),
        );

        let result = executable.render(
            MockContentEngine::new().get_render_context(),
            &[mime::TEXT_PLAIN],
        );
        assert!(
            result.is_err(),
            "Executable succeeded but it should have failed"
        );
    }

    #[test]
    fn executables_require_working_directory_that_exists() {
        let working_directory = "/hopefully/this/path/does/not/actually/exist/on/your/system";
        let executable = Executable::new(
            "pwd",
            working_directory,
            MediaType::from_media_range(mime::TEXT_PLAIN).unwrap(),
        );

        let result = executable.render(
            MockContentEngine::new().get_render_context(),
            &[mime::TEXT_PLAIN],
        );
        assert!(
            result.is_err(),
            "Executable succeeded but it should have failed"
        );
    }
}
