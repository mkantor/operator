use crate::content::*;
use actix_rt::System;
use actix_web::{middleware, web, App, HttpResponse, HttpServer, Responder};
use log::*;
use std::net::ToSocketAddrs;
use std::sync::{Arc, RwLock};

pub fn run_server<A, E>(content_engine: Arc<RwLock<E>>, index_address: &str, socket_address: A)
where
    A: 'static + ToSocketAddrs,
    E: 'static + ContentEngine + Send + Sync,
{
    stderrlog::new()
        .verbosity(3)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap(); // FIXME: Return a Result from run_server.

    info!("Initializing HTTP server");
    let mut system = System::new("server");
    system
        .block_on(async move {
            HttpServer::new(|| {
                App::new()
                    .wrap(middleware::Logger::default())
                    .route("/", web::get().to(index))
            })
            .bind(socket_address)?
            .run()
            .await
        })
        .unwrap(); // TODO: Return Result.
    info!("HTTP server has terminated");
}

async fn index() -> impl Responder {
    debug!("👋 yo!");
    HttpResponse::Ok().body("Hello world!")
}
