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
use std::fmt;

pub use self::mime::{MediaRange, MediaType};
pub use content_directory::ContentDirectory;
pub use content_engine::{
    ContentEngine, ContentLoadingError, FilesystemBasedContentEngine, RegisteredTemplateParseError,
    UnregisteredTemplateParseError,
};
pub use content_item::{ContentRenderingError, UnregisteredTemplate};
pub use content_registry::RegisteredContent;

const HANDLEBARS_FILE_EXTENSION: &str = "hbs";

pub struct Media {
    pub media_type: MediaType,
    pub content: String,
}
impl Media {
    fn new(media_type: MediaType, content: String) -> Self {
        Self {
            content,
            media_type,
        }
    }
}
impl fmt::Display for Media {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.content)
    }
}

pub trait Render {
    fn render<'engine, 'accept, E, A>(
        &self,
        context: RenderContext<'engine, E>,
        acceptable_media_ranges: A,
    ) -> Result<Media, ContentRenderingError>
    where
        E: ContentEngine,
        A: IntoIterator<Item = &'accept MediaRange>;
}

#[derive(Clone, Serialize)]
struct SolitonRenderData {
    version: SolitonVersion,
}

const TARGET_MEDIA_TYPE_PROPERTY_NAME: &str = "target-media-type";

#[derive(Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
struct RenderData {
    soliton: SolitonRenderData,
    content: ContentIndex,
    target_media_type: Option<MediaType>, // Field name must align with TARGET_MEDIA_TYPE_PROPERTY_NAME.
}

pub struct RenderContext<'engine, E: ContentEngine> {
    content_engine: &'engine E,
    data: RenderData,
}
