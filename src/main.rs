use std::fmt;
use std::io;

mod cli;

const USAGE: &'static str = "Usage: gluon

Renders a handlebars template from STDIN (eventually).

Try: echo \"hello world\" | gluon";

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

    cli::echo(&mut input, &mut output).map_err(|_| Error::Unknown())
}
