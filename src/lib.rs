use serde::Serialize;

pub mod cli;
pub mod content;
pub mod http;
pub mod test_lib;

#[derive(Clone, Copy, Serialize)]
pub struct ServerVersion(pub &'static str);

#[derive(Clone, Serialize)]
pub struct ServerInfo {
    pub version: ServerVersion,
}
