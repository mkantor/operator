use crate::content::content_engine::InternalContentEngine;
use crate::content::*;
use futures::executor;
use futures::stream::TryStreamExt;
use handlebars::{self, Handlebars};
use std::collections::{BTreeMap, HashMap};
use std::marker::PhantomData;
use std::mem;
use std::rc::Rc;
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
    Engine: ContentEngine<ServerInfo> + InternalContentEngine,
{
    fn call<'registry: 'context, 'context>(
        &self,
        helper: &handlebars::Helper<'registry, 'context>,
        _: &'registry Handlebars<'registry>,
        handlebars_context: &'context handlebars::Context,
        handlebars_render_context: &mut handlebars::RenderContext<'registry, 'context>,
        output: &mut dyn handlebars::Output,
    ) -> handlebars::HelperResult {
        let content_engine = self
            .content_engine
            .read()
            .expect("RwLock for ContentEngine has been poisoned");

        let param_0 = helper
            .param(0)
            .ok_or_else(|| {
                handlebars::RenderError::new(
                    "The `get` helper requires an argument (the route of the content item to get).",
                )
            })?
            .value();
        let route = param_0
            .as_str()
            .ok_or_else(|| {
                handlebars::RenderError::new(format!(
                    "The `get` helper's first argument must be a string (the route of the content \
                    item to get), but it was `{}`.",
                    param_0,
                ))
            })?
            .parse::<Route>()
            .map_err(|error| {
                handlebars::RenderError::new(format!(
                    "The `get` helper's first argument (`{}`) must be a valid route: {}",
                    param_0, error,
                ))
            })?;

        let custom_context = helper
            .hash()
            .iter()
            .map(|(key, value)| (*key, value.value()))
            .collect::<BTreeMap<&str, &serde_json::Value>>();

        let content_item = content_engine.get_internal(&route).ok_or_else(|| {
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

        let mut modified_context_data_as_json_map = match handlebars_render_context.context() {
            Some(ref mut modified_context) => {
                let modified_context_data_as_json =
                    mem::take(Rc::make_mut(modified_context).data_mut());
                match modified_context_data_as_json {
                    serde_json::Value::Object(modified_context_data_as_json_map) => modified_context_data_as_json_map,
                    _ => {
                        return Err(handlebars::RenderError::new(format!(
                            "The `get` helper call failed because the pre-existing handlebars render context was \
                            not an object (it was `{}`).",
                            modified_context_data_as_json
                        )))
                    }
                }
            }
            None => serde_json::Map::default(),
        };

        // Merge render data and custom context atop the existing RenderContext data.
        for (key, value) in current_render_data {
            modified_context_data_as_json_map.insert(key.to_string(), value.clone());
        }
        for (key, value) in custom_context {
            modified_context_data_as_json_map.insert(key.to_string(), value.clone());
        }
        handlebars_render_context.set_context(handlebars::Context::wraps(
            serde_json::Value::Object(modified_context_data_as_json_map),
        )?);

        let target_media_type = get_target_media_type(current_render_data)?;
        let optional_request_route = get_optional_request_route(current_render_data)?;
        let query_parameters = get_query_parameters(current_render_data)?;
        let request_headers = get_request_headers(current_render_data)?;

        let context = content_engine
            .render_context(optional_request_route, query_parameters, request_headers)
            .with_handlebars_render_context(handlebars_render_context.clone());

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

fn get_target_media_type(
    render_data: &serde_json::value::Map<String, serde_json::Value>,
) -> Result<MediaType, handlebars::RenderError> {
    let target_media_type = render_data
        .get(TARGET_MEDIA_TYPE_PROPERTY_NAME)
        .and_then(|value| value.as_str())
        .and_then(|media_type_essence| media_type_essence.parse::<MediaType>().ok())
        .ok_or_else(|| {
            handlebars::RenderError::new(format!(
                "The `get` helper call failed because a valid target media type could not be found \
                in the handlebars context. The context JSON must contain a property at `{}` \
                whose value is a valid media type essence string.",
                TARGET_MEDIA_TYPE_PROPERTY_NAME,
            ))
        })?;
    Ok(target_media_type)
}

fn get_optional_request_route(
    render_data: &serde_json::value::Map<String, serde_json::Value>,
) -> Result<Option<Route>, handlebars::RenderError> {
    let optional_request_route = {
        let request_route_value = render_data
            .get(REQUEST_DATA_PROPERTY_NAME)
            .and_then(|request_data| request_data.get(ROUTE_PROPERTY_NAME))
            .ok_or_else(|| {
                handlebars::RenderError::new(format!(
                    "The `get` helper call failed because the request route could not be found \
                    in the handlebars context. The context JSON must contain a property at `{}.{}` \
                    whose value is a string or null.",
                    REQUEST_DATA_PROPERTY_NAME, ROUTE_PROPERTY_NAME
                ))
            })?;

        if request_route_value.is_null() {
            None
        } else {
            let request_route = request_route_value.as_str()
        .ok_or_else(|| {
            handlebars::RenderError::new(format!(
                "The `get` helper call failed because the request route in the handlebars context was \
                not a string or null (it was `{}`).",
                request_route_value,
            ))
        })?
        .parse::<Route>()
        .map_err(|error| {
            handlebars::RenderError::new(format!(
                "The `get` helper call failed because the request route in the handlebars context was invalid ({}).",
                error,
            ))
        })?;
            Some(request_route)
        }
    };
    Ok(optional_request_route)
}

fn get_query_parameters(
    render_data: &serde_json::value::Map<String, serde_json::Value>,
) -> Result<HashMap<String, String>, handlebars::RenderError> {
    let query_parameters = render_data
        .get(REQUEST_DATA_PROPERTY_NAME)
        .and_then(|request_data| request_data.get(QUERY_PARAMETERS_PROPERTY_NAME))
        .ok_or_else(|| {
            handlebars::RenderError::new(format!(
                "The `get` helper call failed because the query parameters could not be found \
                in the handlebars context. The context JSON must contain a property at `{}.{}` \
                whose value is a map.",
                REQUEST_DATA_PROPERTY_NAME,
                QUERY_PARAMETERS_PROPERTY_NAME,
            ))
        })?
        .as_object()
        .ok_or_else(|| {
            handlebars::RenderError::new(String::from(
                "The `get` helper call failed because the query parameters in the handlebars context \
                was not a map."
            ))
        })?
        .into_iter()
        .flat_map(|(key, value)| {
            value
                .as_str()
                .map(|value| (key.clone(), String::from(value)))
        })
        .collect::<HashMap<String, String>>();
    Ok(query_parameters)
}

fn get_request_headers(
    render_data: &serde_json::value::Map<String, serde_json::Value>,
) -> Result<HashMap<String, String>, handlebars::RenderError> {
    let request_headers = render_data
            .get(REQUEST_DATA_PROPERTY_NAME)
            .and_then(|request_data| request_data.get(REQUEST_HEADERS_PROPERTY_NAME))
            .ok_or_else(|| {
                handlebars::RenderError::new(format!(
                    "The `get` helper call failed because the request headers could not be found \
                    in the handlebars context. The context JSON must contain a property at `{}.{}` \
                    whose value is a map.",
                    REQUEST_DATA_PROPERTY_NAME,
                    REQUEST_HEADERS_PROPERTY_NAME
                ))
            })?.as_object().ok_or_else(|| {
                handlebars::RenderError::new(String::from(
                    "The `get` helper call failed because the request headers in the handlebars context \
                    was not a map."
                ))
            })?
            .into_iter()
            .flat_map(|(key, value)| {
                value.as_str().map(|value| (key.clone(), String::from(value)))
            })
            .collect::<HashMap<String, String>>();
    Ok(request_headers)
}
