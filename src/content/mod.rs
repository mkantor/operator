mod content_engine;
mod content_index;
mod content_item;
mod handlebars_helpers;
mod mime;
mod test_lib;

use crate::lib::*;
use content_index::*;
use serde::Serialize;
use std::fmt;

pub use self::mime::{MediaRange, MediaType};
pub use content_engine::{
    ContentEngine, ContentLoadingError, FilesystemBasedContentEngine, RegisteredContent,
    RegisteredTemplateParseError, UnregisteredTemplateParseError,
};
pub use content_item::{ContentRenderingError, UnregisteredTemplate};

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
    fn render<'engine, E: ContentEngine>(
        &self,
        context: RenderContext<'engine, E>,
        acceptable_media_ranges: &[MediaRange],
    ) -> Result<Media, ContentRenderingError>;
}

#[derive(Clone, Serialize)]
struct SolitonRenderData {
    version: SolitonVersion,
}

const SOURCE_MEDIA_TYPE_OF_PARENT_PROPERTY_NAME: &str = "source-media-type-of-parent";

#[derive(Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
struct RenderData {
    soliton: SolitonRenderData,
    content: ContentIndex,
    source_media_type_of_parent: Option<MediaType>, // Field name must align with SOURCE_MEDIA_TYPE_OF_PARENT_PROPERTY_NAME.
}

pub struct RenderContext<'engine, E: ContentEngine> {
    content_engine: &'engine E,
    data: RenderData,
}
