use serde::Serialize;

pub mod cli;
pub mod content;
pub mod http;
pub mod test_lib;

const VERSION: ServerVersion = ServerVersion(env!("CARGO_PKG_VERSION"));

#[derive(Clone, Copy, Serialize)]
pub struct ServerVersion(pub &'static str);

#[derive(Clone, Serialize)]
pub struct ServerInfo {
    pub version: ServerVersion,
}

impl Default for ServerInfo {
    fn default() -> Self {
        ServerInfo { version: VERSION }
    }
}

#[macro_export]
macro_rules! bug_message {
    () => {
        "You've encountered a bug in Operator! Please open an issue at <https://github.com/mkantor/operator/issues>."
    };
    ($detail:expr$(,)?) => {
        concat!(bug_message!(), " ", $detail)
    };
}
