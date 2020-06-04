extern crate handlebars;

use crate::lib::*;
use handlebars::Handlebars;
use serde::Serialize;
use std::io;
use thiserror::Error;

/// Formats a template name for use in `RendererError` messages.
///
/// The returned `String` has a leading space if nonempty.
fn format_template_name_for_renderer_error(template_name: Option<&String>) -> String {
    match template_name {
        Some(template_name) => format!(" \"{}\"", template_name),
        None => String::from(""),
    }
}

#[derive(Error, Debug)]
pub enum RendererError {
    #[error("Failed to parse template{}.", format_template_name_for_renderer_error(.source.template_name.as_ref()))]
    TemplateError { source: handlebars::TemplateError },

    #[error("Rendering failed for template{}.", format_template_name_for_renderer_error(.source.template_name.as_ref()))]
    RenderError { source: handlebars::RenderError },

    #[error("IO error in template \"{}\".", .name)]
    IOError { source: io::Error, name: String },
}

#[derive(Serialize)]
struct GluonRenderData {
    version: GluonVersion,
}

#[derive(Serialize)]
struct RenderData {
    gluon: GluonRenderData,
}

pub fn render(gluon_version: GluonVersion, template_string: &str) -> Result<String, RendererError> {
    let registry = Handlebars::new();
    let render_data = RenderData {
        gluon: GluonRenderData {
            version: gluon_version,
        },
    };
    registry
        .render_template(template_string, &render_data)
        .map_err(|template_render_error| match template_render_error {
            handlebars::TemplateRenderError::TemplateError(source) => {
                RendererError::TemplateError { source }
            }
            handlebars::TemplateRenderError::RenderError(source) => {
                RendererError::RenderError { source }
            }
            handlebars::TemplateRenderError::IOError(source, name) => {
                RendererError::IOError { source, name }
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lib::*;

    #[test]
    fn renders_valid_template() {
        match render(GluonVersion("0.0.0"), VALID_TEMPLATE) {
            Ok(rendered) => assert_eq!(rendered, VALID_TEMPLATE_RENDERED),
            Err(_) => panic!("Rendering failed when it should have succeeded."),
        }
    }

    #[test]
    fn fails_on_invalid_template() {
        assert!(render(GluonVersion("0.0.0"), INVALID_TEMPLATE).is_err());
    }

    #[test]
    fn provides_version_to_templates() {
        match render(GluonVersion("1.2.3"), "{{gluon.version}}") {
            Ok(rendered) => assert_eq!(rendered, "1.2.3"),
            Err(_) => panic!("Rendering failed when it should have succeeded."),
        }
    }
}
