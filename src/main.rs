mod cli;
mod content;
mod content_directory;
mod http;
mod lib;
mod test_lib;

use crate::content_directory::ContentDirectory;
use crate::lib::*;
use mime::Mime;
use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use structopt::StructOpt;

const VERSION: SolitonVersion = SolitonVersion(env!("CARGO_PKG_VERSION"));

#[derive(StructOpt)]
enum SolitonCommand {
    /// Evaluates a handlebars template from STDIN.
    #[structopt(
        after_help = "EXAMPLES:\n    echo \"{{#if true}}hello world{{/if}}\" | soliton render --content-directory=path/to/content --source-media-type=text/html --target-media-type=text/html"
    )]
    Render {
        #[structopt(long, parse(from_os_str))]
        content_directory: PathBuf,

        #[structopt(long)]
        source_media_type: Mime,

        #[structopt(long)]
        target_media_type: Mime,
    },

    /// Gets content from the content directory.
    #[structopt(
        after_help = "EXAMPLES:\n    mkdir content && echo 'hello world' > content/hello.html.hbs && soliton get --content-directory=path/to/content --address=hello --target-media-type=text/html"
    )]
    Get {
        #[structopt(long, parse(from_os_str))]
        content_directory: PathBuf,

        #[structopt(long)]
        address: String,

        #[structopt(long)]
        target_media_type: Mime,
    },

    /// Serves the content directory over HTTP.
    #[structopt(
        after_help = "EXAMPLES:\n    mkdir site && echo '<!doctype html><title>my website</title><blink>under construction</blink>' > site/home.html && soliton serve --content-directory=./site --index-address=home --socket-address=127.0.0.1:8080"
    )]
    Serve {
        #[structopt(long, parse(from_os_str))]
        content_directory: PathBuf,

        #[structopt(long)]
        index_address: String,

        #[structopt(long)]
        socket_address: SocketAddr,
    },
}

fn main() {
    let command = SolitonCommand::from_args();

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();

    match handle_command(command, &mut input, &mut output) {
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
    command: SolitonCommand,
    input: &mut I,
    output: &mut O,
) -> Result<(), anyhow::Error> {
    stderrlog::new()
        .verbosity(3)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()?;

    match command {
        SolitonCommand::Render {
            content_directory,
            source_media_type,
            target_media_type,
        } => cli::render(
            ContentDirectory::from_root(&fs::canonicalize(content_directory)?)?,
            source_media_type.clone(),
            &target_media_type,
            VERSION,
            input,
            output,
        )
        .map_err(anyhow::Error::from),

        SolitonCommand::Get {
            content_directory,
            address,
            target_media_type,
        } => cli::get(
            ContentDirectory::from_root(&fs::canonicalize(content_directory)?)?,
            &address,
            &target_media_type,
            VERSION,
            output,
        )
        .map_err(anyhow::Error::from),

        SolitonCommand::Serve {
            content_directory,
            index_address,
            socket_address,
        } => cli::serve(
            ContentDirectory::from_root(&fs::canonicalize(content_directory)?)?,
            &index_address,
            socket_address,
            VERSION,
        )
        .map_err(anyhow::Error::from),
    }
}
