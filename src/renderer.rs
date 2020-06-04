extern crate handlebars;

use handlebars::Handlebars;
use std::io;
use thiserror::Error;

/// Formats a template name for use in `RendererError` messages.
///
/// The returned `String` has a leading space if non-empty.
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

pub fn render(template_string: &str) -> Result<String, RendererError> {
    let registry = Handlebars::new();
    registry
        .render_template(template_string, &())
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
        match render(VALID_TEMPLATE) {
            Ok(rendered) => assert_eq!(rendered, VALID_TEMPLATE_RENDERED),
            Err(_) => panic!("Rendering failed when it should have succeeded."),
        }
    }

    #[test]
    fn renders_invalid_template() {
        assert!(render(INVALID_TEMPLATE).is_err());
    }
}
