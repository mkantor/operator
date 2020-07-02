use super::{RenderData, UnregisteredTemplateParseError};
use crate::lib::*;
use handlebars::{Context, Handlebars, RenderContext, Renderable};
use std::fs;
use std::io;
use std::io::{Read, Seek, SeekFrom};
use std::rc::Rc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContentRenderingError {
    #[error(
        "Rendering failed for template{}.",
        .source.template_name.as_ref().map(|known_name| format!(" '{}'", known_name)).unwrap_or_default(),
    )]
    TemplateRenderError {
        #[from]
        source: handlebars::RenderError,
    },

    #[error("Input/output error during rendering.")]
    IOError {
        #[from]
        source: io::Error,
    },
}

pub struct StaticContentItem {
    contents: fs::File,
}
impl StaticContentItem {
    pub fn new(contents: fs::File) -> Self {
        StaticContentItem { contents }
    }
}
impl Render for StaticContentItem {
    type RenderArgs = RenderData;
    type Error = ContentRenderingError;
    fn render(&self, _: &RenderData) -> Result<String, Self::Error> {
        // We clone the file handle and operate on that to avoid taking
        // self as mut. Note that all clones share a cursor, so seeking
        // back to the beginning is necessary to ensure we read the
        // entire file.
        let mut readable_contents = self.contents.try_clone()?;
        let mut rendered_content = String::new();
        readable_contents.seek(SeekFrom::Start(0))?;
        readable_contents.read_to_string(&mut rendered_content)?;
        Ok(rendered_content)
    }
}

// This must only be constructed with template names that have already been
// validated to exist in the registry.
pub struct RegisteredTemplate<'registry> {
    handlebars_registry: Rc<Handlebars<'registry>>,
    name_in_registry: String,
}
impl<'registry> RegisteredTemplate<'registry> {
    pub fn from_registry(
        handlebars_registry: Rc<Handlebars<'registry>>,
        name_in_registry: String,
    ) -> Option<Self> {
        if handlebars_registry.has_template(&name_in_registry) {
            Some(RegisteredTemplate {
                handlebars_registry,
                name_in_registry,
            })
        } else {
            None
        }
    }
}
impl<'registry> Render for RegisteredTemplate<'registry> {
    type RenderArgs = RenderData;
    type Error = ContentRenderingError;
    fn render(&self, render_data: &RenderData) -> Result<String, Self::Error> {
        self.handlebars_registry
            .render(&self.name_in_registry, &render_data)
            .map_err(ContentRenderingError::from)
    }
}

pub struct UnregisteredTemplate<'registry> {
    handlebars_registry: &'registry Handlebars<'registry>,
    template: handlebars::Template,
}
impl<'registry> UnregisteredTemplate<'registry> {
    pub fn from_source<S: AsRef<str> + 'registry>(
        handlebars_registry: &'registry Handlebars<'registry>,
        handlebars_source: S,
    ) -> Result<Self, UnregisteredTemplateParseError> {
        let template = handlebars::Template::compile2(handlebars_source, true)?;
        Ok(UnregisteredTemplate {
            handlebars_registry,
            template,
        })
    }
}
impl<'registry> Render for UnregisteredTemplate<'registry> {
    type RenderArgs = RenderData;
    type Error = ContentRenderingError;

    fn render(&self, render_data: &RenderData) -> Result<String, Self::Error> {
        let context = Context::wraps(render_data)?;
        let mut render_context = RenderContext::new(None);
        self.template
            .renders(self.handlebars_registry, &context, &mut render_context)
            .map_err(ContentRenderingError::from)
    }
}
