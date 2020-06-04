use crate::lib::*;
use crate::renderer;
use std::io;
use std::io::{Read, Write};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CliError {
    #[error("Failed to read data.")]
    ReadError { source: io::Error },

    #[error("Unable to render template.")]
    RenderError {
        #[from]
        source: renderer::RendererError,
    },

    #[error("Failed to write data.")]
    WriteError { source: io::Error },
}

pub fn render(
    gluon_version: GluonVersion,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<(), CliError> {
    let mut template = String::new();
    input
        .read_to_string(&mut template)
        .map_err(|source| CliError::ReadError { source })?;

    let rendered_output = renderer::render(gluon_version, &template)?;
    write!(output, "{}", rendered_output).map_err(|source| CliError::WriteError { source })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lib::*;
    use std::str;

    #[test]
    fn renders_valid_template() {
        let mut input = VALID_TEMPLATE.as_bytes();
        let mut output = Vec::new();
        let result = render(GluonVersion("0.0.0"), &mut input, &mut output);

        assert!(result.is_ok());
        assert_eq!(
            str::from_utf8(output.as_slice()),
            Ok(VALID_TEMPLATE_RENDERED)
        );
    }

    #[test]
    fn renders_invalid_template() {
        let mut input = INVALID_TEMPLATE.as_bytes();
        let mut output = Vec::new();
        let result = render(GluonVersion("0.0.0"), &mut input, &mut output);

        assert!(result.is_err());
    }
}
