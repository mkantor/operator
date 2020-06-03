use std::fmt;

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
    Err(Error::Unknown())
}
