use super::{RenderContext, UnregisteredTemplateParseError};
use crate::lib::*;
use handlebars::{self, Renderable as _};
use mime::{self, Mime};
use std::fs;
use std::io;
use std::io::{Read, Seek, SeekFrom};
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

    #[error(
        "Unable to satisfy target media type '{}' from source media type '{}'.",
        .target_media_type,
        .source_media_type,
    )]
    MediaTypeError {
        source_media_type: Mime,
        target_media_type: Mime,
    },
}

pub struct StaticContentItem {
    contents: fs::File,
    media_type: Mime,
}
impl StaticContentItem {
    pub fn new(contents: fs::File, media_type: Mime) -> Self {
        StaticContentItem {
            contents,
            media_type,
        }
    }
}
impl Render for StaticContentItem {
    type RenderArgs = RenderContext<'static>;
    type Error = ContentRenderingError;

    fn render(&self, _: &RenderContext) -> Result<String, Self::Error> {
        // TODO: Pass target media type as a parameter to this method.
        let assumed_target_media_type = mime::TEXT_HTML;

        if assumed_target_media_type != self.media_type {
            Err(ContentRenderingError::MediaTypeError {
                source_media_type: self.media_type.clone(),
                target_media_type: assumed_target_media_type,
            })
        } else {
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
}

pub struct RegisteredTemplate {
    name_in_registry: String,
}
impl RegisteredTemplate {
    pub fn new<S: AsRef<str>>(name_in_registry: S) -> Self {
        RegisteredTemplate {
            name_in_registry: String::from(name_in_registry.as_ref()),
        }
    }
}
impl Render for RegisteredTemplate {
    type RenderArgs = RenderContext<'static>;
    type Error = ContentRenderingError;

    fn render(&self, context: &RenderContext) -> Result<String, Self::Error> {
        context
            .engine
            .handlebars_registry
            .render(&self.name_in_registry, &context.data)
            .map_err(ContentRenderingError::from)
    }
}

pub struct UnregisteredTemplate {
    template: handlebars::Template,
}
impl UnregisteredTemplate {
    pub fn from_source<S: AsRef<str>>(
        handlebars_source: S,
    ) -> Result<Self, UnregisteredTemplateParseError> {
        let template = handlebars::Template::compile2(handlebars_source, true)?;
        Ok(UnregisteredTemplate { template })
    }
}
impl Render for UnregisteredTemplate {
    type RenderArgs = RenderContext<'static>;
    type Error = ContentRenderingError;

    fn render(&self, context: &RenderContext) -> Result<String, Self::Error> {
        let handlebars_context = handlebars::Context::wraps(&context.data)?;
        let mut handlebars_render_context = handlebars::RenderContext::new(None);
        self.template
            .renders(
                &context.engine.handlebars_registry,
                &handlebars_context,
                &mut handlebars_render_context,
            )
            .map_err(ContentRenderingError::from)
    }
}
