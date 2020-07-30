use crate::content::*;
use handlebars::{self, Handlebars};
use std::sync::{Arc, RwLock};

pub struct GetHelper<E: ContentEngine> {
    content_engine: Arc<RwLock<E>>,
}
impl<E: ContentEngine> GetHelper<E> {
    pub fn new(content_engine: Arc<RwLock<E>>) -> Self {
        Self { content_engine }
    }
}

impl<E: ContentEngine> handlebars::HelperDef for GetHelper<E> {
    fn call<'registry: 'context, 'context>(
        &self,
        helper: &handlebars::Helper<'registry, 'context>,
        _: &'registry Handlebars<'registry>,
        handlebars_context: &'context handlebars::Context,
        _: &mut handlebars::RenderContext<'registry, 'context>,
        output: &mut dyn handlebars::Output,
    ) -> handlebars::HelperResult {
        let content_engine = self
            .content_engine
            .read()
            .expect("RwLock for ContentEngine has been poisoned");

        let route = helper
            .param(0)
            .ok_or_else(|| handlebars::RenderError::new(
                "The `get` helper requires an argument (the route of the content item to get).",
            ))?
            .value()
            .as_str()
            .ok_or_else(|| handlebars::RenderError::new(
                "The `get` helper's first argument must be a string (the route of the content item to get).",
            ))?;

        let content_item = content_engine.get(&route).ok_or_else(|| {
            handlebars::RenderError::new(format!(
                "No content found at route passed to `get` helper (\"{}\").",
                route
            ))
        })?;

        // FIXME: This works for now, but only because of other assumptions. It
        // should really be setting the target media type to the *source* media
        // type of the calling template. This doesn't matter yet because they
        // are guaranteed to be identical (otherwise rendering fails), but if
        // soliton eventually supports transcoding between different media
        // types it won't always be.
        let target_media_type = handlebars_context.data().as_object()
            .and_then(|object| object.get(TARGET_MEDIA_TYPE_PROPERTY_NAME))
            .and_then(|value| value.as_str())
            .and_then(|media_type_essence| media_type_essence.parse::<Mime>().ok())
            .ok_or_else(|| {
                handlebars::RenderError::new(format!(
                    "The `get` helper call failed because a valid target media type could not be found in the handlebars context. The context JSON must contain a top-level property named \"{}\" whose value is a valid media type essence string. The current context is `{}`.",
                    TARGET_MEDIA_TYPE_PROPERTY_NAME,
                    handlebars_context.data()
                ))
            })?;

        let context = content_engine.get_render_context(&target_media_type);

        let rendered_content = content_item
            .render(context).map_err(|soliton_render_error| {
                handlebars::RenderError::new(format!(
                    "The `get` helper call failed because the content item being retrieved (\"{}\") could not be rendered: {}",
                    route,
                    soliton_render_error
                ))
            })?;

        output.write(rendered_content.as_ref())?;
        Ok(())
    }
}
