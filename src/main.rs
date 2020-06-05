use std::io;

mod cli;
mod lib;
mod renderer;
mod test_lib;

use crate::lib::*;
use structopt::StructOpt;

#[derive(StructOpt)]
/// Renders a handlebars template from STDIN.
#[structopt(after_help = "EXAMPLES:\n    echo \"{{#if true}}hello world{{/if}}\" | gluon")]
struct CommandLineOptions {
    // Options will go here.
}

const VERSION: GluonVersion = GluonVersion(env!("CARGO_PKG_VERSION"));

fn main() {
    let _options = CommandLineOptions::from_args();

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();

    if let Err(err) = cli::render(VERSION, &mut input, &mut output).map_err(anyhow::Error::from) {
        eprintln!("Error: {:?}", err);
        std::process::exit(1);
    }
}
