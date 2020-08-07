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

    #[error("The requested content cannot be rendered as an acceptable media type.")]
    CannotProvideAcceptableMediaType,

    #[error("You've encountered a bug! This should never happen: {}", .message)]
    Bug { message: String },
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

    pub(super) fn render_to_native_media_type(&self) -> Result<Media<File>, RenderingFailure> {
        // We clone the file handle and operate on that to avoid taking
        // self as mut. Note that all clones share a cursor, so seeking
        // back to the beginning is necessary to ensure we read the
        // entire file.
        let mut file = self.contents.try_clone()?;
        file.seek(SeekFrom::Start(0))?;
        Ok(Media::new(self.media_type.clone(), file))
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

    pub(super) fn render_to_native_media_type<ServerInfo: Clone + Serialize>(
        &self,
        handlebars_registry: &Handlebars,
        render_data: RenderData<ServerInfo>,
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

    pub(super) fn render_to_native_media_type<ServerInfo: Clone + Serialize>(
        &self,
        handlebars_registry: &Handlebars,
        render_data: RenderData<ServerInfo>,
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
    fn render<'engine, 'accept, ServerInfo, Engine, Accept>(
        &self,
        context: RenderContext<ServerInfo, Engine>,
        acceptable_media_ranges: Accept,
    ) -> Result<Media<Self::Output>, ContentRenderingError>
    where
        ServerInfo: Clone + Serialize,
        Engine: ContentEngine<ServerInfo>,
        Accept: IntoIterator<Item = &'accept MediaRange>,
        Self::Output: Read,
    {
        for acceptable_media_range in acceptable_media_ranges {
            if self
                .rendered_media_type
                .is_within_media_range(acceptable_media_range)
            {
                return self
                    .render_to_native_media_type(
                        context.content_engine.handlebars_registry(),
                        context.data.clone(),
                    )
                    .map_err(ContentRenderingError::RenderingFailure);
            }
        }

        Err(ContentRenderingError::CannotProvideAcceptableMediaType)
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

    pub(super) fn render_to_native_media_type(
        &self,
    ) -> Result<Media<ChildStdout>, RenderingFailure> {
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
                        "Program exited with failure, but its exit code was not available. \
                        It may have been killed by a signal.",
                    ),
                    program: self.program.clone(),
                    working_directory: self.working_directory.clone(),
                },
            })
        } else {
            Ok(Media::new(self.output_media_type.clone(), stdout))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_lib::*;
    use super::*;
    use crate::test_lib::*;
    use ::mime;
    use std::fs;
    use std::io::Write;
    use std::str;
    use tempfile::tempfile;

    #[test]
    fn static_content_is_stringified_when_rendered() {
        let mut file = tempfile().expect("Failed to create temporary file");
        write!(file, "hello world").expect("Failed to write to temporary file");
        let static_content = StaticContentItem {
            media_type: MediaType::from_media_range(mime::TEXT_PLAIN).unwrap(),
            contents: file,
        };
        let mut output = static_content
            .render_to_native_media_type()
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
            .render_to_native_media_type()
            .expect("Render failed");

        assert_eq!(media_to_bytes(&mut output), non_utf8_bytes);
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
            .render_to_native_media_type()
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

        let result = executable.render_to_native_media_type();
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

        let result = executable.render_to_native_media_type();
        assert!(
            result.is_err(),
            "Executable succeeded but it should have failed"
        );
    }
}
