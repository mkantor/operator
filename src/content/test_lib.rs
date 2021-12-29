#![cfg(test)]

use std::collections::HashMap;

use super::content_index::ContentIndexEntries;
use super::*;
use bytes::{Bytes, BytesMut};
use futures::executor;
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
impl<'a> ContentEngine<()> for MockContentEngine<'a> {
    fn render_context(
        &self,
        route: Option<Route>,
        query_parameters: HashMap<String, String>,
        request_headers: HashMap<String, String>,
    ) -> RenderContext<(), Self> {
        RenderContext {
            content_engine: self,
            handlebars_render_context: None,
            data: RenderData {
                server_info: (),
                index: ContentIndex::Directory(ContentIndexEntries::new()),
                target_media_type: None,
                error_code: None,
                request: RequestData {
                    route,
                    query_parameters,
                    request_headers,
                },
            },
        }
    }
    fn new_template(
        &self,
        handlebars_source: &str,
        media_type: MediaType,
    ) -> Result<UnregisteredTemplate, TemplateError> {
        UnregisteredTemplate::from_source(handlebars_source, media_type)
    }
    fn get(&self, _: &Route) -> Option<&ContentRepresentations> {
        None
    }
    fn handlebars_registry(&self) -> &Handlebars {
        &self.0
    }
}

pub fn media_to_string(media: Media<impl ByteStream + Unpin>) -> String {
    let bytes = block_on_content(media).expect("There was an error in the content stream");
    String::from_utf8(bytes.into_iter().collect()).expect("Failed to read media into a string")
}

pub fn block_on_content(media: Media<impl ByteStream + Unpin>) -> Result<Bytes, StreamError> {
    let mut all_bytes = BytesMut::new();
    for result in executor::block_on_stream(media.content) {
        match result {
            Ok(bytes) => all_bytes.extend_from_slice(&bytes),
            error => return error,
        }
    }
    Ok(all_bytes.freeze())
}
