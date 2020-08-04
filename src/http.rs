use crate::content::*;
use actix_rt::System;
use actix_web::http::header::{self, Header};
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer};
use mime_guess::MimeGuess;
use std::cmp::Ordering;
use std::io::{self, Read};
use std::net::ToSocketAddrs;
use std::sync::{Arc, RwLock};

struct AppData<E: 'static + ContentEngine + Send + Sync> {
    shared_content_engine: Arc<RwLock<E>>,
    index_route: String,
}

pub fn run_server<A, E>(
    shared_content_engine: Arc<RwLock<E>>,
    index_route: &str,
    socket_address: A,
) -> Result<(), io::Error>
where
    A: 'static + ToSocketAddrs,
    E: 'static + ContentEngine + Send + Sync,
{
    let index_route = String::from(index_route);

    log::info!("Initializing HTTP server");
    let mut system = System::new("server");
    let result = system.block_on(async move {
        HttpServer::new(move || {
            App::new()
                .app_data(AppData {
                    shared_content_engine: shared_content_engine.clone(),
                    index_route: index_route.clone(),
                })
                .route("/{path:.*}", web::get().to(get::<E>))
        })
        .bind(socket_address)?
        .run()
        .await
    });

    log::info!("HTTP server has terminated");
    result
}

async fn get<E: 'static + ContentEngine + Send + Sync>(request: HttpRequest) -> HttpResponse {
    let app_data = request
        .app_data::<AppData<E>>()
        .expect("App data was not of the expected type!");

    let path = request
        .match_info()
        .get("path")
        .expect("Failed to match request path!");

    let (route, media_range_from_url) = if path.is_empty() {
        // Default to the index route.
        (app_data.index_route.as_str(), None)
    } else {
        // If the path has an extension which maps to a media range, use it as
        // the most-acceptable media range (give it a higher quality value than
        // everything in the accept header).
        //
        // This somewhat-unusual feature exists because there is not a great
        // way to link to particular representations of resources on the web
        // without putting something in the URL, and it's awfully convenient
        // for mere humans to be able to do this (compare "to get my resume in
        // PDF format, visit http://mysite.com/resume.pdf" to "...first install
        // this browser extension that lets you customize HTTP headers, then
        // set the accept header to application/pdf, then visit
        // http://mysite.com/resume").
        let media_range_from_url = MimeGuess::from_path(path).first();
        let route = if media_range_from_url.is_some() {
            // Drop the extension from the path.
            path.rsplitn(2, '.').last().expect(
                "Calling rsplitn(2, ..) on a non-empty string returned an empty iterator. This should be impossible!"
            )
        } else {
            path
        };
        (route, media_range_from_url)
    };

    let mut parsed_accept_header_value = match header::Accept::parse(&request) {
        Ok(accept_value) => accept_value,
        Err(error) => {
            log::warn!(
                "Malformed accept header value `{:?}` in request for \"/{}\": {}",
                request.headers().get("accept"),
                route,
                error
            );
            return HttpResponse::BadRequest()
                .content_type(mime::TEXT_PLAIN.essence_str())
                .body("Malformed accept header value.");
        }
    };

    // If the accept header value is empty, allow any media type.
    if parsed_accept_header_value.is_empty() {
        log::info!("Getting content for /{}", path);
        parsed_accept_header_value = header::Accept(vec![header::qitem(mime::STAR_STAR)]);
    } else {
        log::info!(
            "Getting content for /{} with accept: {}",
            path,
            parsed_accept_header_value
        );
    }

    // Sort in order of descending quality (so the client's most-preferred
    // representation is first).
    //
    // Note that QualityItem only implements PartialOrd, not Ord. I thought
    // that might be because the parser lossily converts decimal strings into
    // integers (for the `q` parameter), but it turns out the implementation
    // actually never returns None (as of actix-web v3.0.0). If that ever
    // changes and there is some scenario where a pair of items from the
    // accept header can't be ordered then soliton will give them equal
    // preference. ¯\_(ツ)_/¯
    parsed_accept_header_value.sort_by(|a, b| {
        b.partial_cmp(a).unwrap_or_else(|| {
            log::warn!(
                "Accept header items `{}` and `{}` could not be ordered by quality",
                a,
                b
            );
            Ordering::Equal
        })
    });

    let acceptable_media_ranges = media_range_from_url.iter().chain(
        parsed_accept_header_value
            .iter()
            .map(|quality_item| &quality_item.item),
    );

    let render_result = {
        let content_engine = app_data
            .shared_content_engine
            .read()
            .expect("RwLock for ContentEngine has been poisoned");

        content_engine.get(route).map(|content| {
            let render_context = content_engine.get_render_context();
            content.render(render_context, acceptable_media_ranges)
        })
    };

    match render_result {
        Some(Ok(Media {
            mut content,
            media_type,
        })) => {
            let mut response_bytes = Vec::new();
            match content.read_to_end(&mut response_bytes) {
                Ok(_) => {
                    log::info!("Successfully rendered content from route /{}", route);
                    HttpResponse::Ok()
                        .content_type(media_type.to_string())
                        .body(response_bytes)
                }
                Err(error) => {
                    log::error!("Failed to read content for /{}: {}", route, error);
                    HttpResponse::InternalServerError()
                        .content_type("text/plain")
                        .body("Unable to fulfill request.")
                }
            }
        }
        Some(Err(error @ ContentRenderingError::CannotProvideAcceptableMediaType { .. })) => {
            log::warn!("Cannot provide an acceptable response: {}", error);
            HttpResponse::NotAcceptable()
                .content_type("text/plain")
                .body("Cannot provide an acceptable response.")
        }
        Some(Err(error)) => {
            log::warn!("Failed to render content from route /{}: {}", route, error);
            HttpResponse::InternalServerError()
                .content_type("text/plain")
                .body("Unable to fulfill request.")
        }
        None => {
            log::warn!("No content found at /{}", route);
            HttpResponse::NotFound()
                .content_type("text/plain")
                .body("Not found.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lib::*;
    use actix_web::body::Body;
    use actix_web::http::StatusCode;
    use actix_web::test::TestRequest;
    use std::path::Path;

    fn test_request(content_directory_path: &Path, url_path: &'static str) -> TestRequest {
        let directory = ContentDirectory::from_root(&content_directory_path).unwrap();
        let shared_content_engine = FilesystemBasedContentEngine::from_content_directory(
            directory,
            SolitonVersion("0.0.0"),
        )
        .expect("Content engine could not be created");

        TestRequest::default()
            .app_data(AppData {
                shared_content_engine: shared_content_engine,
                index_route: String::new(),
            })
            .param("path", url_path)
    }

    #[actix_rt::test]
    async fn content_may_be_not_found() {
        let request =
            test_request(&example_path("empty"), "nothing/exists/at/this/path").to_http_request();
        let response = get::<FilesystemBasedContentEngine>(request).await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[actix_rt::test]
    async fn content_can_be_retrieved_with_exact_media_type() {
        let request = test_request(&example_path("hello-world"), "hello")
            .header("accept", "text/html")
            .to_http_request();

        let response = get::<FilesystemBasedContentEngine>(request).await;
        let response_body = response
            .body()
            .as_ref()
            .expect("Response body was not available");
        let response_content_type = response
            .headers()
            .get("content-type")
            .expect("Response was missing content-type header");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response status was not 200 OK"
        );
        assert_eq!(
            response_content_type, "text/html",
            "Response content-type was not text/html",
        );
        assert_eq!(
            response_body,
            &Body::from_slice(b"hello world\n"),
            "Response body was incorrect"
        );
    }

    #[actix_rt::test]
    async fn content_can_be_retrieved_with_media_range() {
        let request = test_request(&example_path("hello-world"), "hello")
            .header("accept", "text/*")
            .to_http_request();

        let response = get::<FilesystemBasedContentEngine>(request).await;
        let response_body = response
            .body()
            .as_ref()
            .expect("Response body was not available");
        let response_content_type = response
            .headers()
            .get("content-type")
            .expect("Response was missing content-type header");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response status was not 200 OK"
        );
        assert_eq!(
            response_content_type, "text/html",
            "Response content-type was not text/html",
        );
        assert_eq!(
            response_body,
            &Body::from_slice(b"hello world\n"),
            "Response body was incorrect"
        );
    }

    #[actix_rt::test]
    async fn content_can_be_retrieved_with_star_star_media_range() {
        let request = test_request(&example_path("hello-world"), "hello")
            .header("accept", "*/*")
            .to_http_request();

        let response = get::<FilesystemBasedContentEngine>(request).await;
        let response_body = response
            .body()
            .as_ref()
            .expect("Response body was not available");
        let response_content_type = response
            .headers()
            .get("content-type")
            .expect("Response was missing content-type header");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response status was not 200 OK"
        );
        assert_eq!(
            response_content_type, "text/html",
            "Response content-type was not text/html",
        );
        assert_eq!(
            response_body,
            &Body::from_slice(b"hello world\n"),
            "Response body was incorrect"
        );
    }

    #[actix_rt::test]
    async fn content_can_be_retrieved_with_elaborate_accept_header() {
        let request = test_request(&example_path("hello-world"), "hello")
            .header("accept", "audio/aac, text/*;q=0.9, image/gif;q=0.1")
            .to_http_request();

        let response = get::<FilesystemBasedContentEngine>(request).await;
        let response_body = response
            .body()
            .as_ref()
            .expect("Response body was not available");
        let response_content_type = response
            .headers()
            .get("content-type")
            .expect("Response was missing content-type header");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response status was not 200 OK"
        );
        assert_eq!(
            response_content_type, "text/html",
            "Response content-type was not text/html",
        );
        assert_eq!(
            response_body,
            &Body::from_slice(b"hello world\n"),
            "Response body was incorrect"
        );
    }

    #[actix_rt::test]
    async fn content_can_be_retrieved_with_missing_accept_header() {
        let request = test_request(&example_path("hello-world"), "hello").to_http_request();

        let response = get::<FilesystemBasedContentEngine>(request).await;
        let response_body = response
            .body()
            .as_ref()
            .expect("Response body was not available");
        let response_content_type = response
            .headers()
            .get("content-type")
            .expect("Response was missing content-type header");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response status was not 200 OK"
        );
        assert_eq!(
            response_content_type, "text/html",
            "Response content-type was not text/html",
        );
        assert_eq!(
            response_body,
            &Body::from_slice(b"hello world\n"),
            "Response body was incorrect"
        );
    }

    #[actix_rt::test]
    async fn multimedia_content_can_be_retrieved() {
        let request = test_request(&example_path("multimedia"), "dramatic-prairie-dog")
            .header("accept", "video/*")
            .to_http_request();

        let response: HttpResponse = get::<FilesystemBasedContentEngine>(request).await;
        let response_body = response
            .body()
            .as_ref()
            .expect("Response body was not available");
        let response_content_type = response
            .headers()
            .get("content-type")
            .expect("Response was missing content-type header");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response status was not 200 OK"
        );
        assert_eq!(
            response_content_type, "video/mp4",
            "Response content-type was not video/mp4",
        );

        let response_bytes = match response_body {
            Body::None => vec![],
            Body::Empty => vec![],
            Body::Bytes(bytes) => bytes.to_vec(),
            Body::Message(_) => {
                unimplemented!("can't get bytes from response with generic message body")
            }
        };
        assert_eq!(
            response_bytes.len(),
            198946,
            "Response body did not have the expected size",
        );
    }

    #[actix_rt::test]
    async fn content_cannot_be_retrieved_if_no_acceptable_media_type() {
        let request = test_request(&example_path("hello-world"), "hello")
            .header("accept", "application/msword, font/otf, audio/3gpp2;q=0.1")
            .to_http_request();

        let response = get::<FilesystemBasedContentEngine>(request).await;

        assert_eq!(
            response.status(),
            StatusCode::NOT_ACCEPTABLE,
            "Response status was not 406 Not Acceptable"
        );
    }

    #[actix_rt::test]
    async fn extension_on_url_adds_acceptable_media_type() {
        // Note .html extension on URL path, but no text/html (nor any other
        // workable media range) in the accept header.
        let request = test_request(&example_path("hello-world"), "hello.html")
            .header("accept", "application/msword, font/otf, audio/3gpp2;q=0.1")
            .to_http_request();

        let response = get::<FilesystemBasedContentEngine>(request).await;
        let response_content_type = response
            .headers()
            .get("content-type")
            .expect("Response was missing content-type header");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Response status was not 200 OK"
        );
        assert_eq!(
            response_content_type, "text/html",
            "Response content-type was not text/html",
        );
    }
}
