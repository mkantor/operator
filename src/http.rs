use crate::content::*;
use actix_rt::System;
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer};
use std::net::ToSocketAddrs;
use std::sync::{Arc, RwLock};

pub fn run_server<A, E>(content_engine: Arc<RwLock<E>>, index_address: &str, socket_address: A)
where
    A: 'static + ToSocketAddrs,
    E: 'static + ContentEngine + Send + Sync,
{
    log::info!("Initializing HTTP server");
    let mut system = System::new("server");
    system
        .block_on(async move {
            HttpServer::new(move || {
                App::new()
                    .app_data(content_engine.clone())
                    .route("/{address:.*}", web::get().to(get::<E>))
            })
            .bind(socket_address)?
            .run()
            .await
        })
        .unwrap(); // TODO: Return Result.
    log::info!("HTTP server has terminated");
}

async fn get<E: 'static + ContentEngine + Send + Sync>(request: HttpRequest) -> HttpResponse {
    let address = request
        .match_info()
        .get("address") // TODO: Use index_address if address is empty.
        .expect("No address provided!");

    log::info!("Getting content for \"/{}\"", address);

    let locked_engine = request
        .app_data::<Arc<RwLock<E>>>()
        .expect("App data was not of the expected type!");

    let content_engine = locked_engine
        .read()
        .expect("RwLock for ContentEngine has been poisoned");

    let result = content_engine.get(address).map(|content| {
        // TODO: Content negotation!
        let render_context = content_engine.get_render_context(&mime::TEXT_HTML);
        content.render(&render_context)
    });

    // FIXME: Is logging response status problematic? It could leak info on a
    // site with dynamic state. Maybe make these logs trace level?
    match result {
        Some(Ok(body)) => {
            log::info!("Successfully rendered content for \"/{}\"", address);
            HttpResponse::Ok().body(body)
        }
        Some(Err(error)) => {
            log::warn!("Failed to render content for \"/{}\"", address);
            HttpResponse::InternalServerError().body(error.to_string())
        }
        None => {
            log::warn!("No content found at \"/{}\"", address);
            HttpResponse::NotFound().body("Not found.")
        }
    }
}
