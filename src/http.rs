use crate::content::*;
use std::net::ToSocketAddrs;
use std::sync::{Arc, RwLock};

pub fn run_server<A, E>(content_engine: Arc<RwLock<E>>, index_address: &str, socket_address: A)
where
    A: 'static + ToSocketAddrs,
    E: 'static + ContentEngine + Send + Sync,
{
    todo!()
}
