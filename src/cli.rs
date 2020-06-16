use crate::content::*;
use crate::directory::Directory;
use crate::lib::*;
use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RenderCommandError {
    #[error("Failed to read input.")]
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

    #[error("Failed to write output.")]
    WriteError { source: io::Error },
}

#[derive(Error, Debug)]
pub enum GetCommandError {
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

    #[error("Content not found at address '{}'.", .address)]
    ContentNotFound { address: String },

    #[error("Unable to render template.")]
    TemplateRenderError {
        #[from]
        source: TemplateRenderError,
    },

    #[error("Failed to write output.")]
    WriteError { source: io::Error },
}

/// Reads a template from `input`, renders it, and writes it to `output`.
pub fn render<I: io::Read, O: io::Write>(
    content_directory: Directory,
    gluon_version: GluonVersion,
    input: &mut I,
    output: &mut O,
) -> Result<(), RenderCommandError> {
    let engine = ContentEngine::from_content_directory(content_directory)?;

    let mut template = String::new();
    input
        .read_to_string(&mut template)
        .map_err(|source| RenderCommandError::ReadError { source })?;

    let content_item = engine.new_content(&template)?;
    let render_data = engine.get_render_data(gluon_version);
    let rendered_output = content_item.render(&render_data)?;
    write!(output, "{}", rendered_output)
        .map_err(|source| RenderCommandError::WriteError { source })?;

    Ok(())
}

/// Renders an item from the content directory and writes it to `output`.
pub fn get<O: io::Write>(
    content_directory: Directory,
    address: &str,
    gluon_version: GluonVersion,
    output: &mut O,
) -> Result<(), GetCommandError> {
    let engine = ContentEngine::from_content_directory(content_directory)?;
    let content_item = engine
        .get(address)
        .ok_or(GetCommandError::ContentNotFound {
            address: String::from(address),
        })?;
    let render_data = engine.get_render_data(gluon_version);
    let rendered_output = content_item.render(&render_data)?;
    write!(output, "{}", rendered_output)
        .map_err(|source| GetCommandError::WriteError { source })?;

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
            let directory = arbitrary_content_directory_with_valid_content();
            let result = render(directory, GluonVersion("0.0.0"), &mut input, &mut output);

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
            let directory = arbitrary_content_directory_with_valid_content();
            let result = render(directory, GluonVersion("0.0.0"), &mut input, &mut output);

            assert!(
                result.is_err(),
                "Template rendering succeeded for `{}`, but it should have failed",
                template,
            );
        }
    }

    #[test]
    fn cli_can_get_content() {
        let mut output = Vec::new();
        let content_directory_path = &example_path("valid/hello-world");
        let address = "hello";
        let expected_output = "hello world\n";

        let directory = arbitrary_content_directory_with_valid_content();
        let result = get(directory, address, GluonVersion("0.0.0"), &mut output);

        assert!(
            result.is_ok(),
            "Template rendering failed for content at '{}' in '{}': {}",
            address,
            content_directory_path.display(),
            result.unwrap_err(),
        );
        let output_as_str = str::from_utf8(output.as_slice()).expect("Output was not UTF-8");
        assert_eq!(
            output_as_str,
            expected_output,
            "Template rendering for content at '{}' in '{}' did not produce the expected output (\"{}\"), instead got \"{}\"",
            address,
            content_directory_path.display(),
            expected_output,
            output_as_str
        );
    }

    #[test]
    fn cli_can_fail_to_get_content_which_does_not_exist() {
        let mut output = Vec::new();
        let address = "this-address-does-not-refer-to-any-content";

        let directory = arbitrary_content_directory_with_valid_content();
        let result = get(directory, address, GluonVersion("0.0.0"), &mut output);

        match result {
            Ok(_) => panic!(
                "Getting content from '{}' succeeded, but it should have failed",
                address
            ),
            Err(GetCommandError::ContentNotFound {
                address: address_from_error,
            }) => assert_eq!(
                address_from_error, address,
                "Address from error did not match address used"
            ),
            Err(_) => panic!("Wrong type of error was produced, expected ContentNotFound"),
        };
    }
}
