mod content_engine;
mod content_index;
mod content_item;
mod handlebars_helpers;

use crate::lib::*;
use content_index::*;
use serde::Serialize;

pub use content_engine::{
    ContentEngine, ContentLoadingError, RegisteredTemplateParseError,
    UnregisteredTemplateParseError,
};
pub use content_item::ContentRenderingError;

const HANDLEBARS_FILE_EXTENSION: &str = "hbs";
const HTML_FILE_EXTENSION: &str = "html";

#[derive(Serialize)]
struct SolitonRenderData {
    version: SolitonVersion,
}

#[derive(Serialize)]
pub struct RenderData {
    soliton: SolitonRenderData,
    content: ContentIndex,
}

pub struct RenderContext<'a> {
    engine: &'a ContentEngine<'a>,
    data: RenderData,
}
