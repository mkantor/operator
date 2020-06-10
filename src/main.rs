mod cli;
mod content;
mod lib;
mod test_lib;

use crate::lib::*;
use std::io;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt)]
enum GluonCommand {
    /// Evaluates a handlebars template from STDIN.
    #[structopt(
        after_help = "EXAMPLES:\n    echo \"{{#if true}}hello world{{/if}}\" | gluon render --content-directory=."
    )]
    Render {
        #[structopt(long, parse(from_os_str))]
        content_directory: PathBuf,
    },
}

const VERSION: GluonVersion = GluonVersion(env!("CARGO_PKG_VERSION"));

fn main() {
    let result = match GluonCommand::from_args() {
        GluonCommand::Render { content_directory } => {
            let stdin = io::stdin();
            let stdout = io::stdout();
            let mut input = stdin.lock();
            let mut output = stdout.lock();

            cli::render(&content_directory, VERSION, &mut input, &mut output)
                .map_err(anyhow::Error::from)
        }
    };

    match result {
        Err(error) => {
            eprintln!("Error: {:?}", error);
            std::process::exit(1);
        }
        Ok(_) => {
            std::process::exit(0);
        }
    }
}
