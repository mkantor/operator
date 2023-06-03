use super::*;
use body::{FileBody, InMemoryBody, ProcessBody};
use handlebars::{self, Handlebars, Renderable as _};
use std::fs;
use std::io;
use std::mem;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use thiserror::Error;

/// Indicates that there was an error during rendering.
#[derive(Error, Debug)]
pub enum RenderingFailedError {
    #[error(
        "Rendering failed for handlebars template: {}",
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
        "Render data could not be serialized: {}",
        .source,
    )]
    RenderDataSerializationFailed {
        #[from]
        source: serde_json::error::Error,
    },

    #[error("Input/output error during rendering")]
    IOError {
        #[from]
        source: io::Error,
    },

    #[error("{} This should never happen: {}", bug_message!(), .0)]
    Bug(String),
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

    pub(super) fn render_to_native_media_type(
        &self,
    ) -> Result<Media<FileBody>, RenderingFailedError> {
        // We clone the file handle and operate on that to avoid taking
        // self as mut.
        let file = self.contents.try_clone()?;
        let stream = FileBody::try_from_file(file)?;
        Ok(Media::new(self.media_type.clone(), stream))
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

    pub(super) fn render_to_native_media_type<ServerInfo>(
        &self,
        handlebars_registry: &Handlebars,
        render_data: RenderData<ServerInfo>,
        handlebars_render_context: Option<handlebars::RenderContext>,
    ) -> Result<Media<InMemoryBody>, RenderingFailedError>
    where
        ServerInfo: Clone + Serialize,
    {
        let render_data = RenderData {
            target_media_type: Some(self.rendered_media_type.clone()),
            ..render_data
        };
        let rendered_content = match handlebars_render_context {
            None => handlebars_registry.render(&self.name_in_registry, &render_data)?,
            Some(mut handlebars_render_context) => handlebars_registry
                .get_template(&self.name_in_registry)
                .ok_or_else(|| {
                    RenderingFailedError::Bug(format!(
                        "Template '{}' was not found in the registry",
                        &self.name_in_registry
                    ))
                })?
                .renders(
                    handlebars_registry,
                    &handlebars::Context::wraps(&render_data)?,
                    &mut { handlebars_render_context },
                )?,
        };

        Ok(Media::new(
            self.rendered_media_type.clone(),
            InMemoryBody(rendered_content.bytes().collect()),
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
    ) -> Result<Self, TemplateError> {
        let template = handlebars::Template::compile(handlebars_source.as_ref())?;
        Ok(UnregisteredTemplate {
            template,
            rendered_media_type,
        })
    }

    pub(super) fn render_to_native_media_type<ServerInfo>(
        &self,
        handlebars_registry: &Handlebars,
        render_data: RenderData<ServerInfo>,
    ) -> Result<Media<InMemoryBody>, RenderingFailedError>
    where
        ServerInfo: Clone + Serialize,
    {
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
            InMemoryBody(rendered_content.bytes().collect()),
        ))
    }
}
impl Render for UnregisteredTemplate {
    type Output = InMemoryBody;
    fn render<'accept, ServerInfo, Engine, Accept>(
        &self,
        context: RenderContext<ServerInfo, Engine>,
        acceptable_media_ranges: Accept,
    ) -> Result<Media<Self::Output>, RenderError>
    where
        ServerInfo: Clone + Serialize,
        Engine: ContentEngine<ServerInfo>,
        Accept: IntoIterator<Item = &'accept MediaRange>,
        Self::Output: ByteStream,
    {
        for acceptable_media_range in acceptable_media_ranges {
            if self
                .rendered_media_type
                .is_within_media_range(acceptable_media_range)
            {
                return self
                    .render_to_native_media_type(
                        context.content_engine.handlebars_registry(),
                        context.data,
                    )
                    .map_err(RenderError::RenderingFailed);
            }
        }

        Err(RenderError::CannotProvideAcceptableMediaType)
    }
}

/// A program that can be run by the operating system, e.g. a shell script.
///
/// If the executed program terminates with a nonzero exit code, rendering
/// output is the contents of standard output. Otherwise a rendering failure
/// occurs.
///
/// Render data is available as JSON in the OPERATOR_RENDER_DATA environment
/// variable.
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

    pub(super) fn render_to_native_media_type<ServerInfo>(
        &self,
        render_data: RenderData<ServerInfo>,
        additional_data: Option<serde_json::Value>,
    ) -> Result<Media<ProcessBody>, RenderingFailedError>
    where
        ServerInfo: Clone + Serialize,
    {
        let base_render_data = RenderData {
            target_media_type: Some(self.output_media_type.clone()),
            ..render_data
        };

        let render_data_environment_variable_value = match additional_data {
            None => serde_json::ser::to_string(&base_render_data)?,
            Some(serde_json::Value::Object(mut additional_data_as_json_map)) => {
                // merge additional data atop base render data
                let base_render_data_as_json = serde_json::value::to_value(base_render_data)?;
                if let serde_json::Value::Object(mut base_render_data_as_json_map) =
                    base_render_data_as_json
                {
                    for (key, value) in additional_data_as_json_map.iter_mut() {
                        base_render_data_as_json_map.insert(key.to_string(), mem::take(value));
                    }
                    serde_json::Value::Object(base_render_data_as_json_map).to_string()
                } else {
                    return Err(RenderingFailedError::Bug(format!(
                        "Render data did not serialize to a JSON object, instead got `{}`.",
                        base_render_data_as_json
                    )));
                }
            }
            Some(non_object_additional_data) => non_object_additional_data.to_string(),
        };

        let mut command = Command::new(self.program.clone());
        let child = command
            .current_dir(self.working_directory.clone())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env(
                "OPERATOR_RENDER_DATA",
                render_data_environment_variable_value,
            )
            .spawn()
            .map_err(|io_error| RenderingFailedError::ExecutableError {
                message: format!("Unable to execute program: {}", io_error),
                program: self.program.clone(),
                working_directory: self.working_directory.clone(),
            })?;

        Ok(Media::new(
            self.output_media_type.clone(),
            ProcessBody::new(child),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_lib::*;
    use super::*;
    use crate::content::content_index::ContentIndexEntries;
    use crate::test_lib::*;
    use crate::ServerInfo;
    use ::mime;
    use maplit::hashmap;
    use std::fs;
    use std::io::Write;
    use std::str;
    use tempfile::tempfile;
    use test_log::test;

    fn test_render_data() -> RenderData<ServerInfo> {
        RenderData {
            server_info: ServerInfo::without_socket_address().expect("Unable to create ServerInfo"),
            index: ContentIndex::Directory(ContentIndexEntries::new()),
            target_media_type: None,
            error_code: None,
            request: RequestData {
                route: None,
                query_parameters: hashmap![],
                request_headers: hashmap![],
            },
        }
    }

    #[test]
    fn static_content_can_be_rendered() {
        let mut file = tempfile().expect("Failed to create temporary file");
        write!(file, "hello world").expect("Failed to write to temporary file");
        let static_content = StaticContentItem {
            media_type: MediaType::from_media_range(mime::TEXT_PLAIN).unwrap(),
            contents: file,
        };
        let output = static_content
            .render_to_native_media_type()
            .expect("Render failed");

        assert_eq!(media_to_string(output), String::from("hello world"));
    }

    #[test]
    fn static_content_can_be_arbitrary_bytes() {
        let non_utf8_bytes = &[0xfe, 0xfe, 0xff, 0xff];
        assert!(str::from_utf8(non_utf8_bytes).is_err());

        let mut file = tempfile().expect("Failed to create temporary file");
        file.write_all(non_utf8_bytes)
            .expect("Failed to write to temporary file");
        let static_content = StaticContentItem {
            media_type: MediaType::from_media_range(mime::APPLICATION_OCTET_STREAM).unwrap(),
            contents: file,
        };
        let output = static_content
            .render_to_native_media_type()
            .expect("Render failed");

        assert_eq!(
            block_on_content(output).expect("There was an error in the content stream"),
            Bytes::copy_from_slice(non_utf8_bytes)
        );
    }

    #[test]
    fn unregistered_template_can_be_rendered() {
        let content_engine = MockContentEngine::new();

        let template = UnregisteredTemplate::from_source(
            "{{#if true}}it works!{{/if}}",
            MediaType::from_media_range(mime::TEXT_PLAIN).unwrap(),
        )
        .expect("Test template was invalid");
        let rendered = template.render_to_native_media_type(
            content_engine.handlebars_registry(),
            content_engine
                .render_context(None, hashmap![], hashmap![])
                .data,
        );

        let template_output = media_to_string(rendered.expect("Rendering failed"));
        assert_eq!(template_output, "it works!");
    }

    #[test]
    fn registered_template_can_be_rendered() {
        let mut content_engine = MockContentEngine::new();
        content_engine
            .register_template("test", "{{#if true}}it works!{{/if}}")
            .expect("Could not register test template");

        let template = RegisteredTemplate::new(
            "test",
            MediaType::from_media_range(mime::TEXT_PLAIN).unwrap(),
        );
        let rendered = template.render_to_native_media_type(
            content_engine.handlebars_registry(),
            content_engine
                .render_context(Some(route("/test")), hashmap![], hashmap![])
                .data,
            None,
        );

        let template_output = media_to_string(rendered.expect("Rendering failed"));
        assert_eq!(template_output, "it works!");
    }

    #[test]
    fn registered_template_can_be_rendered_with_custom_handlebars_context() {
        let mut content_engine = MockContentEngine::new();
        content_engine
            .register_template("test", "{{ ping }}")
            .expect("Could not register test template");

        let template = RegisteredTemplate::new(
            "test",
            MediaType::from_media_range(mime::TEXT_PLAIN).unwrap(),
        );

        let replaced_render_data = handlebars::Context::wraps(hashmap!["ping" => "pong"])
            .expect("Could not create fake render data");
        let mut handlebars_render_context = handlebars::RenderContext::new(None);
        handlebars_render_context.set_context(replaced_render_data);

        let rendered = template.render_to_native_media_type(
            content_engine.handlebars_registry(),
            content_engine
                .render_context(Some(route("/test")), hashmap![], hashmap![])
                .data,
            Some(handlebars_render_context),
        );

        let template_output = media_to_string(rendered.expect("Rendering failed"));
        assert_eq!(template_output, "pong");
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
        let output = executable
            .render_to_native_media_type(test_render_data(), None)
            .expect("Executable failed but it should have succeeded");

        assert_eq!(
            media_to_string(output),
            format!("{}\n", working_directory.display())
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
        let result = executable.render_to_native_media_type(test_render_data(), None);
        assert!(
            result.is_err(),
            "Executable succeeded but it should have failed"
        );
    }

    #[test]
    fn executables_emit_stream_error_if_exit_code_is_not_zero() {
        let path = format!("{}/src", PROJECT_DIRECTORY);
        let working_directory =
            fs::canonicalize(path).expect("Could not canonicalize path for test");

        // Exits with 1 and prints nothing to stdout.
        {
            let executable = Executable::new(
                "false",
                working_directory.clone(),
                MediaType::from_media_range(mime::TEXT_PLAIN).unwrap(),
            );
            let output = executable
                .render_to_native_media_type(test_render_data(), None)
                .expect("Executable failed but it should have succeeded");

            match block_on_content(output) {
                Err(StreamError::ExecutableExitedWithNonzero {
                    exit_code,
                    stderr_contents,
                    ..
                }) => {
                    assert_eq!(exit_code, Some(1));
                    assert_eq!(stderr_contents, None);
                }
                Err(_) => panic!("Got a different error than expected"),
                Ok(_) => panic!("Expected an error"),
            }
        }

        // Exits with nonzero and prints a message to stdout.
        {
            let executable = Executable::new(
                "mv",
                working_directory.clone(),
                MediaType::from_media_range(mime::TEXT_PLAIN).unwrap(),
            );
            let output = executable
                .render_to_native_media_type(test_render_data(), None)
                .expect("Executable failed but it should have succeeded");

            match block_on_content(output) {
                Err(StreamError::ExecutableExitedWithNonzero {
                    exit_code,
                    stderr_contents,
                    ..
                }) => {
                    assert!(exit_code.is_some() && exit_code != Some(0));
                    assert!(stderr_contents.is_some());
                    assert!(stderr_contents.unwrap().len() > 0);
                }
                Err(_) => panic!("Got a different error than expected"),
                Ok(_) => panic!("Expected an error"),
            }
        }
    }
}
