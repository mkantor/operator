#![cfg(test)]

use crate::content::*;
use handlebars::Handlebars;
use mime::Mime;

pub struct MockContentEngine;
impl ContentEngine for MockContentEngine {
    fn get_render_context(&self) -> RenderContext {
        RenderContext {
            content_engine: self,
            data: RenderData {
                soliton: SolitonRenderData {
                    version: SolitonVersion("0.0.0"),
                },
                content: ContentIndex::Directory(ContentIndexEntries::new()),
                source_media_type_of_parent: None,
            },
        }
    }
    fn new_template(
        &self,
        _: &str,
        _: Mime,
    ) -> Result<Box<dyn Render>, UnregisteredTemplateParseError> {
        unimplemented!()
    }
    fn get(&self, _: &str) -> Option<&dyn Render> {
        unimplemented!()
    }
    fn handlebars_registry(&self) -> &Handlebars {
        unimplemented!()
    }
}
pub const MOCK_CONTENT_ENGINE: MockContentEngine = MockContentEngine;
