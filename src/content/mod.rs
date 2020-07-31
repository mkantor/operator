mod content_engine;
mod content_index;
mod content_item;
mod handlebars_helpers;
mod serializable_media_range;
mod test_lib;

use crate::lib::*;
use content_index::*;
use mime::Mime;
use serde::Serialize;
use serializable_media_range::SerializableMediaRange;

pub use content_engine::{
    ContentEngine, ContentLoadingError, FilesystemBasedContentEngine, RegisteredContent,
    RegisteredTemplateParseError, UnregisteredTemplateParseError,
};
pub use content_item::{ContentRenderingError, UnregisteredTemplate};

const HANDLEBARS_FILE_EXTENSION: &str = "hbs";

pub trait Render {
    fn render<'engine, 'data, E: ContentEngine>(
        &self,
        context: RenderContext<'engine, 'data, E>,
        acceptable_media_ranges: &[Mime],
    ) -> Result<String, ContentRenderingError>;
}

#[derive(Clone, Serialize)]
struct SolitonRenderData {
    version: SolitonVersion,
}

const SOURCE_MEDIA_TYPE_OF_PARENT_PROPERTY_NAME: &str = "source-media-type-of-parent";

#[derive(Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
struct RenderData<'a> {
    soliton: SolitonRenderData,
    content: ContentIndex,
    source_media_type_of_parent: Option<SerializableMediaRange<'a>>, // Field name must align with SOURCE_MEDIA_TYPE_OF_PARENT_PROPERTY_NAME.
}

pub struct RenderContext<'engine, 'data, E: ContentEngine> {
    content_engine: &'engine E,
    data: RenderData<'data>,
}
