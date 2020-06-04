use std::io;

mod cli;
mod renderer;
mod test_lib;

const USAGE: &str = "Usage: gluon

Renders a handlebars template from STDIN.

Try: echo \"{{#if true}}hello world{{/if}}\" | gluon";

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();

    if let Err(err) = cli::render(&mut input, &mut output).map_err(anyhow::Error::from) {
        eprintln!("Error: {:?}\n\n{}", err, USAGE);
        std::process::exit(1);
    }
}
