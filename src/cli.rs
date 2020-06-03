use crate::renderer;
use std::io::{Read, Write};

pub enum Error {
    ReadError(),
    RenderError(),
    WriteError(),
}

pub fn render(input: &mut Read, output: &mut Write) -> Result<(), Error> {
    let mut template = String::new();
    input
        .read_to_string(&mut template)
        .map_err(|_| Error::ReadError())?;

    let rendered_output = renderer::render(&template).map_err(|_| Error::RenderError())?;
    write!(output, "{}", rendered_output).map_err(|_| Error::WriteError())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lib::*;
    use std::str;

    #[test]
    fn test_render_valid_template() {
        let mut input = valid_template.as_bytes();
        let mut output = Vec::new();
        let result = render(&mut input, &mut output);

        assert!(result.is_ok());
        assert_eq!(
            str::from_utf8(output.as_slice()),
            Ok(valid_template_rendered)
        );
    }

    #[test]
    fn test_render_invalid_template() {
        let mut input = invalid_template.as_bytes();
        let mut output = Vec::new();
        let result = render(&mut input, &mut output);

        assert!(result.is_err());
    }
}
