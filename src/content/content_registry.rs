use super::content_engine::CanonicalRoute;
use super::content_item::*;
use super::*;
use std::collections::HashMap;

/// A renderable item from the content directory.
pub enum RegisteredContent {
    StaticContentItem(StaticContentItem),
    RegisteredTemplate(RegisteredTemplate),
    Executable(Executable),
}
impl Render for RegisteredContent {
    type Output = Box<dyn Read>;
    fn render<'accept, ServerInfo, Engine, Accept>(
        &self,
        context: RenderContext<ServerInfo, Engine>,
        acceptable_media_ranges: Accept,
    ) -> Result<Media<Self::Output>, ContentRenderingError>
    where
        ServerInfo: Clone + Serialize,
        Engine: ContentEngine<ServerInfo>,
        Accept: IntoIterator<Item = &'accept MediaRange>,
        Self::Output: Read,
    {
        match self {
            Self::StaticContentItem(renderable) => renderable
                .render(context, acceptable_media_ranges)
                .map(box_media),
            Self::RegisteredTemplate(renderable) => renderable
                .render(context, acceptable_media_ranges)
                .map(box_media),
            Self::Executable(renderable) => renderable
                .render(context, acceptable_media_ranges)
                .map(box_media),
        }
    }
}

pub type ContentRegistry = HashMap<CanonicalRoute, RegisteredContent>;

fn box_media<'o, O: Read + 'o>(media: Media<O>) -> Media<Box<dyn Read + 'o>> {
    Media {
        content: Box::new(media.content),
        media_type: media.media_type,
    }
}
