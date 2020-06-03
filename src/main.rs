use std::fmt;
use std::io;

mod cli;
mod renderer;
mod test_lib;

const USAGE: &'static str = "Usage: gluon

Renders a handlebars template from STDIN.

Try: echo \"{{#if true}}hello world{{/if}}\" | gluon";

type Success = ();

enum Error {
    Unknown(),
}
impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let error_message = match self {
            Error::Unknown() => "Unknown error.",
        };
        write!(f, "{}\n\n{}", error_message, USAGE)
    }
}

fn main() -> Result<Success, Error> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();

    cli::render(&mut input, &mut output).map_err(|_| Error::Unknown())
}
