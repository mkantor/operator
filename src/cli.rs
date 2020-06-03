use std::io::{Read, Write};

pub enum Error {
    ReadError(),
    WriteError(),
}

pub fn echo(input: &mut Read, output: &mut Write) -> Result<(), Error> {
    let mut input_buffer = String::new();
    input
        .read_to_string(&mut input_buffer)
        .map_err(|_| Error::ReadError())?;
    write!(output, "{}", input_buffer).map_err(|_| Error::WriteError())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_echo() {
        let mut input = "an arbitrary str".as_bytes();
        let mut output = Vec::new();
        let expected_output = input;
        let result = echo(&mut input, &mut output);

        assert!(result.is_ok());
        assert_eq!(output.as_slice(), expected_output);
    }
}
