use super::{RenderData, UnregisteredTemplateParseError};
use crate::lib::*;
use handlebars::{Context, Handlebars, RenderContext, Renderable};
use std::fs;
use std::io;
use std::io::{Read, Seek, SeekFrom};
use std::rc::Rc;
use thiserror::Error;

// ContentItem data is stored in separate structs rather than inline in the
// enum in order to keep internals private and make it impossible to
// construct from the outside. We want to enforce some additional
// invariants via smart constructors; e.g. RegisteredTemplate cannot fail
// rendering because the template was not found in the registry.

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

// This must only be constructed with template names that have already been
// validated to exist in the registry.
pub struct RegisteredTemplate<'a> {
    handlebars_registry: Rc<Handlebars<'a>>,
    name_in_registry: String,
}
impl<'a> RegisteredTemplate<'a> {
    pub fn from_registry(
        handlebars_registry: Rc<Handlebars<'a>>,
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

pub struct UnregisteredTemplate<'a> {
    handlebars_registry: &'a Handlebars<'a>,
    template: handlebars::Template,
}
impl<'a> UnregisteredTemplate<'a> {
    pub fn from_source(
        handlebars_registry: &'a Handlebars<'a>,
        handlebars_source: &str,
    ) -> Result<Self, UnregisteredTemplateParseError> {
        let template = handlebars::Template::compile2(handlebars_source, true)?;
        Ok(UnregisteredTemplate {
            handlebars_registry,
            template,
        })
    }
}

pub enum ContentItem<'a> {
    /// A static (non-template) file.
    StaticContentItem(StaticContentItem),

    /// A named template that exists in the registry.
    RegisteredTemplate(RegisteredTemplate<'a>),

    /// An anonymous template for on-the-fly rendering.
    UnregisteredTemplate(UnregisteredTemplate<'a>),
}

impl<'a> Render<'a> for ContentItem<'_> {
    type RenderArgs = RenderData;
    type Error = ContentRenderingError;

    fn render(&self, render_data: &RenderData) -> Result<String, Self::Error> {
        let rendered_content = match &self {
            ContentItem::StaticContentItem(StaticContentItem { contents }) => {
                // We clone the file handle and operate on that to avoid taking
                // self as mut. Note that all clones share a cursor, so seeking
                // back to the beginning is necessary to ensure we read the
                // entire file.
                let mut readable_contents = contents.try_clone()?;
                let mut output = String::new();
                readable_contents.seek(SeekFrom::Start(0))?;
                readable_contents.read_to_string(&mut output)?;
                output
            }

            ContentItem::RegisteredTemplate(RegisteredTemplate {
                handlebars_registry,
                name_in_registry,
            }) => handlebars_registry.render(name_in_registry, &render_data)?,

            ContentItem::UnregisteredTemplate(UnregisteredTemplate {
                handlebars_registry,
                template,
            }) => {
                let context = Context::wraps(render_data)?;
                let mut render_context = RenderContext::new(None);
                template.renders(handlebars_registry, &context, &mut render_context)?
            }
        };

        Ok(rendered_content)
    }
}
