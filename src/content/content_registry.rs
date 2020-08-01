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
    fn render<'a, E: ContentEngine, A: IntoIterator<Item = &'a MediaRange>>(
        &self,
        context: RenderContext<E>,
        acceptable_media_ranges: A,
    ) -> Result<Media, ContentRenderingError> {
        match self {
            Self::StaticContentItem(renderable) => {
                renderable.render(context, acceptable_media_ranges)
            }
            Self::RegisteredTemplate(renderable) => {
                renderable.render(context, acceptable_media_ranges)
            }
            Self::Executable(renderable) => renderable.render(context, acceptable_media_ranges),
        }
    }
}

pub type ContentRegistry = HashMap<CanonicalRoute, RegisteredContent>;
