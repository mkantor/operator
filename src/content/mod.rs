mod content_engine;
mod content_index;
mod content_item;
mod handlebars_helpers;

use crate::lib::*;
use content_index::*;
use mime::Mime;
use serde::{Serialize, Serializer};

pub use content_engine::{
    ContentEngine, ContentLoadingError, RegisteredTemplateParseError,
    UnregisteredTemplateParseError,
};
pub use content_item::ContentRenderingError;

const HANDLEBARS_FILE_EXTENSION: &str = "hbs";

#[derive(Serialize)]
struct SolitonRenderData {
    version: SolitonVersion,
}

#[derive(PartialEq, Eq)]
struct SerializableMediaType<'a> {
    media_type: &'a Mime,
}
impl<'a> Serialize for SerializableMediaType<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.media_type.essence_str())
    }
}
impl<'a> PartialEq<Mime> for SerializableMediaType<'a> {
    fn eq(&self, other: &Mime) -> bool {
        self.media_type == other
    }
}
impl<'a> PartialEq<SerializableMediaType<'a>> for Mime {
    fn eq(&self, other: &SerializableMediaType) -> bool {
        self == other.media_type
    }
}

const TARGET_MEDIA_TYPE_PROPERTY_NAME: &str = "target-media-type";

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct RenderData<'a> {
    soliton: SolitonRenderData,
    content: ContentIndex,
    target_media_type: SerializableMediaType<'a>, // Field name must align with TARGET_MEDIA_TYPE_PROPERTY_NAME.
}

pub struct RenderContext<'a, 'b> {
    engine: &'a ContentEngine<'a>,
    data: RenderData<'b>,
}
