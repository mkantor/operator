mod body;
mod content_directory;
mod content_engine;
mod content_index;
mod content_item;
mod content_registry;
mod handlebars_helpers;
mod mime;
mod route;
mod test_lib;

use crate::bug_message;
use bytes::Bytes;
use content_index::ContentIndex;
use content_item::RenderingFailedError;
use futures::Stream;
use serde::Serialize;
use std::io;
use thiserror::Error;

pub use self::mime::{MediaRange, MediaType};
pub use content_directory::ContentDirectory;
pub use content_engine::{
    ContentEngine, ContentLoadingError, FilesystemBasedContentEngine, TemplateParseError,
};
pub use content_item::UnregisteredTemplate;
pub use content_registry::{ContentRepresentations, RegisteredContent};
pub use route::Route;

// This is just a trait alias to help make type signatures a bit saner.
pub trait ByteStream: Stream<Item = Result<Bytes, StreamError>>
where
    Self: Unpin,
{
}
impl<T> ByteStream for T where T: Stream<Item = Result<Bytes, StreamError>> + Unpin {}

/// A piece of rendered content along with its media type.
pub struct Media<Content: ByteStream> {
    pub media_type: MediaType,
    pub content: Content,
}
impl<Content: ByteStream> Media<Content> {
    fn new(media_type: MediaType, content: Content) -> Self {
        Self {
            media_type,
            content,
        }
    }
}

/// Could not produce rendered output, either because rendering was attempted
/// and failed or because no acceptable media types are available.
#[derive(Error, Debug)]
pub enum RenderError {
    #[error(transparent)]
    RenderingFailed(RenderingFailedError),

    #[error("The requested content cannot be rendered as an acceptable media type.")]
    CannotProvideAcceptableMediaType,

    #[error("{} This should never happen: {}", bug_message!(), .0)]
    Bug(String),
}

/// Something went wrong after starting to stream content.
#[derive(Error, Debug)]
pub enum StreamError {
    #[error(
        "Process exited with {}{}",
        match .exit_code {
            Some(code) => format!("code {}", code),
            None => String::from("unknown code"),
        },
        .stderr_contents.as_ref().map(|message| format!(": {}", message)).unwrap_or_default(),
    )]
    ExecutableExitedWithNonzero {
        pid: u32,
        exit_code: Option<i32>,
        stderr_contents: Option<String>,
    },

    #[error("Executable output could not be captured")]
    ExecutableOutputCouldNotBeCaptured { pid: u32 },

    #[error("Input/output error during rendering")]
    IOError {
        #[from]
        source: io::Error,
    },

    #[error("Stream was cancelled")]
    Canceled,
}

pub trait Render {
    type Output;
    fn render<'engine, 'accept, ServerInfo, Engine, Accept>(
        &self,
        context: RenderContext<'engine, ServerInfo, Engine>,
        acceptable_media_ranges: Accept,
    ) -> Result<Media<Self::Output>, RenderError>
    where
        ServerInfo: Clone + Serialize,
        Engine: ContentEngine<ServerInfo>,
        Accept: IntoIterator<Item = &'accept MediaRange>,
        Self::Output: ByteStream;
}

// These must match up with serialized property names in RenderData.
const REQUEST_ROUTE_PROPERTY_NAME: &str = "request-route";
const TARGET_MEDIA_TYPE_PROPERTY_NAME: &str = "target-media-type";

#[derive(Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
struct RenderData<ServerInfo: Clone + Serialize> {
    #[serde(rename = "/")]
    index: ContentIndex,
    server_info: ServerInfo,
    request_route: Option<Route>,
    target_media_type: Option<MediaType>,
    error_code: Option<u16>,
}

/// Values used during rendering, including the data passed to handlebars
/// templates.
pub struct RenderContext<'engine, ServerInfo, Engine>
where
    ServerInfo: Clone + Serialize,
    Engine: ContentEngine<ServerInfo>,
{
    content_engine: &'engine Engine,
    data: RenderData<ServerInfo>,
}

impl<'engine, ServerInfo, Engine> RenderContext<'engine, ServerInfo, Engine>
where
    ServerInfo: Clone + Serialize,
    Engine: ContentEngine<ServerInfo>,
{
    pub fn into_error_context(self, error_code: u16) -> Self {
        RenderContext {
            data: RenderData {
                error_code: Some(error_code),
                ..self.data
            },
            ..self
        }
    }
}
