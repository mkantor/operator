use super::*;
use handlebars::{self, Renderable as _};
use mime::{self, Mime};
use std::fs;
use std::io;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContentRenderingError {
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

    #[error(
        "Unable to satisfy target media type '{}' from source media type '{}'.",
        .target_media_type,
        .source_media_type,
    )]
    MediaTypeError {
        source_media_type: Mime,
        target_media_type: Mime,
    },
}

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
}
impl Render for StaticContentItem {
    fn render<E: ContentEngine>(
        &self,
        _context: RenderContext<E>,
        acceptable_media_ranges: &[Mime],
    ) -> Result<String, ContentRenderingError> {
        let target_media_type = acceptable_media_ranges.first().unwrap_or_else(|| {
            todo!("Content negotiation");
        });
        if target_media_type != &self.media_type {
            Err(ContentRenderingError::MediaTypeError {
                source_media_type: self.media_type.clone(),
                target_media_type: target_media_type.clone(),
            })
        } else {
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
}

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
}
impl Render for RegisteredTemplate {
    fn render<E: ContentEngine>(
        &self,
        context: RenderContext<E>,
        acceptable_media_ranges: &[Mime],
    ) -> Result<String, ContentRenderingError> {
        let target_media_type = acceptable_media_ranges.first().unwrap_or_else(|| {
            todo!("Content negotiation");
        });
        if target_media_type != &self.rendered_media_type {
            Err(ContentRenderingError::MediaTypeError {
                source_media_type: self.rendered_media_type.clone(),
                target_media_type: target_media_type.clone(),
            })
        } else {
            let render_data = RenderData {
                source_media_type_of_parent: Some(SerializableMediaRange::from(
                    &self.rendered_media_type,
                )),
                ..context.data
            };
            context
                .content_engine
                .handlebars_registry()
                .render(&self.name_in_registry, &render_data)
                .map_err(ContentRenderingError::from)
        }
    }
}

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
}
impl Render for UnregisteredTemplate {
    fn render<E: ContentEngine>(
        &self,
        context: RenderContext<E>,
        acceptable_media_ranges: &[Mime],
    ) -> Result<String, ContentRenderingError> {
        let target_media_type = acceptable_media_ranges.first().unwrap_or_else(|| {
            todo!("Content negotiation");
        });
        if target_media_type != &self.rendered_media_type {
            Err(ContentRenderingError::MediaTypeError {
                source_media_type: self.rendered_media_type.clone(),
                target_media_type: target_media_type.clone(),
            })
        } else {
            let render_data = RenderData {
                source_media_type_of_parent: Some(SerializableMediaRange::from(
                    &self.rendered_media_type,
                )),
                ..context.data
            };
            let handlebars_context = handlebars::Context::wraps(&render_data)?;
            let mut handlebars_render_context = handlebars::RenderContext::new(None);
            self.template
                .renders(
                    context.content_engine.handlebars_registry(),
                    &handlebars_context,
                    &mut handlebars_render_context,
                )
                .map_err(ContentRenderingError::from)
        }
    }
}

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
}
impl Render for Executable {
    fn render<E: ContentEngine>(
        &self,
        _context: RenderContext<E>,
        acceptable_media_ranges: &[Mime],
    ) -> Result<String, ContentRenderingError> {
        let target_media_type = acceptable_media_ranges.first().unwrap_or_else(|| {
            todo!("Content negotiation");
        });
        if target_media_type != &self.output_media_type {
            Err(ContentRenderingError::MediaTypeError {
                source_media_type: self.output_media_type.clone(),
                target_media_type: target_media_type.clone(),
            })
        } else {
            let mut command = Command::new(self.program.clone());
            command.current_dir(self.working_directory.clone());

            let process::Output {
                status,
                stdout,
                stderr,
            } = command
                .output()
                .map_err(|io_error| ContentRenderingError::ExecutableError {
                    message: format!("Unable to execute program: {}", io_error),
                    program: self.program.clone(),
                    working_directory: self.working_directory.clone(),
                })?;

            if !status.success() {
                Err(match status.code() {
                    Some(exit_code) => ContentRenderingError::ExecutableExitedWithNonzero {
                        stderr_contents: match String::from_utf8_lossy(&stderr).as_ref() {
                            "" => None,
                            message => Some(message.to_string()),
                        },
                        program: self.program.clone(),
                        exit_code,
                        working_directory: self.working_directory.clone(),
                    },
                    None => ContentRenderingError::ExecutableError {
                        message: String::from(
                            "Program exited with failure, but its exit code was not available. It may have been killed by a signal."
                        ),
                        program: self.program.clone(),
                        working_directory: self.working_directory.clone(),
                    }
                })
            } else {
                let output = String::from_utf8(stdout).map_err(|utf8_error| {
                    ContentRenderingError::ExecutableError {
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
                MOCK_CONTENT_ENGINE.get_render_context(),
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
            MOCK_CONTENT_ENGINE.get_render_context(),
            &[target_media_type],
        );

        assert!(render_result.is_err());
    }

    #[test]
    fn executables_execute_when_rendered() {
        let path = format!("{}/src", PROJECT_DIRECTORY);
        let working_directory =
            fs::canonicalize(path).expect("Could not canonicalize path for test");
        let executable = Executable::new("pwd", working_directory.clone(), mime::TEXT_PLAIN);

        let output = executable
            .render(
                MOCK_CONTENT_ENGINE.get_render_context(),
                &[mime::TEXT_PLAIN],
            )
            .expect("Executable failed but it should have succeeded");
        assert_eq!(output, format!("{}\n", working_directory.display()));
    }

    #[test]
    fn executables_exiting_with_nonzero_are_err() {
        let executable = Executable::new("false", PROJECT_DIRECTORY, mime::TEXT_PLAIN);

        let result = executable.render(
            MOCK_CONTENT_ENGINE.get_render_context(),
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
            MOCK_CONTENT_ENGINE.get_render_context(),
            &[mime::TEXT_PLAIN],
        );
        assert!(
            result.is_err(),
            "Executable succeeded but it should have failed"
        );
    }
}
