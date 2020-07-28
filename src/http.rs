use crate::content::*;
use actix_rt::System;
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer};
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
    let path = request
        .match_info()
        .get("path")
        .expect("Failed to match request path!");

    log::info!("Getting content for \"/{}\"", path);

    let app_data = request
        .app_data::<AppData<E>>()
        .expect("App data was not of the expected type!");

    let route = if path.is_empty() {
        &app_data.index_route
    } else {
        path
    };

    let render_result = {
        let content_engine = app_data
            .shared_content_engine
            .read()
            .expect("RwLock for ContentEngine has been poisoned");

        content_engine.get(route).map(|content| {
            // TODO: Content negotiation!
            let render_context = content_engine.get_render_context(&mime::TEXT_HTML);
            content.render(&render_context)
        })
    };

    // FIXME: Is logging response status problematic? It could leak info on a
    // site with dynamic state. Maybe make these logs trace level?
    match render_result {
        Some(Ok(body)) => {
            log::info!("Successfully rendered content from route \"/{}\"", route);
            HttpResponse::Ok()
                .content_type(mime::TEXT_HTML.essence_str())
                .body(body)
        }
        Some(Err(error)) => {
            log::warn!("Failed to render content from route \"/{}\"", route);
            HttpResponse::InternalServerError()
                .content_type(mime::TEXT_HTML.essence_str())
                .body(error.to_string())
        }
        None => {
            log::warn!("No content found at \"/{}\"", route);
            HttpResponse::NotFound()
                .content_type(mime::TEXT_HTML.essence_str())
                .body("Not found.")
        }
    }
}
