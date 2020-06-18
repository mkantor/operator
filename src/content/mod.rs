mod content_engine;
mod content_index;
mod content_item;

use crate::lib::*;
use content_index::*;
use serde::Serialize;

pub use content_engine::{
    ContentEngine, ContentLoadingError, RegisteredTemplateParseError, TemplateRenderError,
    UnregisteredTemplateParseError,
};

const HANDLEBARS_FILE_EXTENSION: &str = ".hbs";

#[derive(Serialize)]
struct GluonRenderData {
    version: GluonVersion,
}

#[derive(Serialize)]
pub struct RenderData {
    gluon: GluonRenderData,
    content: ContentIndex,
}
