use super::{TemplateRenderError, UnregisteredTemplateParseError};
use crate::lib::*;
use handlebars::{Context, Handlebars, RenderContext, Renderable};
use serde::Serialize;

// ContentItem data is stored in separate structs rather than inline in the
// enum in order to keep internals private and make it impossible to
// construct from the outside. We want to enforce some additional
// invariants via smart constructors; e.g. RegisteredTemplate cannot fail
// rendering because the template was not found in the registry.

// This must only be constructed with template names that have already been
// validated to exist in the registry.
pub struct RegisteredTemplate<'a> {
    template_registry: &'a Handlebars<'a>,
    name_in_registry: &'a str,
}

pub struct UnregisteredTemplate<'a> {
    template_registry: &'a Handlebars<'a>,
    template: handlebars::Template,
}

pub enum ContentItem<'a> {
    /// A named template that exists in the registry.
    RegisteredTemplate(RegisteredTemplate<'a>),

    /// An anonymous template for on-the-fly rendering.
    UnregisteredTemplate(UnregisteredTemplate<'a>),
}

impl<'a> ContentItem<'a> {
    pub fn new_template(
        template_registry: &'a Handlebars<'a>,
        handlebars_source: &str,
    ) -> Result<Self, UnregisteredTemplateParseError> {
        let template = handlebars::Template::compile2(handlebars_source, true)
            .map_err(|source| UnregisteredTemplateParseError { source })?;
        Ok(ContentItem::UnregisteredTemplate(UnregisteredTemplate {
            template_registry,
            template,
        }))
    }

    pub fn from_registry(
        template_registry: &'a Handlebars<'a>,
        name_in_registry: &'a str,
    ) -> Option<ContentItem<'a>> {
        if template_registry.has_template(name_in_registry) {
            Some(ContentItem::RegisteredTemplate(RegisteredTemplate {
                template_registry,
                name_in_registry,
            }))
        } else {
            None
        }
    }
}

#[derive(Serialize)]
struct GluonRenderData {
    version: GluonVersion,
}

#[derive(Serialize)]
struct RenderData {
    gluon: GluonRenderData,
}

impl Render for ContentItem<'_> {
    type RenderArgs = GluonVersion;
    type Error = TemplateRenderError;

    fn render(&self, gluon_version: GluonVersion) -> Result<String, Self::Error> {
        let render_data = RenderData {
            gluon: GluonRenderData {
                version: gluon_version,
            },
        };

        let rendered_content = match &self {
            ContentItem::RegisteredTemplate(RegisteredTemplate {
                template_registry,
                name_in_registry,
            }) => template_registry.render(name_in_registry, &render_data)?,
            ContentItem::UnregisteredTemplate(UnregisteredTemplate {
                template_registry,
                template,
            }) => {
                let context = Context::wraps(render_data)?;
                let mut render_context = RenderContext::new(None);
                template.renders(template_registry, &context, &mut render_context)?
            }
        };

        Ok(rendered_content)
    }
}
