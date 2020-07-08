use crate::content::*;
use handlebars::{self, Handlebars};
use std::sync::{Arc, RwLock};

pub struct GetHelper<'a> {
    content_engine: Arc<RwLock<ContentEngine<'a>>>,
}
impl<'a> GetHelper<'a> {
    pub fn new(content_engine: Arc<RwLock<ContentEngine<'a>>>) -> Self {
        Self { content_engine }
    }
}

impl<'a> handlebars::HelperDef for GetHelper<'a> {
    fn call<'registry: 'context, 'context>(
        &self,
        helper: &handlebars::Helper<'registry, 'context>,
        _: &'registry Handlebars<'registry>,
        _: &'context handlebars::Context,
        _: &mut handlebars::RenderContext<'registry, 'context>,
        output: &mut dyn handlebars::Output,
    ) -> handlebars::HelperResult {
        let engine = self
            .content_engine
            .read()
            .expect("RwLock for ContentEngine has been poisoned");

        let address = helper
            .param(0)
            .ok_or_else(|| handlebars::RenderError::new(
                "The `get` helper requires an argument (the address of the content item to get).",
            ))?
            .value()
            .as_str()
            .ok_or_else(|| handlebars::RenderError::new(
                "The `get` helper's first argument must be a string (the address of the content item to get).",
            ))?;

        let content_item = engine.get(&address).ok_or_else(|| {
            handlebars::RenderError::new(format!(
                "No content found at address passed to `get` helper (\"{}\").",
                address
            ))
        })?;
        let context = engine.get_render_context();

        output.write(content_item.render(&context).unwrap().as_ref())?;
        Ok(())
    }
}
