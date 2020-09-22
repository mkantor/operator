use crate::content::*;
use futures::executor;
use futures::stream::TryStreamExt;
use handlebars::{self, Handlebars};
use std::marker::PhantomData;
use std::sync::{Arc, RwLock};

pub struct GetHelper<ServerInfo, Engine>
where
    ServerInfo: Clone + Serialize,
    Engine: ContentEngine<ServerInfo>,
{
    content_engine: Arc<RwLock<Engine>>,
    server_info_type: PhantomData<ServerInfo>,
}
impl<ServerInfo, Engine> GetHelper<ServerInfo, Engine>
where
    ServerInfo: Clone + Serialize,
    Engine: ContentEngine<ServerInfo>,
{
    pub fn new(content_engine: Arc<RwLock<Engine>>) -> Self {
        Self {
            content_engine,
            server_info_type: PhantomData,
        }
    }
}

impl<ServerInfo, Engine> handlebars::HelperDef for GetHelper<ServerInfo, Engine>
where
    ServerInfo: Clone + Serialize,
    Engine: ContentEngine<ServerInfo>,
{
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

        let path_and_json = helper
            .param(0)
            .ok_or_else(|| {
                handlebars::RenderError::new(
                    "The `get` helper requires an argument (the route of the content item to get).",
                )
            })?
            .value();
        let route = path_and_json
            .as_str()
            .ok_or_else(|| {
                handlebars::RenderError::new(format!(
                    "The `get` helper's first argument must be a string (the route of the content \
                    item to get), but it was `{}`.",
                    path_and_json,
                ))
            })?
            .parse::<Route>()
            .map_err(|error| {
                handlebars::RenderError::new(format!(
                    "The `get` helper's first argument (`{}`) must be a valid route: {}",
                    path_and_json, error,
                ))
            })?;

        let content_item = content_engine.get(&route).ok_or_else(|| {
            handlebars::RenderError::new(format!(
                "No content found at route passed to `get` helper (\"{}\").",
                route,
            ))
        })?;

        let current_render_data = handlebars_context.data().as_object().ok_or_else(|| {
            handlebars::RenderError::new(format!(
                "The `get` helper call failed because the context JSON was not an object. It is `{}`.",
                handlebars_context.data(),
            ))
        })?;

        let target_media_type = current_render_data.get(TARGET_MEDIA_TYPE_PROPERTY_NAME)
            .and_then(|value| value.as_str())
            .and_then(|media_type_essence| media_type_essence.parse::<MediaType>().ok())
            .ok_or_else(|| {
                handlebars::RenderError::new(format!(
                    "The `get` helper call failed because a valid target media type could not be found \
                    in the handlebars context. The context JSON must contain a top-level property named \"{}\" \
                    whose value is a valid media type essence string. The current context is `{}`.",
                    TARGET_MEDIA_TYPE_PROPERTY_NAME,
                    handlebars_context.data(),
                ))
            })?;

        let optional_request_route = {
            let request_route_value = current_render_data.get(REQUEST_ROUTE_PROPERTY_NAME)
            .ok_or_else(|| {
                handlebars::RenderError::new(format!(
                    "The `get` helper call failed because the request route could not be found \
                    in the handlebars context. The context JSON must contain a top-level property named \"{}\" \
                    whose value is a string or null. The current context is `{}`.",
                    REQUEST_ROUTE_PROPERTY_NAME,
                    handlebars_context.data(),
                ))
            })?;

            if request_route_value.is_null() {
                None
            } else {
                let request_route = request_route_value.as_str()
                .ok_or_else(|| {
                    handlebars::RenderError::new(format!(
                        "The `get` helper call failed because the request route in the handlebars context was \
                        not a string or null (it was `{}`). The current context is `{}`.",
                        request_route_value,
                        handlebars_context.data(),
                    ))
                })?
                .parse::<Route>()
                .map_err(|error| {
                    handlebars::RenderError::new(format!(
                        "The `get` helper call failed because the request route in the handlebars context was invalid ({}). \
                        The current context is `{}`.",
                        error,
                        handlebars_context.data(),
                    ))
                })?;
                Some(request_route)
            }
        };

        let context = content_engine.get_render_context(optional_request_route);

        let rendered = content_item
            .render(context, &[target_media_type.into_media_range()]).map_err(|render_error| {
                handlebars::RenderError::new(format!(
                    "The `get` helper call failed because the content item being retrieved (\"{}\") \
                    could not be rendered: {}",
                    route,
                    render_error,
                ))
            })?;

        // Unfortunately handlebars-rust needs a string, so we block the thread
        // until the stream has been exhausted (or produces an error).
        let (size_lower_bound, _) = rendered.content.size_hint();
        let bytes = executor::block_on(rendered.content.try_fold(
            Vec::with_capacity(size_lower_bound),
            |mut all_bytes, additional_bytes| async {
                all_bytes.extend(additional_bytes);
                Ok(all_bytes)
            },
        ))
        .map_err(|streaming_error| {
            handlebars::RenderError::new(format!(
                "The `get` helper call failed because there was an error collecting the rendered content \
                for \"{}\": {}",
                route,
                streaming_error,
            ))
        })?;
        let rendered_content_as_string = String::from_utf8(bytes)?;

        output.write(&rendered_content_as_string)?;
        Ok(())
    }
}
