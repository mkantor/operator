use crate::content::*;
use actix_rt::System;
use actix_web::http::header::{self, Header};
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer};
use mime::Mime;
use std::cmp::Ordering;
use std::io;
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

    log::info!("Getting content for \"/{}\"", path);

    let route = if path.is_empty() {
        &app_data.index_route
    } else {
        path
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

    let render_result = {
        let content_engine = app_data
            .shared_content_engine
            .read()
            .expect("RwLock for ContentEngine has been poisoned");

        content_engine.get(route).map(|content| {
            let render_context = content_engine.get_render_context();

            // TODO: See if I can remove this `clone` and `collect` as I get
            // further along. Can probably make Render trait more generic.
            let acceptable_media_ranges = parsed_accept_header_value
                .iter()
                .map(|quality_item| quality_item.item.clone())
                .collect::<Vec<Mime>>();
            content.render(render_context, &acceptable_media_ranges)
        })
    };

    // FIXME: Is logging response status problematic? It could leak info on a
    // site with dynamic state. Maybe make these logs trace level?
    match render_result {
        Some(Ok(body)) => {
            log::info!("Successfully rendered content from route \"/{}\"", route);
            // FIXME: Need to make `.render()` return the resulting media type.
            let hardcoded_placeholder_content_type = mime::TEXT_HTML;
            HttpResponse::Ok()
                .content_type(hardcoded_placeholder_content_type.essence_str())
                .body(body)
        }
        Some(Err(error @ ContentRenderingError::CannotProvideAcceptableMediaType { .. })) => {
            log::warn!("Cannot provide acceptable media: {}", error);
            HttpResponse::NotAcceptable()
                .content_type(mime::TEXT_PLAIN.essence_str())
                .body("Cannot provide an acceptable response.")
        }
        Some(Err(error)) => {
            log::warn!(
                "Failed to render content from route \"/{}\": {}",
                route,
                error
            );
            HttpResponse::InternalServerError()
                .content_type(mime::TEXT_PLAIN.essence_str())
                .body("Unable to fulfill request.")
        }
        None => {
            log::warn!("No content found at \"/{}\"", route);
            HttpResponse::NotFound()
                .content_type(mime::TEXT_PLAIN.essence_str())
                .body("Not found.")
        }
    }
}
