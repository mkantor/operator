mod cli;
mod content;
mod http;
mod lib;
mod test_lib;

use crate::content::{ContentDirectory, MediaRange, MediaType};
use crate::lib::*;
use anyhow::Context;
use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process;
use structopt::StructOpt;

const VERSION: SolitonVersion = SolitonVersion(env!("CARGO_PKG_VERSION"));

#[derive(StructOpt)]
struct SolitonCommand {
    #[structopt(long, short = "q")]
    quiet: bool,

    #[structopt(long, short = "v", parse(from_occurrences))]
    verbose: usize,

    #[structopt(subcommand)]
    subcommand: SolitonSubcommand,
}

#[derive(StructOpt)]
enum SolitonSubcommand {
    /// Evaluates a handlebars template from STDIN.
    #[structopt(
        after_help = "EXAMPLE:\n    echo '{{#if true}}hello world{{/if}}' \\\n        | soliton render --content-directory=/dev/null --media-type=text/plain"
    )]
    Render {
        #[structopt(long, parse(from_os_str))]
        content_directory: PathBuf,

        #[structopt(long)]
        media_type: MediaType,
    },

    /// Renders a file from the content directory.
    #[structopt(
        after_help = "EXAMPLE:\n    mkdir -p content\n    echo 'hello world' > content/hello.txt\n    soliton get --content-directory=content --route=hello --accept=text/*"
    )]
    Get {
        #[structopt(long, parse(from_os_str))]
        content_directory: PathBuf,

        #[structopt(long)]
        route: String,

        #[structopt(long)]
        accept: MediaRange,
    },

    /// Serves the content directory over HTTP.
    #[structopt(
        after_help = "EXAMPLE:\n    mkdir -p site\n    echo '<!doctype html><title>my website</title><blink>under construction</blink>' > site/home.html\n    soliton -vv serve --content-directory=site --index-route=home --socket-address=127.0.0.1:8080"
    )]
    Serve {
        #[structopt(long, parse(from_os_str))]
        content_directory: PathBuf,

        #[structopt(long)]
        index_route: Option<String>,

        #[structopt(long)]
        error_handler_route: Option<String>,

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

    let result = stderrlog::new()
        .quiet(command.quiet)
        .verbosity(command.verbose)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .map_err(anyhow::Error::from)
        .and_then(|_| handle_subcommand(command.subcommand, &mut input, &mut output));

    match result {
        Err(error) => {
            log::error!("{:?}", error);
            process::exit(1);
        }
        Ok(_) => {
            process::exit(0);
        }
    }
}

fn handle_subcommand<I: io::Read, O: io::Write>(
    subcommand: SolitonSubcommand,
    input: &mut I,
    output: &mut O,
) -> Result<(), anyhow::Error> {
    match subcommand {
        SolitonSubcommand::Render {
            content_directory,
            media_type,
        } => cli::render(
            get_content_directory(content_directory)?,
            media_type,
            VERSION,
            input,
            output,
        )
        .map_err(anyhow::Error::from),

        SolitonSubcommand::Get {
            content_directory,
            route,
            accept,
        } => cli::get(
            get_content_directory(content_directory)?,
            &route,
            accept,
            VERSION,
            output,
        )
        .map_err(anyhow::Error::from),

        SolitonSubcommand::Serve {
            content_directory,
            index_route,
            error_handler_route,
            socket_address,
        } => cli::serve(
            get_content_directory(content_directory)?,
            index_route,
            error_handler_route,
            socket_address,
            VERSION,
        )
        .map_err(anyhow::Error::from),
    }
}

fn get_content_directory<P: AsRef<Path>>(path: P) -> Result<ContentDirectory, anyhow::Error> {
    let path = path.as_ref();
    let canonical_path = &fs::canonicalize(path)
        .with_context(|| format!("Cannot use '{}' as a content directory.", path.display()))?;
    let content_directory = ContentDirectory::from_root(canonical_path)?;
    Ok(content_directory)
}
