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
    ContentEngine, ContentLoadingError, FilesystemBasedContentEngine, RegisteredTemplateParseError,
    UnregisteredTemplateParseError,
};
pub use content_item::ContentRenderingError;

const HANDLEBARS_FILE_EXTENSION: &str = "hbs";

pub trait Render {
    fn render<'engine, 'data>(
        &self,
        context: RenderContext<'engine, 'data>,
        target_media_type: &Mime,
    ) -> Result<String, ContentRenderingError>;
}

#[derive(Serialize)]
struct SolitonRenderData {
    version: SolitonVersion,
}

const SOURCE_MEDIA_TYPE_OF_PARENT_PROPERTY_NAME: &str = "source-media-type-of-parent";

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct RenderData<'a> {
    soliton: SolitonRenderData,
    content: ContentIndex,
    source_media_type_of_parent: Option<SerializableMediaRange<'a>>, // Field name must align with SOURCE_MEDIA_TYPE_OF_PARENT_PROPERTY_NAME.
}

pub struct RenderContext<'engine, 'data> {
    content_engine: &'engine dyn ContentEngine,
    data: RenderData<'data>,
}
