#![cfg(test)]

use crate::content::*;
use handlebars::Handlebars;
use mime::Mime;

pub struct MockContentEngine;
impl ContentEngine for MockContentEngine {
    fn get_render_context<'a, 'b>(&'a self, media_type: &'b Mime) -> RenderContext<'a, 'b> {
        RenderContext {
            engine: self,
            data: RenderData {
                soliton: SolitonRenderData {
                    version: SolitonVersion("0.0.0"),
                },
                content: ContentIndex::Directory(ContentIndexEntries::new()),
                target_media_type: SerializableMediaType { media_type },
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
