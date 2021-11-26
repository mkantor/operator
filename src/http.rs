use crate::content::*;
use crate::*;
use actix_rt::System;
use actix_web::error::QueryPayloadError;
use actix_web::http::header::{self, Header};
use actix_web::{http, web, App, HttpRequest, HttpResponse, HttpServer};
use futures::TryStreamExt;
use mime_guess::MimeGuess;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::io;
use std::net::ToSocketAddrs;
use std::str::FromStr;
use std::sync::{Arc, RwLock};

#[derive(Error, Debug)]
#[error("Invalid query string '{}'", .query_string)]
pub struct InvalidQueryStringError {
    query_string: String,
    source: QueryPayloadError,
}

pub struct QueryString(HashMap<String, String>);

impl Default for QueryString {
    fn default() -> Self {
        QueryString(HashMap::new())
    }
}

impl From<QueryString> for HashMap<String, String> {
    fn from(query_string: QueryString) -> HashMap<String, String> {
        query_string.0
    }
}

impl FromStr for QueryString {
    type Err = InvalidQueryStringError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        web::Query::<HashMap<String, String>>::from_query(input)
            .map(|query_parameters| QueryString(query_parameters.to_owned()))
            .map_err(|source| InvalidQueryStringError {
                query_string: String::from(input),
                source,
            })
    }
}

struct AppData<Engine: 'static + ContentEngine<ServerInfo> + Send + Sync> {
    shared_content_engine: Arc<RwLock<Engine>>,
    index_route: Option<Route>,
    error_handler_route: Option<Route>,
}

pub fn run_server<SocketAddress, Engine>(
    shared_content_engine: Arc<RwLock<Engine>>,
    index_route: Option<Route>,
    error_handler_route: Option<Route>,
    socket_address: SocketAddress,
) -> Result<(), io::Error>
where
    SocketAddress: 'static + ToSocketAddrs,
    Engine: 'static + ContentEngine<ServerInfo> + Send + Sync,
{
    log::info!("Initializing HTTP server");
    let mut system = System::new("server");
    let result = system.block_on(async move {
        HttpServer::new(move || {
            App::new()
                .app_data(AppData {
                    shared_content_engine: shared_content_engine.clone(),
                    index_route: index_route.clone(),
                    error_handler_route: error_handler_route.clone(),
                })
                .default_service(web::get().to(get::<Engine>))
        })
        .keep_alive(None)
        .bind(socket_address)?
        .run()
        .await
    });

    log::info!("HTTP server has terminated");
    result
}

/// Use the URL path, app data, and accept header to render some content for
/// the HTTP response.
///
/// Content negotiation is performed based on media types (just the accept
/// header; not accept-language, etc) and content is only rendered as media
/// types the client asked for.
///
/// If the path has an extension which maps to a media range it will be
/// considered for content negotiation instead of the accept header. This
/// feature exists because there is not a great way to link to particular
/// representations of resources on the web without putting something in the
/// URL, and it's awfully convenient for humans (compare "to get my resume in
/// PDF format, visit http://mysite.com/resume.pdf" to "...first install this
/// browser extension that lets you customize HTTP headers, then set the accept
/// header to application/pdf, then visit http://mysite.com/resume").
async fn get<Engine>(request: HttpRequest) -> HttpResponse
where
    Engine: 'static + ContentEngine<ServerInfo> + Send + Sync,
{
    let app_data = request
        .app_data::<AppData<Engine>>()
        .expect("App data was not of the expected type!");

    let path = request.uri().path();

    log::info!(
        // e.g. "Handling request GET /styles.css HTTP/1.1 with Accept: text/css,*/*;q=0.1"
        "Handling request {} {} {}{}",
        request.method(),
        request.uri(),
        match request.version() {
            http::Version::HTTP_09 => "HTTP/0.9",
            http::Version::HTTP_10 => "HTTP/1.0",
            http::Version::HTTP_11 => "HTTP/1.1",
            http::Version::HTTP_2 => "HTTP/2.0",
            http::Version::HTTP_3 => "HTTP/3.0",
            _ => "HTTP",
        },
        request
            .headers()
            .get(header::ACCEPT)
            .and_then(|value| value.to_str().ok())
            .map(|value| format!(" with Accept: {}", value))
            .unwrap_or_default()
    );

    let (route, media_range_from_url) = {
        let media_range_from_url = MimeGuess::from_path(path).first();
        let path_without_extension = if media_range_from_url.is_some() {
            // Drop the extension from the path.
            path.rsplitn(2, '.').last().expect(bug_message!(
                "Calling rsplitn(2, ..) on a non-empty string returned an empty iterator. This should be impossible!",
            ))
        } else {
            path
        };

        match path_without_extension.parse::<Route>() {
            Err(error) => panic!(
                bug_message!("This should never happen: HTTP request path could not be parsed into a Route: {}"),
                error,
            ),
            Ok(request_route) => {
                if request_route.as_ref() == "/" {
                    // Default to the index route if one was specified.
                    let adjusted_route = match &app_data.index_route {
                        Some(default_route) => default_route.clone(),
                        None => request_route,
                    };
                    let media_range_from_url = None;
                    (adjusted_route, media_range_from_url)
                } else {
                    (request_route, media_range_from_url)
                }
            }
        }
    };

    let content_engine = app_data
        .shared_content_engine
        .read()
        .expect("RwLock for ContentEngine has been poisoned");

    let query_string = request.query_string();
    let query_parameters = match query_string.parse::<QueryString>() {
        Ok(query_parameters) => query_parameters.into(),
        Err(error) => {
            log::warn!(
                "Responding with {} for {}. Malformed query string `{}`: {}",
                http::StatusCode::BAD_REQUEST,
                route,
                query_string,
                error
            );
            return error_response(
                http::StatusCode::BAD_REQUEST,
                &*content_engine,
                route,
                HashMap::new(),
                &app_data.error_handler_route,
                vec![&mime::TEXT_PLAIN],
            );
        }
    };

    // Use the media type from the URL path extension if there was one,
    // otherwise use the accept header.
    let mut parsed_accept_header_value = header::Accept::parse(&request);
    let acceptable_media_ranges = match media_range_from_url {
        Some(ref media_range_from_url) => vec![media_range_from_url],
        None => match parsed_accept_header_value {
            Ok(ref mut accept_value) => acceptable_media_ranges_from_accept_header(accept_value),
            Err(error) => {
                log::warn!(
                    "Responding with {} for {}. Malformed Accept header value `{:?}`: {}",
                    http::StatusCode::BAD_REQUEST,
                    route,
                    request.headers().get(header::ACCEPT),
                    error
                );
                return error_response(
                    http::StatusCode::BAD_REQUEST,
                    &*content_engine,
                    route,
                    query_parameters,
                    &app_data.error_handler_route,
                    vec![&mime::TEXT_PLAIN],
                );
            }
        },
    };

    let render_result = content_engine.get(&route).map(|content| {
        let render_context =
            content_engine.render_context(Some(route.clone()), query_parameters.clone());
        content.render(render_context, acceptable_media_ranges.clone())
    });

    match render_result {
        Some(Ok(Media {
            content,
            media_type,
        })) => {
            log::info!(
                "Responding with {}, body from {} as {}",
                http::StatusCode::OK,
                route,
                media_type,
            );
            let loggable_media_type = media_type.clone();
            let loggable_route = route.clone();
            HttpResponse::Ok()
                .content_type(media_type.to_string())
                .streaming(
                    content
                        .map_err(|error| {
                            log::error!(
                                "An error occurred while streaming a response body: {}",
                                error,
                            );
                        })
                        .inspect_ok(move |bytes| {
                            let max_length = 64;
                            if bytes.len() > max_length {
                                log::trace!(
                                    "Streaming data for {} as {}: {:?} ...and {} more bytes",
                                    loggable_route,
                                    loggable_media_type,
                                    bytes.slice(0..max_length),
                                    bytes.len() - max_length
                                );
                            } else {
                                log::trace!(
                                    "Streaming data for {} as {}: {:?}",
                                    loggable_route,
                                    loggable_media_type,
                                    bytes
                                );
                            }
                        }),
                )
        }
        Some(Err(error @ RenderError::CannotProvideAcceptableMediaType { .. })) => {
            log::warn!(
                "Responding with {} for {}. Cannot provide an acceptable response: {}",
                http::StatusCode::NOT_ACCEPTABLE,
                route,
                error,
            );
            error_response(
                http::StatusCode::NOT_ACCEPTABLE,
                &*content_engine,
                route,
                query_parameters,
                &app_data.error_handler_route,
                acceptable_media_ranges,
            )
        }
        Some(Err(error)) => {
            log::warn!(
                "Responding with {} for {}. Failed to render content: {}",
                http::StatusCode::INTERNAL_SERVER_ERROR,
                route,
                error,
            );
            error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                &*content_engine,
                route,
                query_parameters,
                &app_data.error_handler_route,
                acceptable_media_ranges,
            )
        }
        None => {
            log::warn!(
                "Responding with {} for {}. No content found.",
                http::StatusCode::NOT_FOUND,
                route,
            );
            error_response(
                http::StatusCode::NOT_FOUND,
                &*content_engine,
                route,
                query_parameters,
                &app_data.error_handler_route,
                acceptable_media_ranges,
            )
        }
    }
}

fn error_response<Engine>(
    status_code: http::StatusCode,
    content_engine: &Engine,
    request_route: Route,
    query_parameters: HashMap<String, String>,
    error_handler_route: &Option<Route>,
    acceptable_media_ranges: Vec<&MediaRange>,
) -> HttpResponse
where
    Engine: 'static + ContentEngine<ServerInfo> + Send + Sync,
{
    let error_code = if !status_code.is_client_error() && !status_code.is_server_error() {
        log::error!(
            bug_message!(
                "This should never happen: The HTTP status code given to the error handler ({}) does not indicate an error.",
            ),
            status_code,
        );
        http::StatusCode::INTERNAL_SERVER_ERROR
    } else {
        status_code
    };

    let mut response_builder = HttpResponse::build(error_code);

    error_handler_route
        .as_ref()
        .and_then(|route| {
            content_engine.get(route).and_then(|content| {
                let error_context = content_engine
                    .render_context(Some(request_route), query_parameters)
                    .into_error_context(status_code.as_u16());
                match content.render(error_context, acceptable_media_ranges) {
                    Ok(rendered_content) => Some(rendered_content),
                    Err(rendering_error) => {
                        log::error!(
                            "Error occurred while rendering error handler: {}",
                            rendering_error
                        );
                        None
                    }
                }
            })
        })
        .map(
            |Media {
                 media_type,
                 content,
             }| {
                response_builder
                    .content_type(media_type.to_string())
                    .streaming(content.map_err(|error| {
                        log::error!(
                            "An error occurred while streaming a response body: {}",
                            error,
                        );
                    }))
            },
        )
        .unwrap_or_else(|| {
            // Default error response if the error handler itself failed.
            response_builder.content_type("text/plain").body(
                error_code
                    .canonical_reason()
                    .unwrap_or("Something Went Wrong"),
            )
        })
}

fn acceptable_media_ranges_from_accept_header<'a>(
    accept_value: &'a mut header::Accept,
) -> Vec<&'a MediaRange> {
    // If the accept header value is empty, allow any media type.
    if accept_value.is_empty() {
        vec![&mime::STAR_STAR]
    } else {
        // Sort in order of descending quality (so the client's most-preferred
        // representation is first).
        //
        // Note that QualityItem only implements PartialOrd, not Ord. I thought
        // that might be because the parser lossily converts decimal strings
        // into integers (for the `q` parameter), but it turns out the
        // implementation actually never returns None (as of actix-web v3.0.0).
        // If that ever changes and there is some scenario where a pair of
        // items from the accept header can't be ordered then they will be
        // given equal preference. ¯\_(ツ)_/¯
        accept_value.sort_by(|a, b| {
            b.partial_cmp(a).unwrap_or_else(|| {
                log::warn!(
                    "Accept header items `{}` and `{}` could not be ordered by quality",
                    a,
                    b
                );
                Ordering::Equal
            })
        });

        accept_value
            .iter()
            .map(|quality_item| &quality_item.item)
            .collect::<Vec<&'a MediaRange>>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lib::*;
    use actix_web::body::{Body, ResponseBody};
    use actix_web::http::StatusCode;
    use actix_web::test::TestRequest;
    use bytes::{Bytes, BytesMut};
    use std::path::Path;
    use test_log::test;

    type TestContentEngine<'a> = FilesystemBasedContentEngine<'a, ServerInfo>;

    fn test_request(
        content_directory_path: &Path,
        index_route: Option<&str>,
        error_handler_route: Option<&str>,
    ) -> TestRequest {
        let directory = ContentDirectory::from_root(&content_directory_path).unwrap();
        let shared_content_engine = FilesystemBasedContentEngine::from_content_directory(
            directory,
            ServerInfo {
                version: ServerVersion(""),
                operator_path: PathBuf::new(),
                socket_address: None,
            },
        )
        .expect("Content engine could not be created");

        TestRequest::default().app_data(AppData {
            shared_content_engine: shared_content_engine,
            index_route: index_route.map(route),
            error_handler_route: error_handler_route.map(route),
        })
    }

    async fn collect_response_body(body: ResponseBody<Body>) -> Result<Bytes, actix_web::Error> {
        body.try_fold(BytesMut::new(), |mut accumulator, bytes| {
            accumulator.extend_from_slice(&bytes);
            async { Ok(accumulator) }
        })
        .await
        .map(BytesMut::freeze)
    }

    #[actix_rt::test]
    async fn content_may_be_not_found() {
        let request = test_request(&sample_path("empty"), None, None)
            .uri("/nothing/exists/at/this/path")
            .to_http_request();
        let response = get::<TestContentEngine>(request).await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[actix_rt::test]
    async fn content_can_be_retrieved_with_exact_media_type() {
        let request = test_request(&sample_path("hello-world"), None, None)
            .uri("/hello")
            .header(header::ACCEPT, "text/plain")
            .to_http_request();

        let mut response = get::<TestContentEngine>(request).await;
        let response_body = collect_response_body(response.take_body())
            .await
            .expect("There was an error in the content stream");
        let response_content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("Response was missing Content-Type header");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response status was not 200"
        );
        assert_eq!(
            response_content_type, "text/plain",
            "Response Content-Type was not text/plain",
        );
        assert_eq!(response_body, "hello world", "Response body was incorrect");
    }

    #[actix_rt::test]
    async fn content_can_be_retrieved_with_media_range() {
        let request = test_request(&sample_path("hello-world"), None, None)
            .uri("/hello")
            .header(header::ACCEPT, "text/*")
            .to_http_request();

        let mut response = get::<TestContentEngine>(request).await;
        let response_body = collect_response_body(response.take_body())
            .await
            .expect("There was an error in the content stream");
        let response_content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("Response was missing Content-Type header");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response status was not 200"
        );
        assert_eq!(
            response_content_type, "text/plain",
            "Response Content-Type was not text/plain",
        );
        assert_eq!(response_body, "hello world", "Response body was incorrect");
    }

    #[actix_rt::test]
    async fn content_can_be_retrieved_with_star_star_media_range() {
        let request = test_request(&sample_path("hello-world"), None, None)
            .uri("/hello")
            .header(header::ACCEPT, "*/*")
            .to_http_request();

        let mut response = get::<TestContentEngine>(request).await;
        let response_body = collect_response_body(response.take_body())
            .await
            .expect("There was an error in the content stream");
        let response_content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("Response was missing Content-Type header");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response status was not 200"
        );
        assert_eq!(
            response_content_type, "text/plain",
            "Response Content-Type was not text/plain",
        );
        assert_eq!(response_body, "hello world", "Response body was incorrect");
    }

    #[actix_rt::test]
    async fn content_can_be_retrieved_with_elaborate_accept_header() {
        let request = test_request(&sample_path("hello-world"), None, None)
            .uri("/hello")
            .header(header::ACCEPT, "audio/aac, text/*;q=0.9, image/gif;q=0.1")
            .to_http_request();

        let mut response = get::<TestContentEngine>(request).await;
        let response_body = collect_response_body(response.take_body())
            .await
            .expect("There was an error in the content stream");
        let response_content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("Response was missing Content-Type header");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response status was not 200"
        );
        assert_eq!(
            response_content_type, "text/plain",
            "Response Content-Type was not text/plain",
        );
        assert_eq!(response_body, "hello world", "Response body was incorrect");
    }

    #[actix_rt::test]
    async fn content_can_be_retrieved_with_missing_accept_header() {
        let request = test_request(&sample_path("hello-world"), None, None)
            .uri("/hello")
            .to_http_request();

        let mut response = get::<TestContentEngine>(request).await;
        let response_body = collect_response_body(response.take_body())
            .await
            .expect("There was an error in the content stream");
        let response_content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("Response was missing Content-Type header");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response status was not 200"
        );
        assert_eq!(
            response_content_type, "text/plain",
            "Response Content-Type was not text/plain",
        );
        assert_eq!(response_body, "hello world", "Response body was incorrect");
    }

    #[actix_rt::test]
    async fn multimedia_content_can_be_retrieved() {
        let request = test_request(&sample_path("multimedia"), None, None)
            .uri("/dramatic-prairie-dog")
            .header(header::ACCEPT, "video/*")
            .to_http_request();

        let mut response = get::<TestContentEngine>(request).await;
        let response_body = collect_response_body(response.take_body())
            .await
            .expect("There was an error in the content stream");
        let response_content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("Response was missing Content-Type header");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response status was not 200"
        );
        assert_eq!(
            response_content_type, "video/mp4",
            "Response Content-Type was not video/mp4",
        );

        assert_eq!(
            response_body.len(),
            198946,
            "Response body did not have the expected size",
        );
    }

    #[actix_rt::test]
    async fn content_cannot_be_retrieved_if_no_acceptable_media_type() {
        let request = test_request(&sample_path("hello-world"), None, None)
            .uri("/hello")
            .header(
                header::ACCEPT,
                "application/msword, font/otf, audio/3gpp2;q=0.1",
            )
            .to_http_request();

        let response = get::<TestContentEngine>(request).await;

        assert_eq!(
            response.status(),
            StatusCode::NOT_ACCEPTABLE,
            "Response status was not 406"
        );
    }

    #[actix_rt::test]
    async fn extension_on_url_takes_precedence_over_accept_header() {
        // Note .txt extension on URL path, but no text/plain (nor any other
        // workable media range) in the accept header.
        let request = test_request(&sample_path("hello-world"), None, None)
            .uri("/hello.txt")
            .header(
                header::ACCEPT,
                "application/msword, font/otf, audio/3gpp2;q=0.1",
            )
            .to_http_request();

        let response = get::<TestContentEngine>(request).await;
        let response_content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("Response was missing Content-Type header");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response status was not 200"
        );
        assert_eq!(
            response_content_type, "text/plain",
            "Response Content-Type was not text/plain",
        );
    }

    #[actix_rt::test]
    async fn if_url_has_extension_accept_header_is_ignored() {
        // URL path extension has the wrong media type, but accept header has
        // the correct one. Should be HTTP 406 because the accept header is not
        // considered when there is an extension.
        let request = test_request(&sample_path("hello-world"), None, None)
            .uri("/hello.doc")
            .header(header::ACCEPT, "text/plain")
            .to_http_request();

        let response = get::<TestContentEngine>(request).await;

        assert_eq!(
            response.status(),
            StatusCode::NOT_ACCEPTABLE,
            "Response status was not 406"
        );
    }

    #[actix_rt::test]
    async fn index_route_is_used_for_empty_uri_path() {
        let request = test_request(&sample_path("hello-world"), Some("/hello"), None)
            .header(header::ACCEPT, "text/plain")
            .to_http_request();

        let mut response = get::<TestContentEngine>(request).await;
        let response_body = collect_response_body(response.take_body())
            .await
            .expect("There was an error in the content stream");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response status was not 200"
        );
        assert_eq!(response_body, "hello world", "Response body was incorrect");
    }

    #[actix_rt::test]
    async fn error_handler_is_given_http_status_code() {
        {
            let request_not_found =
                test_request(&sample_path("error-handling"), None, Some("/error-handler"))
                    .header(header::ACCEPT, "text/plain")
                    .uri("/not/a/real/path/so/this/should/404")
                    .to_http_request();

            let mut response = get::<TestContentEngine>(request_not_found).await;
            let response_body = collect_response_body(response.take_body())
                .await
                .expect("There was an error in the content stream");

            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "Response status was not 404"
            );
            assert_eq!(
                response_body, "error code: 404",
                "Response body was incorrect"
            );
        }

        {
            let request_not_acceptable_error =
                test_request(&sample_path("error-handling"), None, Some("/error-handler"))
                    .header(header::ACCEPT, "text/plain")
                    .uri("/json-file")
                    .to_http_request();

            let mut response = get::<TestContentEngine>(request_not_acceptable_error).await;
            let response_body = collect_response_body(response.take_body())
                .await
                .expect("There was an error in the content stream");

            assert_eq!(
                response.status(),
                StatusCode::NOT_ACCEPTABLE,
                "Response status was not 406"
            );
            assert_eq!(
                response_body, "error code: 406",
                "Response body was incorrect"
            );
        }
    }

    #[actix_rt::test]
    async fn stream_errors_are_propagated() {
        let request_internal_server_error =
            test_request(&sample_path("error-handling"), None, Some("/error-handler"))
                .header(header::ACCEPT, "text/plain")
                .uri("/trigger-error")
                .to_http_request();

        let mut response = get::<TestContentEngine>(request_internal_server_error).await;
        let response_body = collect_response_body(response.take_body()).await;

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response status was not 200"
        );

        assert_eq!(
            response_body.unwrap_err().to_string(),
            actix_web::Error::from(()).to_string()
        );
    }

    #[actix_rt::test]
    async fn error_handler_can_be_static_content() {
        let request = test_request(
            &sample_path("error-handling"),
            None,
            Some("/static-error-handler"),
        )
        .header(header::ACCEPT, "text/plain")
        .uri("/not/a/real/path/so/this/should/404")
        .to_http_request();

        let mut response = get::<TestContentEngine>(request).await;
        let response_body = collect_response_body(response.take_body())
            .await
            .expect("There was an error in the content stream");

        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "Response status was not 404"
        );
        assert_eq!(
            response_body, "this is static error handler\n",
            "Response body was incorrect"
        );
    }

    #[actix_rt::test]
    async fn error_handler_can_be_executable() {
        let request = test_request(
            &sample_path("error-handling"),
            None,
            Some("/executable-error-handler"),
        )
        .header(header::ACCEPT, "text/plain")
        .uri("/not/a/real/path/so/this/should/404")
        .to_http_request();

        let response = get::<TestContentEngine>(request).await;

        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "Response status was not 404"
        );
    }

    #[actix_rt::test]
    async fn error_handler_is_content_negotiated() {
        {
            let text_plain_request =
                test_request(&sample_path("error-handling"), None, Some("/error-handler"))
                    .header(header::ACCEPT, "text/plain")
                    .uri("/not/a/real/path/so/this/should/404")
                    .to_http_request();

            let mut response = get::<TestContentEngine>(text_plain_request).await;
            let response_body = collect_response_body(response.take_body())
                .await
                .expect("There was an error in the content stream");
            let response_content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .expect("Response was missing Content-Type header");

            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "Response status was not 404"
            );
            assert_eq!(
                response_body, "error code: 404",
                "Response body was incorrect"
            );
            assert_eq!(
                response_content_type, "text/plain",
                "Response Content-Type was not text/plain",
            );
        }

        {
            let text_html_request =
                test_request(&sample_path("error-handling"), None, Some("/error-handler"))
                    .header(header::ACCEPT, "text/html")
                    .uri("/not/a/real/path/so/this/should/404")
                    .to_http_request();

            let mut response = get::<TestContentEngine>(text_html_request).await;
            let response_body = collect_response_body(response.take_body())
                .await
                .expect("There was an error in the content stream");
            let response_content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .expect("Response was missing Content-Type header");

            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "Response status was not 404"
            );
            assert_eq!(
                response_body, "<p>error code: 404</p>",
                "Response body was incorrect"
            );
            assert_eq!(
                response_content_type, "text/html",
                "Response Content-Type was not text/html",
            );
        }
    }

    #[actix_rt::test]
    async fn use_a_default_error_handler_if_specified_handler_fails() {
        {
            // The error handler itself will trigger a rendering error.
            let request =
                test_request(&sample_path("error-handling"), None, Some("/trigger-error"))
                    .header(header::ACCEPT, "text/html")
                    .uri("/not/a/real/path/so/this/should/404")
                    .to_http_request();

            let mut response = get::<TestContentEngine>(request).await;
            let response_body = collect_response_body(response.take_body())
                .await
                .expect("There was an error in the content stream");
            let response_content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .expect("Response was missing Content-Type header");

            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "Response status was not 404"
            );
            assert_eq!(response_body, "Not Found", "Response body was incorrect");
            assert_eq!(
                response_content_type, "text/plain",
                "Response Content-Type was not text/plain",
            );
        }

        {
            // The error handler is fine, but is not an acceptable media type.
            let request =
                test_request(&sample_path("error-handling"), None, Some("/error-handler"))
                    .header(header::ACCEPT, "video/mp4")
                    .uri("/not/a/real/path/so/this/should/404")
                    .to_http_request();

            let mut response = get::<TestContentEngine>(request).await;
            let response_body = collect_response_body(response.take_body())
                .await
                .expect("There was an error in the content stream");
            let response_content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .expect("Response was missing Content-Type header");

            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "Response status was not 404"
            );
            assert_eq!(response_body, "Not Found", "Response body was incorrect");
            assert_eq!(
                // The default error handler always emits text/plain regardless
                // of the accept header.
                response_content_type,
                "text/plain",
                "Response Content-Type was not text/plain",
            );
        }
    }

    #[actix_rt::test]
    async fn error_handler_sees_original_request_route() {
        let request = test_request(
            &sample_path("error-handling"),
            None,
            Some("/error-code-and-request-info"),
        )
        .header(header::ACCEPT, "text/plain")
        .uri("/not/a/real/path/so/this/should/404")
        .to_http_request();

        let mut response = get::<TestContentEngine>(request).await;
        let response_body = collect_response_body(response.take_body())
            .await
            .expect("There was an error in the content stream");

        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "Response status was not 404"
        );
        assert_eq!(
            response_body, "404 /not/a/real/path/so/this/should/404",
            "Response body was incorrect"
        );
    }

    #[actix_rt::test]
    async fn query_parameters_are_handled() {
        let request = test_request(&sample_path("executables"), None, None)
            .uri("/render-data?a=hello&b=1&b=2&c")
            .to_http_request();
        let mut response = get::<TestContentEngine>(request).await;
        let response_body = collect_response_body(response.take_body())
            .await
            .expect("There was an error in the content stream");

        let response_json = serde_json::from_slice::<serde_json::Value>(&response_body)
            .expect("Could not parse JSON");

        assert_eq!(&response_json["request"]["query-parameters"]["a"], "hello");
        assert_eq!(&response_json["request"]["query-parameters"]["b"], "2");
        assert_eq!(&response_json["request"]["query-parameters"]["c"], "");
    }

    #[actix_rt::test]
    async fn query_parameters_are_forwarded_to_getted_content() {
        let request = test_request(&sample_path("executables"), None, None)
            .uri("/get-render-data?hello=world")
            .to_http_request();
        let mut response = get::<TestContentEngine>(request).await;
        let response_body = collect_response_body(response.take_body())
            .await
            .expect("There was an error in the content stream");

        let response_json = serde_json::from_slice::<serde_json::Value>(&response_body)
            .expect("Could not parse JSON");

        assert_eq!(
            &response_json["request"]["query-parameters"]["hello"],
            "world"
        );
    }

    #[actix_rt::test]
    async fn query_parameters_are_forwarded_to_error_handler() {
        let request = test_request(
            &sample_path("error-handling"),
            None,
            Some("/error-code-and-request-info"),
        )
        .uri("/this-route-will-404?hello=world")
        .to_http_request();
        let mut response = get::<TestContentEngine>(request).await;
        let response_body = collect_response_body(response.take_body())
            .await
            .expect("There was an error in the content stream");

        assert_eq!(&response_body, "404 /this-route-will-404\nhello: world");
    }
}
