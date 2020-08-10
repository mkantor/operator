use crate::content::*;
use handlebars::{self, Handlebars};
use std::marker::PhantomData;
use std::sync::{Arc, RwLock};

pub struct GetHelper<ServerInfo, ErrorCode, Engine>
where
    ServerInfo: Clone + Serialize,
    ErrorCode: Clone + Serialize,
    Engine: ContentEngine<ServerInfo, ErrorCode>,
{
    content_engine: Arc<RwLock<Engine>>,
    server_info_type: PhantomData<ServerInfo>,
    error_code_type: PhantomData<ErrorCode>,
}
impl<ServerInfo, ErrorCode, Engine> GetHelper<ServerInfo, ErrorCode, Engine>
where
    ServerInfo: Clone + Serialize,
    ErrorCode: Clone + Serialize,
    Engine: ContentEngine<ServerInfo, ErrorCode>,
{
    pub fn new(content_engine: Arc<RwLock<Engine>>) -> Self {
        Self {
            content_engine,
            server_info_type: PhantomData,
            error_code_type: PhantomData,
        }
    }
}

impl<ServerInfo, ErrorCode, Engine> handlebars::HelperDef
    for GetHelper<ServerInfo, ErrorCode, Engine>
where
    ServerInfo: Clone + Serialize,
    ErrorCode: Clone + Serialize,
    Engine: ContentEngine<ServerInfo, ErrorCode>,
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
        let route = path_and_json.as_str().ok_or_else(|| {
            handlebars::RenderError::new(format!(
                "The `get` helper's first argument must be a string (the route of the content \
                    item to get), but it was `{}`.",
                path_and_json,
            ))
        })?;

        let content_item = content_engine.get(&route).ok_or_else(|| {
            handlebars::RenderError::new(format!(
                "No content found at route passed to `get` helper (\"{}\").",
                route
            ))
        })?;

        let current_render_data = handlebars_context.data().as_object().ok_or_else(|| {
            handlebars::RenderError::new(format!(
                "The `get` helper call failed because the context JSON was not an object. It is `{}`.",
                handlebars_context.data()
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
                    handlebars_context.data()
                ))
            })?;

        let request_route = current_render_data.get(REQUEST_ROUTE_PROPERTY_NAME)
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                handlebars::RenderError::new(format!(
                    "The `get` helper call failed because the request route could not be found \
                    in the handlebars context. The context JSON must contain a top-level property named \"{}\" \
                    whose value is a string. The current context is `{}`.",
                    REQUEST_ROUTE_PROPERTY_NAME,
                    handlebars_context.data()
                ))
            })?;

        let context = content_engine.get_render_context(request_route);

        let mut rendered = content_item
            .render(context, &[target_media_type.into_media_range()]).map_err(|soliton_render_error| {
                handlebars::RenderError::new(format!(
                    "The `get` helper call failed because the content item being retrieved (\"{}\") \
                    could not be rendered: {}",
                    route,
                    soliton_render_error
                ))
            })?;

        let mut rendered_content_as_string = String::new();
        rendered
            .content
            .read_to_string(&mut rendered_content_as_string)?;
        output.write(&rendered_content_as_string)?;
        Ok(())
    }
}
