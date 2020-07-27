use crate::content::*;
use actix_rt::System;
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer};
use std::net::ToSocketAddrs;
use std::sync::{Arc, RwLock};

struct AppData<E: 'static + ContentEngine + Send + Sync> {
    locked_content_engine: Arc<RwLock<E>>,
    index_address: String,
}

pub fn run_server<A, E>(
    locked_content_engine: Arc<RwLock<E>>,
    index_address: &str,
    socket_address: A,
) where
    A: 'static + ToSocketAddrs,
    E: 'static + ContentEngine + Send + Sync,
{
    let index_address = String::from(index_address);

    log::info!("Initializing HTTP server");
    let mut system = System::new("server");
    system
        .block_on(async move {
            HttpServer::new(move || {
                App::new()
                    .app_data(AppData {
                        locked_content_engine: locked_content_engine.clone(),
                        index_address: index_address.clone(),
                    })
                    .route("/{path:.*}", web::get().to(get::<E>))
            })
            .bind(socket_address)?
            .run()
            .await
        })
        .unwrap(); // TODO: Return Result.

    log::info!("HTTP server has terminated");
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

    let content_engine = app_data
        .locked_content_engine
        .read()
        .expect("RwLock for ContentEngine has been poisoned");

    let address = if path.is_empty() {
        &app_data.index_address
    } else {
        path
    };
    let result = content_engine.get(address).map(|content| {
        // TODO: Content negotation!
        let render_context = content_engine.get_render_context(&mime::TEXT_HTML);
        content.render(&render_context)
    });

    // FIXME: Is logging response status problematic? It could leak info on a
    // site with dynamic state. Maybe make these logs trace level?
    match result {
        Some(Ok(body)) => {
            log::info!(
                "Successfully rendered content from address \"/{}\"",
                address
            );
            HttpResponse::Ok().body(body)
        }
        Some(Err(error)) => {
            log::warn!("Failed to render content from address \"/{}\"", address);
            HttpResponse::InternalServerError().body(error.to_string())
        }
        None => {
            log::warn!("No content found at \"/{}\"", address);
            HttpResponse::NotFound().body("Not found.")
        }
    }
}
