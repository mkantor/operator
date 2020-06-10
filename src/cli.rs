use crate::content::*;
use crate::lib::*;
use std::io;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CliError {
    #[error("Failed to read data.")]
    ReadError { source: io::Error },

    #[error("Unable to load content.")]
    ContentLoadingError {
        #[from]
        source: ContentLoadingError,
    },

    #[error("Unable to parse template from content directory.")]
    RegisteredTemplateParseError {
        #[from]
        source: RegisteredTemplateParseError,
    },

    #[error("Unable to parse template from input.")]
    UnregisteredTemplateParseError {
        #[from]
        source: UnregisteredTemplateParseError,
    },

    #[error("Unable to render template.")]
    TemplateRenderError {
        #[from]
        source: TemplateRenderError,
    },

    #[error("Failed to write data.")]
    WriteError { source: io::Error },
}

/// Reads a template from `input`, renders it, and writes it to `output`.
pub fn render(
    content_directory_path: &Path,
    gluon_version: GluonVersion,
    input: &mut dyn io::Read,
    output: &mut dyn io::Write,
) -> Result<(), CliError> {
    let engine = ContentEngine::from_content_directory(content_directory_path)?;

    let mut template = String::new();
    input
        .read_to_string(&mut template)
        .map_err(|source| CliError::ReadError { source })?;

    let rendered_output = engine.new_content(&template)?.render(gluon_version)?;
    write!(output, "{}", rendered_output).map_err(|source| CliError::WriteError { source })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lib::*;
    use std::str;

    #[test]
    fn cli_renders_valid_templates() {
        for &(template, expected_output) in &VALID_TEMPLATES {
            let mut input = template.as_bytes();
            let mut output = Vec::new();
            let result = render(
                arbitrary_content_directory_path_with_valid_content(),
                GluonVersion("0.0.0"),
                &mut input,
                &mut output,
            );

            assert!(
                result.is_ok(),
                "Template rendering failed for `{}`: {}",
                template,
                result.unwrap_err(),
            );
            let output_as_str = str::from_utf8(output.as_slice()).expect("Output was not UTF-8");
            assert_eq!(
                output_as_str,
                expected_output,
                "Template rendering for `{}` did not produce the expected output (\"{}\"), instead got \"{}\"",
                template,
                expected_output,
                output_as_str
            );
        }
    }

    #[test]
    fn cli_fails_to_render_invalid_templates() {
        for &template in &INVALID_TEMPLATES {
            let mut input = template.as_bytes();
            let mut output = Vec::new();
            let result = render(
                arbitrary_content_directory_path_with_valid_content(),
                GluonVersion("0.0.0"),
                &mut input,
                &mut output,
            );

            assert!(
                result.is_err(),
                "Template rendering succeeded for `{}`, but it should have failed",
                template,
            );
        }
    }
}
