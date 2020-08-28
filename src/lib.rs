use serde::Serialize;

#[derive(Clone, Copy, Serialize)]
pub struct ServerVersion(pub &'static str);

#[derive(Clone, Serialize)]
pub struct ServerInfo {
    pub version: ServerVersion,
}
