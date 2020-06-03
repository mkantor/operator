extern crate handlebars;

use handlebars::{Handlebars, TemplateRenderError};

pub enum Error {
    RenderError(TemplateRenderError),
}

pub fn render(template_string: &str) -> Result<String, Error> {
    let registry = Handlebars::new();
    registry
        .render_template(template_string, &())
        .map_err(|underlying_error| Error::RenderError(underlying_error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lib::*;

    #[test]
    fn test_render_valid_template() {
        match render(valid_template) {
            Ok(rendered) => assert_eq!(rendered, valid_template_rendered),
            Err(_) => panic!("Rendering failed when it should have succeeded."),
        }
    }

    #[test]
    fn test_render_invalid_template() {
        assert!(render(invalid_template).is_err());
    }
}
