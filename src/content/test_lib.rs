#![cfg(test)]

use crate::content::*;
use handlebars::Handlebars;

pub struct MockContentEngine<'a>(Handlebars<'a>);
impl<'a> MockContentEngine<'a> {
    pub fn new() -> Self {
        Self(Handlebars::new())
    }
    pub fn register_template(
        &mut self,
        template_name: &str,
        template_contents: &str,
    ) -> Result<(), handlebars::TemplateError> {
        self.0
            .register_template_string(template_name, template_contents)
    }
}
impl<'a> ContentEngine for MockContentEngine<'a> {
    fn get_render_context(&self) -> RenderContext<Self> {
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
        _: MediaType,
    ) -> Result<UnregisteredTemplate, UnregisteredTemplateParseError> {
        unimplemented!("a")
    }
    fn get(&self, _: &str) -> Option<&RegisteredContent> {
        unimplemented!("b")
    }
    fn handlebars_registry(&self) -> &Handlebars {
        &self.0
    }
}
