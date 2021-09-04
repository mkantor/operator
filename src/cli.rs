use crate::content::*;
use crate::*;
use futures::executor;
use futures::stream::TryStreamExt;
use std::collections::HashMap;
use std::io;
use std::net::ToSocketAddrs;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RenderCommandError {
    #[error("Failed to read input.")]
    ReadError { source: io::Error },

    #[error("Unable to collect server info.")]
    ServerInfoError {
        #[from]
        source: ServerInfoError,
    },

    #[error("Unable to load content.")]
    ContentLoadingError {
        #[from]
        source: ContentLoadingError,
    },

    #[error("Unable to parse template from input.")]
    TemplateParseError {
        #[from]
        source: TemplateError,
    },

    #[error("Unable to render content.")]
    RenderError {
        #[from]
        source: RenderError,
    },

    #[error("Unable to emit rendered content.")]
    StreamError {
        #[from]
        source: StreamError,
    },

    #[error("Failed to write output.")]
    WriteError { source: io::Error },
}

#[derive(Error, Debug)]
pub enum GetCommandError {
    #[error("Unable to collect server info.")]
    ServerInfoError {
        #[from]
        source: ServerInfoError,
    },

    #[error("Unable to load content.")]
    ContentLoadingError {
        #[from]
        source: ContentLoadingError,
    },

    #[error("Content not found at route '{}'.", .route)]
    ContentNotFound { route: Route },

    #[error("Unable to render content.")]
    RenderError {
        #[from]
        source: RenderError,
    },

    #[error("Unable to emit rendered content.")]
    StreamError {
        #[from]
        source: StreamError,
    },

    #[error("Failed to write output.")]
    WriteError { source: io::Error },
}

#[derive(Error, Debug)]
pub enum ServeCommandError {
    #[error("Unable to collect server info.")]
    ServerInfoError {
        #[from]
        source: ServerInfoError,
    },

    #[error("Unable to load content.")]
    ContentLoadingError {
        #[from]
        source: ContentLoadingError,
    },

    #[error("Index route does not exist.")]
    IndexRouteMissing,

    #[error("Error handler route does not exist.")]
    ErrorHandlerRouteMissing,

    #[error("Failed to run server.")]
    ServerError { source: io::Error },
}

/// Reads a template from `input`, renders it, and writes it to `output`.
pub fn eval<I: io::Read, O: io::Write>(
    content_directory: ContentDirectory,
    input: &mut I,
    output: &mut O,
) -> Result<(), RenderCommandError> {
    let shared_content_engine = FilesystemBasedContentEngine::from_content_directory(
        content_directory,
        ServerInfo::without_socket_address()?,
    )?;
    let content_engine = shared_content_engine
        .read()
        .expect("RwLock for ContentEngine has been poisoned");

    let mut template = String::new();
    input
        .read_to_string(&mut template)
        .map_err(|source| RenderCommandError::ReadError { source })?;

    let content_item =
        content_engine.new_template(&template, MediaType::APPLICATION_OCTET_STREAM)?;
    let render_context = content_engine.render_context(None, HashMap::new());
    let media = content_item.render(render_context, &[mime::STAR_STAR])?;

    executor::block_on(media.content.try_for_each(|bytes| {
        let result = output.write_all(&bytes).map_err(StreamError::from);
        async { result }
    }))?;

    output
        .flush()
        .map_err(|source| RenderCommandError::WriteError { source })
}

/// Renders an item from the content directory and writes it to `output`.
pub fn get<O: io::Write>(
    content_directory: ContentDirectory,
    route: &Route,
    accept: Option<MediaRange>,
    output: &mut O,
) -> Result<(), GetCommandError> {
    let shared_content_engine = FilesystemBasedContentEngine::from_content_directory(
        content_directory,
        ServerInfo::without_socket_address()?,
    )?;
    let content_engine = shared_content_engine
        .read()
        .expect("RwLock for ContentEngine has been poisoned");

    let content_item =
        content_engine
            .get(route)
            .ok_or_else(|| GetCommandError::ContentNotFound {
                route: route.clone(),
            })?;
    let render_context = content_engine.render_context(Some(route.clone()), HashMap::new());
    let media = content_item.render(render_context, &[accept.unwrap_or(mime::STAR_STAR)])?;

    executor::block_on(media.content.try_for_each(|bytes| {
        let result = output.write_all(&bytes).map_err(StreamError::from);
        async { result }
    }))?;

    output
        .flush()
        .map_err(|source| GetCommandError::WriteError { source })
}

/// Starts an HTTP server for the given content directory.
pub fn serve<A: 'static + ToSocketAddrs>(
    content_directory: ContentDirectory,
    index_route: Option<Route>,
    error_handler_route: Option<Route>,
    bind_to: A,
) -> Result<(), ServeCommandError> {
    let shared_content_engine = FilesystemBasedContentEngine::from_content_directory(
        content_directory,
        ServerInfo::with_socket_address(&bind_to)?,
    )?;

    // If index or error handler are set, validate that they refer to an
    // existing route.
    if index_route.is_some() || error_handler_route.is_some() {
        let content_engine = shared_content_engine
            .read()
            .expect("RwLock for ContentEngine has been poisoned");

        if let Some(specified_index_route) = &index_route {
            let index = content_engine.get(specified_index_route);
            if index.is_none() {
                return Err(ServeCommandError::IndexRouteMissing);
            }
        }

        if let Some(specified_error_handler_route) = &error_handler_route {
            let error_handler = content_engine.get(specified_error_handler_route);
            if error_handler.is_none() {
                return Err(ServeCommandError::ErrorHandlerRouteMissing);
            }
        }
    }

    http::run_server(
        shared_content_engine,
        index_route,
        error_handler_route,
        bind_to,
    )
    .map_err(|source| ServeCommandError::ServerError { source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lib::*;
    use std::str;
    use test_env_log::test;

    #[test]
    fn valid_templates_can_be_evaluated() {
        for &(template, expected_output) in &VALID_TEMPLATES {
            let mut input = template.as_bytes();
            let mut output = Vec::new();
            let directory = arbitrary_content_directory_with_valid_content();
            let result = eval(directory, &mut input, &mut output);

            assert!(
                result.is_ok(),
                "Template rendering failed for `{}`: {}",
                template,
                result.unwrap_err(),
            );
            let output_as_str = str::from_utf8(output.as_slice()).expect("Output was not UTF-8");
            assert_eq!(
                output_as_str,
                expected_output,
                "Template rendering for `{}` did not produce the expected output (\"{}\"), instead got \"{}\"",
                template,
                expected_output,
                output_as_str
            );
        }
    }

    #[test]
    fn invalid_templates_fail_evaluation() {
        for &template in &INVALID_TEMPLATES {
            let mut input = template.as_bytes();
            let mut output = Vec::new();
            let directory = arbitrary_content_directory_with_valid_content();
            let result = eval(directory, &mut input, &mut output);

            assert!(
                result.is_err(),
                "Template rendering succeeded for `{}`, but it should have failed",
                template,
            );
        }
    }

    #[test]
    fn content_can_be_retrieved_from_content_directory() {
        let mut output = Vec::new();
        let route = route("/hello");
        let expected_output = "hello world";

        let directory = arbitrary_content_directory_with_valid_content();
        let result = get(directory, &route, Some(mime::TEXT_PLAIN), &mut output);

        assert!(
            result.is_ok(),
            "Template rendering failed for content at '{}': {}",
            route,
            result.unwrap_err(),
        );
        let output_as_str = str::from_utf8(output.as_slice()).expect("Output was not UTF-8");
        assert_eq!(
            output_as_str,
            expected_output,
            "Template rendering for content at '{}' did not produce the expected output (\"{}\"), instead got \"{}\"",
            route,
            expected_output,
            output_as_str
        );
    }

    #[test]
    fn accept_is_optional_when_retrieving_content() {
        let mut output = Vec::new();
        let route = route("/hello");
        let expected_output = "hello world";

        let directory = arbitrary_content_directory_with_valid_content();
        let result = get(directory, &route, None, &mut output);

        assert!(
            result.is_ok(),
            "Template rendering failed for content at '{}': {}",
            route,
            result.unwrap_err(),
        );
        let output_as_str = str::from_utf8(output.as_slice()).expect("Output was not UTF-8");
        assert_eq!(
            output_as_str,
            expected_output,
            "Template rendering for content at '{}' did not produce the expected output (\"{}\"), instead got \"{}\"",
            route,
            expected_output,
            output_as_str
        );
    }

    #[test]
    fn getting_content_which_does_not_exist_is_an_error() {
        let mut output = Vec::new();
        let route = route("/this-route-does-not-refer-to-any-content");

        let directory = arbitrary_content_directory_with_valid_content();
        let result = get(directory, &route, Some(mime::TEXT_HTML), &mut output);

        match result {
            Ok(_) => panic!(
                "Getting content from '{}' succeeded, but it should have failed",
                route
            ),
            Err(GetCommandError::ContentNotFound {
                route: route_from_error,
            }) => assert_eq!(
                route_from_error, route,
                "Route from error did not match route used"
            ),
            Err(_) => panic!("Wrong type of error was produced, expected ContentNotFound"),
        };
    }
}
