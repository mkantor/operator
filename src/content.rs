extern crate handlebars;
use crate::lib::*;
use handlebars::{Context, Handlebars, RenderContext, Renderable};
use serde::Serialize;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
#[error(
  "Failed to parse template{} from content directory path '{}'.",
  .source.template_name.as_ref().map(|known_name| format!(" '{}'", known_name)).unwrap_or_default(),
  .content_directory_path.to_string_lossy()
)]
pub struct RegisteredTemplateParseError {
    source: handlebars::TemplateError,
    content_directory_path: PathBuf,
}

#[derive(Error, Debug)]
#[error(
  "Failed to parse template{}.",
  .source.template_name.as_ref().map(|known_name| format!(" '{}'", known_name)).unwrap_or_default(),
)]
pub struct UnregisteredTemplateParseError {
    source: handlebars::TemplateError,
}

#[derive(Error, Debug)]
pub enum ContentLoadingError {
    #[error(transparent)]
    TemplateParseError(#[from] RegisteredTemplateParseError),

    #[error(
    "Input/output error when loading{} from content directory path '{}'.",
    .name.as_ref().map(|known_name| format!(" '{}'", known_name)).unwrap_or(String::from(" content")),
    .content_directory_path.to_string_lossy()
  )]
    IOError {
        source: io::Error,
        content_directory_path: PathBuf,
        name: Option<String>,
    },
}

#[derive(Error, Debug)]
#[error(
  "Rendering failed for template{}.",
  .source.template_name.as_ref().map(|known_name| format!(" '{}'", known_name)).unwrap_or_default(),
)]
pub struct TemplateRenderError {
    #[from]
    source: handlebars::RenderError,
}

pub struct ContentEngine<'a> {
    template_registry: Handlebars<'a>,
}

impl<'a> ContentEngine<'a> {
    pub fn from_content_directory(
        content_directory_path: &'a Path,
    ) -> Result<Self, ContentLoadingError> {
        let template_registry = {
            let mut template_registry = Handlebars::new();
            template_registry
                .register_templates_directory(".hbs", content_directory_path)
                .map_err(|template_render_error| match template_render_error {
                    handlebars::TemplateFileError::TemplateError(source) => {
                        ContentLoadingError::TemplateParseError(RegisteredTemplateParseError {
                            source,
                            content_directory_path: PathBuf::from(content_directory_path),
                        })
                    }
                    handlebars::TemplateFileError::IOError(source, original_name) => {
                        // Handlebars-rust will use an empty string when the error does not
                        // correspond to a specific path.
                        let name = if original_name.is_empty() {
                            None
                        } else {
                            Some(original_name)
                        };
                        ContentLoadingError::IOError {
                            source,
                            content_directory_path: PathBuf::from(content_directory_path),
                            name,
                        }
                    }
                })?;
            template_registry
        };

        Ok(ContentEngine { template_registry })
    }

    pub fn new_content(
        &self,
        handlebars_source: &str,
    ) -> Result<ContentItem, UnregisteredTemplateParseError> {
        let template = handlebars::Template::compile2(handlebars_source, true)
            .map_err(|source| UnregisteredTemplateParseError { source })?;
        Ok(ContentItem::UnregisteredTemplate {
            template_registry: &self.template_registry,
            template,
        })
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

pub enum ContentItem<'a> {
    /// A named template that exists in the registry. This variant must only be
    /// constructed with names that have already been validated against the
    /// registry.
    RegisteredTemplate {
        template_registry: &'a Handlebars<'a>,
        name_in_registry: &'a str,
    },

    /// An anonymous template for on-the-fly rendering.
    UnregisteredTemplate {
        template_registry: &'a Handlebars<'a>,
        template: handlebars::Template,
    },
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
            ContentItem::RegisteredTemplate {
                template_registry,
                name_in_registry,
            } => template_registry.render(name_in_registry, &render_data)?,
            ContentItem::UnregisteredTemplate {
                template_registry,
                template,
            } => {
                let context = Context::wraps(render_data)?;
                let mut render_context = RenderContext::new(None);
                template.renders(template_registry, &context, &mut render_context)?
            }
        };

        Ok(rendered_content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lib::*;

    #[test]
    fn content_engine_can_be_created_from_valid_content_directory() {
        for &path in &CONTENT_DIRECTORY_PATHS_WITH_VALID_CONTENTS {
            assert!(
                ContentEngine::from_content_directory(Path::new(path)).is_ok(),
                "Content engine could not be created from {}",
                path
            );
        }
    }

    #[test]
    fn content_engine_cannot_be_created_from_invalid_content_directory() {
        for &path in &CONTENT_DIRECTORY_PATHS_WITH_INVALID_CONTENTS {
            assert!(
                ContentEngine::from_content_directory(Path::new(path)).is_err(),
                "Content engine was successfully created from {}, but this should have failed",
                path
            );
        }
    }

    #[test]
    fn new_templates_can_be_rendered() {
        let engine = ContentEngine::from_content_directory(
            arbitrary_content_directory_path_with_valid_content(),
        )
        .expect("Content engine could not be created");

        for &(template, expected_output) in &VALID_TEMPLATES {
            let new_content = engine
                .new_content(template)
                .expect("Template could not be parsed");
            let rendered = new_content
                .render(GluonVersion("0.0.0"))
                .expect(&format!("Template rendering failed for `{}`", template,));
            assert_eq!(
                rendered,
                expected_output,
                "Template rendering for `{}` did not produce the expected output (\"{}\"), instead got \"{}\"",
                template,
                expected_output,
                rendered,
            );
        }
    }

    #[test]
    fn new_content_fails_for_invalid_templates() {
        let engine = ContentEngine::from_content_directory(
            arbitrary_content_directory_path_with_valid_content(),
        )
        .expect("Content engine could not be created");

        for &template in &INVALID_TEMPLATES {
            let result = engine.new_content(template);

            assert!(
                result.is_err(),
                "Content was successfully created for invalid template `{}`, but it should have failed",
                template,
            );
        }
    }

    #[test]
    fn new_templates_can_reference_partials_from_content_directory() {
        let content_directory_path = example_path("valid/partials");
        let engine = ContentEngine::from_content_directory(&content_directory_path)
            .expect("Content engine could not be created");

        let template = "this is partial: {{> ab}}";
        let expected_output = "this is partial: a\nb\n\n";

        let new_content = engine
            .new_content(template)
            .expect("Template could not be parsed");
        let rendered = new_content
            .render(GluonVersion("0.0.0"))
            .expect(&format!("Template rendering failed for `{}`", template,));
        assert_eq!(
            rendered,
            expected_output,
            "Template rendering for `{}` did not produce the expected output (\"{}\"), instead got \"{}\"",
            template,
            expected_output,
            rendered,
        );
    }
}
