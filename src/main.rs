mod cli;
mod content;
mod content_directory;
mod lib;
mod test_lib;

use crate::content_directory::ContentDirectory;
use crate::lib::*;
use mime::Mime;
use std::io;
use std::path::PathBuf;
use structopt::StructOpt;

const VERSION: SolitonVersion = SolitonVersion(env!("CARGO_PKG_VERSION"));

#[derive(StructOpt)]
enum SolitonCommand {
    /// Evaluates a handlebars template from STDIN.
    #[structopt(
        after_help = "EXAMPLES:\n    echo \"{{#if true}}hello world{{/if}}\" | soliton render --content-directory=path/to/content --source-media-type=text/html"
    )]
    Render {
        #[structopt(long, parse(from_os_str))]
        content_directory: PathBuf,

        #[structopt(long)]
        source_media_type: Mime,
    },

    /// Gets content from the content directory.
    #[structopt(
        after_help = "EXAMPLES:\n    mkdir content && echo 'hello world' > content/hello.hbs && soliton get --content-directory=path/to/content --address=hello"
    )]
    Get {
        #[structopt(long, parse(from_os_str))]
        content_directory: PathBuf,

        #[structopt(long)]
        address: String,
    },
}

fn main() {
    let command = SolitonCommand::from_args();

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();

    match handle_command(&command, &mut input, &mut output) {
        Err(error) => {
            eprintln!("Error: {:?}", error);
            std::process::exit(1);
        }
        Ok(_) => {
            std::process::exit(0);
        }
    }
}

fn handle_command<I: io::Read, O: io::Write>(
    command: &SolitonCommand,
    input: &mut I,
    output: &mut O,
) -> Result<(), anyhow::Error> {
    match command {
        SolitonCommand::Render {
            content_directory,
            source_media_type,
        } => cli::render(
            ContentDirectory::from_root(content_directory)?,
            source_media_type.clone(),
            VERSION,
            input,
            output,
        )
        .map_err(anyhow::Error::from),

        SolitonCommand::Get {
            content_directory,
            address,
        } => cli::get(
            ContentDirectory::from_root(content_directory)?,
            &address,
            VERSION,
            output,
        )
        .map_err(anyhow::Error::from),
    }
}
