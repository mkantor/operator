use anyhow::Context;
use clap::{Parser, Subcommand};
use operator::content::{ContentDirectory, MediaRange, Route};
use operator::http::QueryString;
use operator::*;
use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process;

#[derive(Parser)]
#[command(version, about, propagate_version = true)]
struct OperatorCommand {
    /// Silence all output.
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Verbose mode; multiple -v options increase the verbosity.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    subcommand: OperatorSubcommand,
}

#[derive(Subcommand)]
enum OperatorSubcommand {
    /// Evaluates a handlebars template from STDIN.
    Eval {
        /// Path to a directory containing content files.
        ///
        /// Files in this directory can be referenced from the provided
        /// handlebars template.
        #[arg(long, value_name = "path")]
        content_directory: PathBuf,

        /// Optional query parameters.
        ///
        /// This uses the same format as HTTP requests (without a leading "?").
        /// For example: --query="a=1&b=2".
        #[arg(long, value_name = "query-string")]
        query: Option<QueryString>,
    },

    /// Renders content from a content directory.
    Get {
        /// Path to a directory containing content files.
        ///
        /// The route argument refers to files within this directory.
        #[clap(long, value_name = "path")]
        content_directory: PathBuf,

        /// Route specifying which piece of content to get.
        ///
        /// Routes are extension-less slash-delimited paths rooted in the
        /// content directory. They must begin with a slash.
        #[clap(long, value_name = "route")]
        route: Route,

        /// Optional query parameters.
        ///
        /// This uses the same format as HTTP requests (without a leading "?").
        /// For example: --query="a=1&b=2".
        #[clap(long, value_name = "query-string")]
        query: Option<QueryString>,

        /// Declares what types of media are acceptable as output.
        ///
        /// This serves the same purpose as the HTTP Accept header: to drive
        /// content negotiation. Unlike the Accept header it is only a single
        /// media range. Defaults to "*/*".
        #[clap(long, value_name = "media-range")]
        accept: Option<MediaRange>,
    },

    /// Starts an HTTP server.
    Serve {
        /// Path to a directory containing content files.
        ///
        /// This directory is used to create the website.
        #[clap(long, value_name = "path")]
        content_directory: PathBuf,

        /// What to serve when the request URI has an empty path.
        ///
        /// A request for http://mysite.com/ gets a response from this route.
        /// If this option is not set, such requests always receive a 404.
        #[clap(long, value_name = "route")]
        index_route: Option<Route>,

        /// What to serve when there are errors.
        ///
        /// This facilitates custom error pages. When there is an HTTP error
        /// this route is used to create the response. The HTTP status code can
        /// be obtained from the `error-code` render parameter.
        ///
        /// If the error handler itself fails then a default error message is
        /// used.
        #[clap(long, value_name = "route")]
        error_handler_route: Option<Route>,

        /// The TCP address/port that the server should bind to.
        ///
        /// This is an IP address and port number. For example, "127.0.0.1:80".
        #[clap(long, value_name = "socket-address")]
        bind_to: SocketAddr,
    },
}

fn main() {
    let command = OperatorCommand::parse();

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();

    let result = stderrlog::new()
        .quiet(command.quiet)
        .verbosity(usize::from(command.verbose))
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .map_err(anyhow::Error::from)
        .and_then(|()| handle_subcommand(command.subcommand, &mut input, &mut output));

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
    subcommand: OperatorSubcommand,
    input: &mut I,
    output: &mut O,
) -> Result<(), anyhow::Error> {
    match subcommand {
        OperatorSubcommand::Eval {
            content_directory,
            query,
        } => cli::eval(
            get_content_directory(content_directory)?,
            query,
            input,
            output,
        )
        .map_err(anyhow::Error::from),

        OperatorSubcommand::Get {
            content_directory,
            route,
            query,
            accept,
        } => cli::get(
            get_content_directory(content_directory)?,
            &route,
            query,
            accept,
            output,
        )
        .map_err(anyhow::Error::from),

        OperatorSubcommand::Serve {
            content_directory,
            index_route,
            error_handler_route,
            bind_to,
        } => cli::serve(
            get_content_directory(content_directory)?,
            index_route,
            error_handler_route,
            bind_to,
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
