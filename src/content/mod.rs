mod content_directory;
mod content_engine;
mod content_index;
mod content_item;
mod content_registry;
mod handlebars_helpers;
mod mime;
mod test_lib;

use crate::lib::*;
use content_index::*;
use serde::Serialize;
use std::io::Read;

pub use self::mime::{MediaRange, MediaType};
pub use content_directory::ContentDirectory;
pub use content_engine::{
    ContentEngine, ContentLoadingError, FilesystemBasedContentEngine, RegisteredTemplateParseError,
    UnregisteredTemplateParseError,
};
pub use content_item::{ContentRenderingError, UnregisteredTemplate};
pub use content_registry::RegisteredContent;

const HANDLEBARS_FILE_EXTENSION: &str = "hbs";

pub struct Media<O: Read> {
    pub media_type: MediaType,
    pub content: O,
}
impl<O: Read> Media<O> {
    fn new(media_type: MediaType, content: O) -> Self {
        Self {
            media_type,
            content,
        }
    }
}

pub trait Render {
    type Output;
    fn render<'engine, 'accept, E, A>(
        &self,
        context: RenderContext<'engine, E>,
        acceptable_media_ranges: A,
    ) -> Result<Media<Self::Output>, ContentRenderingError>
    where
        E: ContentEngine,
        A: IntoIterator<Item = &'accept MediaRange>,
        Self::Output: Read;
}

#[derive(Clone, Serialize)]
struct SolitonRenderData {
    version: SolitonVersion,
}

// These must match up with serialized property names in RenderData.
const REQUEST_ROUTE_PROPERTY_NAME: &str = "request-route";
const TARGET_MEDIA_TYPE_PROPERTY_NAME: &str = "target-media-type";

#[derive(Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
struct RenderData {
    soliton: SolitonRenderData,
    content: ContentIndex,
    request_route: String,
    target_media_type: Option<MediaType>,
}

pub struct RenderContext<'engine, E: ContentEngine> {
    content_engine: &'engine E,
    data: RenderData,
}
