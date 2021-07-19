use serde::Serialize;
use std::env;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::PathBuf;
use thiserror::Error;

pub mod cli;
pub mod content;
pub mod http;

#[doc(hidden)]
pub mod test_lib;

#[derive(Clone, Copy, Serialize)]
pub struct ServerVersion(pub &'static str);

const VERSION: ServerVersion = ServerVersion(env!("CARGO_PKG_VERSION"));

#[derive(Error, Debug)]
pub enum ServerInfoError {
    #[error(transparent)]
    IoError {
        #[from]
        source: io::Error,
    },
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ServerInfo {
    pub version: ServerVersion,
    pub operator_path: PathBuf,
    pub socket_address: Option<SocketAddr>,
}

impl ServerInfo {
    fn with_socket_address<A: 'static + ToSocketAddrs>(
        socket_address: &A,
    ) -> Result<Self, ServerInfoError> {
        Ok(ServerInfo {
            version: VERSION,
            operator_path: env::current_exe()?,
            // If there's more than one SocketAddr, use the first.
            socket_address: socket_address.to_socket_addrs()?.next(),
        })
    }
    fn without_socket_address() -> Result<Self, ServerInfoError> {
        Ok(ServerInfo {
            version: VERSION,
            operator_path: env::current_exe()?,
            socket_address: None,
        })
    }
}

#[doc(hidden)]
#[macro_export]
macro_rules! bug_message {
    () => {
        "You've encountered a bug in Operator! Please open an issue at <https://github.com/mkantor/operator/issues>."
    };
    ($detail:expr$(,)?) => {
        concat!(bug_message!(), " ", $detail)
    };
}
