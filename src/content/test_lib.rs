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
                target_media_type: None,
            },
        }
    }
    fn new_template(
        &self,
        handlebars_source: &str,
        media_type: MediaType,
    ) -> Result<UnregisteredTemplate, UnregisteredTemplateParseError> {
        UnregisteredTemplate::from_source(handlebars_source, media_type)
    }
    fn get(&self, _: &str) -> Option<&RegisteredContent> {
        None
    }
    fn handlebars_registry(&self) -> &Handlebars {
        &self.0
    }
}

pub fn media_to_string(media: &mut Media<impl Read>) -> String {
    let mut string = String::new();
    media
        .content
        .read_to_string(&mut string)
        .expect("Failed to read media into a string");
    string
}

pub fn media_to_bytes(media: &mut Media<impl Read>) -> Vec<u8> {
    let mut bytes = Vec::new();
    media
        .content
        .read_to_end(&mut bytes)
        .expect("Failed to read media");
    bytes
}
