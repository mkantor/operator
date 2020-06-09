mod cli;
mod content;
mod lib;
mod test_lib;

use crate::lib::*;
use std::io;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt)]
/// Renders a handlebars template from STDIN.
#[structopt(after_help = "EXAMPLES:\n    echo \"{{#if true}}hello world{{/if}}\" | gluon .")]
struct CommandLineOptions {
    #[structopt(parse(from_os_str))]
    content_directory: PathBuf,
}

const VERSION: GluonVersion = GluonVersion(env!("CARGO_PKG_VERSION"));

fn main() {
    let options = CommandLineOptions::from_args();

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();

    if let Err(err) = cli::render(&options.content_directory, VERSION, &mut input, &mut output)
        .map_err(anyhow::Error::from)
    {
        eprintln!("Error: {:?}", err);
        std::process::exit(1);
    }
}
