mod content_directory;
mod content_engine;
mod content_index;
mod content_item;
mod content_registry;
mod handlebars_helpers;
mod mime;
mod test_lib;

use content_index::ContentIndex;
use content_item::RenderingFailedError;
use serde::Serialize;
use std::io::Read;
use thiserror::Error;

pub use self::mime::{MediaRange, MediaType};
pub use content_directory::ContentDirectory;
pub use content_engine::{
    ContentEngine, ContentLoadingError, FilesystemBasedContentEngine, TemplateParseError,
};
pub use content_item::UnregisteredTemplate;
pub use content_registry::{ContentRepresentations, RegisteredContent};

/// A piece of rendered content along with its media type.
pub struct Media<Content: Read> {
    pub media_type: MediaType,
    pub content: Content,
}
impl<Content: Read> Media<Content> {
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

    #[error("You've encountered a bug! This should never happen: {}", .0)]
    Bug(String),
}

pub trait Render {
    type Output;
    fn render<'engine, 'accept, ServerInfo, ErrorCode, Engine, Accept>(
        &self,
        context: RenderContext<'engine, ServerInfo, ErrorCode, Engine>,
        acceptable_media_ranges: Accept,
    ) -> Result<Media<Self::Output>, RenderError>
    where
        ServerInfo: Clone + Serialize,
        ErrorCode: Clone + Serialize,
        Engine: ContentEngine<ServerInfo, ErrorCode>,
        Accept: IntoIterator<Item = &'accept MediaRange>,
        Self::Output: Read;
}

// These must match up with serialized property names in RenderData.
const REQUEST_ROUTE_PROPERTY_NAME: &str = "request-route";
const TARGET_MEDIA_TYPE_PROPERTY_NAME: &str = "target-media-type";

#[derive(Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
struct RenderData<ServerInfo: Clone + Serialize, ErrorCode: Clone + Serialize> {
    #[serde(rename = "/")]
    index: ContentIndex,
    server_info: ServerInfo,
    request_route: String,
    target_media_type: Option<MediaType>,
    error_code: Option<ErrorCode>,
}

/// Values used during rendering, including the data passed to handlebars
/// templates.
pub struct RenderContext<'engine, ServerInfo, ErrorCode, Engine>
where
    ServerInfo: Clone + Serialize,
    ErrorCode: Clone + Serialize,
    Engine: ContentEngine<ServerInfo, ErrorCode>,
{
    content_engine: &'engine Engine,
    data: RenderData<ServerInfo, ErrorCode>,
}

impl<'engine, ServerInfo, ErrorCode, Engine> RenderContext<'engine, ServerInfo, ErrorCode, Engine>
where
    ServerInfo: Clone + Serialize,
    ErrorCode: Clone + Serialize,
    Engine: ContentEngine<ServerInfo, ErrorCode>,
{
    pub fn into_error_context(self, error_code: ErrorCode) -> Self {
        RenderContext {
            data: RenderData {
                error_code: Some(error_code),
                ..self.data
            },
            ..self
        }
    }
}
