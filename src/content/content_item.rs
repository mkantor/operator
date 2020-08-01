use super::*;
use handlebars::{self, Handlebars, Renderable as _};
use mime::{self, Mime};
use std::fs;
use std::io;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContentRenderingError {
    #[error(transparent)]
    RenderingFailure(RenderingFailure),

    #[error(
        "Unable to provide content for any of these media ranges: {}.",
        media_ranges_to_human_friendly_list(.acceptable_media_ranges).unwrap_or(String::from("none provided")),
    )]
    CannotProvideAcceptableMediaType { acceptable_media_ranges: Vec<Mime> },
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
    media_type: Mime,
}
impl StaticContentItem {
    pub fn new(contents: fs::File, media_type: Mime) -> Self {
        StaticContentItem {
            contents,
            media_type,
        }
    }

    fn render_to_native_media_type(&self) -> Result<String, RenderingFailure> {
        // We clone the file handle and operate on that to avoid taking
        // self as mut. Note that all clones share a cursor, so seeking
        // back to the beginning is necessary to ensure we read the
        // entire file.
        let mut readable_contents = self.contents.try_clone()?;
        let mut rendered_content = String::new();
        readable_contents.seek(SeekFrom::Start(0))?;
        readable_contents.read_to_string(&mut rendered_content)?;
        Ok(rendered_content)
    }
}
impl Render for StaticContentItem {
    fn render<E: ContentEngine>(
        &self,
        _context: RenderContext<E>,
        acceptable_media_ranges: &[Mime],
    ) -> Result<String, ContentRenderingError> {
        negotiate_content(acceptable_media_ranges, |target| {
            if !media_type_is_within_range(&self.media_type, target) {
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
    rendered_media_type: Mime,
}
impl RegisteredTemplate {
    pub fn new<S: AsRef<str>>(name_in_registry: S, rendered_media_type: Mime) -> Self {
        RegisteredTemplate {
            name_in_registry: String::from(name_in_registry.as_ref()),
            rendered_media_type,
        }
    }

    fn render_to_native_media_type(
        &self,
        handlebars_registry: &Handlebars,
        render_data: RenderData,
    ) -> Result<String, RenderingFailure> {
        let render_data = RenderData {
            source_media_type_of_parent: Some(SerializableMediaRange::from(
                self.rendered_media_type.clone(),
            )),
            ..render_data
        };
        let rendered_content = handlebars_registry.render(&self.name_in_registry, &render_data)?;
        Ok(rendered_content)
    }
}
impl Render for RegisteredTemplate {
    fn render<E: ContentEngine>(
        &self,
        context: RenderContext<E>,
        acceptable_media_ranges: &[Mime],
    ) -> Result<String, ContentRenderingError> {
        negotiate_content(acceptable_media_ranges, |target| {
            if !media_type_is_within_range(&self.rendered_media_type, target) {
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
    rendered_media_type: Mime,
}
impl UnregisteredTemplate {
    pub fn from_source<S: AsRef<str>>(
        handlebars_source: S,
        rendered_media_type: Mime,
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
    ) -> Result<String, RenderingFailure> {
        let render_data = RenderData {
            source_media_type_of_parent: Some(SerializableMediaRange::from(
                self.rendered_media_type.clone(),
            )),
            ..render_data
        };
        let handlebars_context = handlebars::Context::wraps(&render_data)?;
        let mut handlebars_render_context = handlebars::RenderContext::new(None);
        let rendered_content = self.template.renders(
            handlebars_registry,
            &handlebars_context,
            &mut handlebars_render_context,
        )?;
        Ok(rendered_content)
    }
}
impl Render for UnregisteredTemplate {
    fn render<E: ContentEngine>(
        &self,
        context: RenderContext<E>,
        acceptable_media_ranges: &[Mime],
    ) -> Result<String, ContentRenderingError> {
        negotiate_content(acceptable_media_ranges, |target| {
            if !media_type_is_within_range(&self.rendered_media_type, target) {
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
    output_media_type: Mime,
}
impl Executable {
    pub fn new<P: AsRef<str>, W: AsRef<Path>>(
        program: P,
        working_directory: W,
        output_media_type: Mime,
    ) -> Self {
        Executable {
            program: String::from(program.as_ref()),
            working_directory: PathBuf::from(working_directory.as_ref()),
            output_media_type,
        }
    }

    fn render_to_native_media_type(&self) -> Result<String, RenderingFailure> {
        let mut command = Command::new(self.program.clone());
        command.current_dir(self.working_directory.clone());

        let process::Output {
            status,
            stdout,
            stderr,
        } = command
            .output()
            .map_err(|io_error| RenderingFailure::ExecutableError {
                message: format!("Unable to execute program: {}", io_error),
                program: self.program.clone(),
                working_directory: self.working_directory.clone(),
            })?;

        if !status.success() {
            Err(match status.code() {
                Some(exit_code) => RenderingFailure::ExecutableExitedWithNonzero {
                    stderr_contents: match String::from_utf8_lossy(&stderr).as_ref() {
                        "" => None,
                        message => Some(message.to_string()),
                    },
                    program: self.program.clone(),
                    exit_code,
                    working_directory: self.working_directory.clone(),
                },
                None => RenderingFailure::ExecutableError {
                    message: String::from(
                        "Program exited with failure, but its exit code was not available. It may have been killed by a signal."
                    ),
                    program: self.program.clone(),
                    working_directory: self.working_directory.clone(),
                }
            })
        } else {
            let output = String::from_utf8(stdout).map_err(|utf8_error| {
                RenderingFailure::ExecutableError {
                    message: format!(
                        "Program exited with success, but its output was not valid UTF-8: {}",
                        utf8_error
                    ),
                    program: self.program.clone(),
                    working_directory: self.working_directory.clone(),
                }
            })?;
            Ok(output)
        }
    }
}
impl Render for Executable {
    fn render<E: ContentEngine>(
        &self,
        _context: RenderContext<E>,
        acceptable_media_ranges: &[Mime],
    ) -> Result<String, ContentRenderingError> {
        negotiate_content(acceptable_media_ranges, |target| {
            if !media_type_is_within_range(&self.output_media_type, target) {
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
fn negotiate_content<T, F>(
    acceptable_media_ranges: &[Mime],
    attempt_render: F,
) -> Result<T, ContentRenderingError>
where
    F: Fn(&Mime) -> Option<Result<T, RenderingFailure>>,
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
    Err(match errors.into_iter().nth(0) {
        None => ContentRenderingError::CannotProvideAcceptableMediaType {
            acceptable_media_ranges: Vec::from(acceptable_media_ranges),
        },
        Some(first_error) => ContentRenderingError::RenderingFailure(first_error),
    })
}

fn media_type_is_within_range(media_type: &Mime, media_range: &Mime) -> bool {
    if media_range == &mime::STAR_STAR {
        true
    } else if media_range.subtype() == "*" {
        media_type.type_() == media_range.type_()
    } else {
        media_type == media_range
    }
}

fn media_ranges_to_human_friendly_list(media_ranges: &[Mime]) -> Option<String> {
    media_ranges
        .split_first()
        .map(|(first_media_range, other_media_ranges)| {
            other_media_ranges.iter().fold(
                String::from(first_media_range.essence_str()),
                |message, media_range| message + ", " + media_range.essence_str(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::super::test_lib::*;
    use super::super::*;
    use super::*;
    use crate::test_lib::*;
    use std::fs;
    use std::io::Write;
    use tempfile::tempfile;

    enum Renderable {
        StaticContentItem(StaticContentItem),
        Executable(Executable),
        RegisteredTemplate(RegisteredTemplate),
        UnregisteredTemplate(UnregisteredTemplate),
    }
    impl Render for Renderable {
        fn render<E: ContentEngine>(
            &self,
            context: RenderContext<E>,
            acceptable_media_ranges: &[Mime],
        ) -> Result<String, ContentRenderingError> {
            match self {
                Self::StaticContentItem(renderable) => {
                    renderable.render(context, acceptable_media_ranges)
                }
                Self::Executable(renderable) => renderable.render(context, acceptable_media_ranges),
                Self::RegisteredTemplate(renderable) => {
                    renderable.render(context, acceptable_media_ranges)
                }
                Self::UnregisteredTemplate(renderable) => {
                    renderable.render(context, acceptable_media_ranges)
                }
            }
        }
    }

    #[test]
    fn static_content_is_stringified_when_rendered() {
        let mut file = tempfile().expect("Failed to create temporary file");
        write!(file, "hello world").expect("Failed to write to temporary file");
        let static_content = StaticContentItem {
            media_type: mime::TEXT_PLAIN,
            contents: file,
        };
        let output = static_content
            .render(
                MockContentEngine::new().get_render_context(),
                &[mime::TEXT_PLAIN],
            )
            .expect("Render failed");

        assert_eq!(output, String::from("hello world"));
    }

    #[test]
    fn static_content_must_match_media_type_to_render() {
        let source_media_type = mime::TEXT_XML;
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
        let mut mock_engine = MockContentEngine::new();
        mock_engine
            .register_template("registered-template", "")
            .unwrap();
        let empty_file = tempfile().expect("Failed to create temporary file");
        let renderables = [
            Renderable::StaticContentItem(StaticContentItem::new(empty_file, mime::TEXT_PLAIN)),
            Renderable::Executable(Executable::new("true", PROJECT_DIRECTORY, mime::TEXT_PLAIN)),
            Renderable::RegisteredTemplate(RegisteredTemplate::new(
                "registered-template",
                mime::TEXT_PLAIN,
            )),
            Renderable::UnregisteredTemplate(
                UnregisteredTemplate::from_source("", mime::TEXT_PLAIN)
                    .expect("Failed to create test template"),
            ),
        ];

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
        let mut mock_engine = MockContentEngine::new();
        mock_engine
            .register_template("registered-template", "")
            .unwrap();
        let empty_file = tempfile().expect("Failed to create temporary file");
        let renderables = [
            Renderable::StaticContentItem(StaticContentItem::new(empty_file, mime::TEXT_PLAIN)),
            Renderable::Executable(Executable::new("true", PROJECT_DIRECTORY, mime::TEXT_PLAIN)),
            Renderable::RegisteredTemplate(RegisteredTemplate::new(
                "registered-template",
                mime::TEXT_PLAIN,
            )),
            Renderable::UnregisteredTemplate(
                UnregisteredTemplate::from_source("", mime::TEXT_PLAIN)
                    .expect("Failed to create test template"),
            ),
        ];

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
        let mut mock_engine = MockContentEngine::new();
        mock_engine
            .register_template("registered-template", "")
            .unwrap();
        let empty_file = tempfile().expect("Failed to create temporary file");
        let renderables = [
            Renderable::StaticContentItem(StaticContentItem::new(empty_file, mime::TEXT_PLAIN)),
            Renderable::Executable(Executable::new("true", PROJECT_DIRECTORY, mime::TEXT_PLAIN)),
            Renderable::RegisteredTemplate(RegisteredTemplate::new(
                "registered-template",
                mime::TEXT_PLAIN,
            )),
            Renderable::UnregisteredTemplate(
                UnregisteredTemplate::from_source("", mime::TEXT_PLAIN)
                    .expect("Failed to create test template"),
            ),
        ];

        for (index, renderable) in renderables.iter().enumerate() {
            let render_result = renderable.render(
                mock_engine.get_render_context(),
                &[mime::IMAGE_GIF, mime::TEXT_PLAIN, mime::TEXT_CSS],
            );
            assert!(
                render_result.is_ok(),
                "Rendering item {} with acceptable media type did not succeed as expected: {}",
                index,
                render_result.unwrap_err(),
            )
        }
    }

    #[test]
    fn executables_execute_when_rendered() {
        let path = format!("{}/src", PROJECT_DIRECTORY);
        let working_directory =
            fs::canonicalize(path).expect("Could not canonicalize path for test");
        let executable = Executable::new("pwd", working_directory.clone(), mime::TEXT_PLAIN);

        let output = executable
            .render(
                MockContentEngine::new().get_render_context(),
                &[mime::TEXT_PLAIN],
            )
            .expect("Executable failed but it should have succeeded");
        assert_eq!(output, format!("{}\n", working_directory.display()));
    }

    #[test]
    fn executables_exiting_with_nonzero_are_err() {
        let executable = Executable::new("false", PROJECT_DIRECTORY, mime::TEXT_PLAIN);

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
        let executable = Executable::new("pwd", working_directory, mime::TEXT_PLAIN);

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
