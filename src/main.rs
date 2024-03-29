use anyhow::Context;
use operator::content::{ContentDirectory, MediaRange, Route};
use operator::http::QueryString;
use operator::*;
use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process;
use structopt::StructOpt;

#[derive(StructOpt)]
#[structopt(about)]
struct OperatorCommand {
    /// Silence all output.
    #[structopt(long, short = "q", global = true)]
    quiet: bool,

    /// Verbose mode; multiple -v options increase the verbosity.
    #[structopt(long, short = "v", parse(from_occurrences), global = true)]
    verbose: usize,

    #[structopt(subcommand)]
    subcommand: OperatorSubcommand,
}

#[derive(StructOpt)]
enum OperatorSubcommand {
    /// Evaluates a handlebars template from STDIN.
    #[structopt(after_help = concat!(
        "EXAMPLE:\n",
        "    echo '{{#if true}}hello world{{/if}}' | operator eval --content-directory=/dev/null"
    ), display_order = 0)]
    Eval {
        /// Path to a directory containing content files.
        ///
        /// Files in this directory can be referenced from the provided
        /// handlebars template.
        #[structopt(long, parse(from_os_str), value_name = "path")]
        content_directory: PathBuf,

        /// Optional query parameters.
        ///
        /// This uses the same format as HTTP requests (without a leading "?").
        /// For example: --query="a=1&b=2".
        #[structopt(long, value_name = "query-string")]
        query: Option<QueryString>,
    },

    /// Renders content from a content directory.
    #[structopt(after_help = concat!(
        "EXAMPLE:\n",
        "    mkdir -p content\n",
        "    echo 'hello world' > content/hello.txt\n",
        "    operator get --content-directory=./content --route=/hello"
    ), display_order = 1)]
    Get {
        /// Path to a directory containing content files.
        ///
        /// The route argument refers to files within this directory.
        #[structopt(long, parse(from_os_str), value_name = "path")]
        content_directory: PathBuf,

        /// Route specifying which piece of content to get.
        ///
        /// Routes are extension-less slash-delimited paths rooted in the
        /// content directory. They must begin with a slash.
        #[structopt(long, value_name = "route")]
        route: Route,

        /// Optional query parameters.
        ///
        /// This uses the same format as HTTP requests (without a leading "?").
        /// For example: --query="a=1&b=2".
        #[structopt(long, value_name = "query-string")]
        query: Option<QueryString>,

        /// Declares what types of media are acceptable as output.
        ///
        /// This serves the same purpose as the HTTP Accept header: to drive
        /// content negotiation. Unlike the Accept header it is only a single
        /// media range. Defaults to "*/*".
        #[structopt(long, value_name = "media-range")]
        accept: Option<MediaRange>,
    },

    /// Starts an HTTP server.
    #[structopt(after_help = concat!(
        "EXAMPLE:\n",
        "    mkdir -p site\n",
        "    echo '<!doctype html><title>my website</title><blink>under construction</blink>' > site/home.html\n",
        "    operator -vv serve --bind-to=127.0.0.1:8080 --content-directory=./site --index-route=/home",
    ), display_order = 2)]
    Serve {
        /// Path to a directory containing content files.
        ///
        /// This directory is used to create the website.
        #[structopt(long, parse(from_os_str), value_name = "path")]
        content_directory: PathBuf,

        /// What to serve when the request URI has an empty path.
        ///
        /// A request for http://mysite.com/ gets a response from this route.
        /// If this option is not set, such requests always receive a 404.
        #[structopt(long, value_name = "route")]
        index_route: Option<Route>,

        /// What to serve when there are errors.
        ///
        /// This facilitates custom error pages. When there is an HTTP error
        /// this route is used to create the response. The HTTP status code can
        /// be obtained from the `error-code` render parameter.
        ///
        /// If the error handler itself fails then a default error message is
        /// used.
        #[structopt(long, value_name = "route")]
        error_handler_route: Option<Route>,

        /// The TCP address/port that the server should bind to.
        ///
        /// This is an IP address and port number. For example, "127.0.0.1:80".
        #[structopt(long, value_name = "socket-address")]
        bind_to: SocketAddr,
    },
}

fn main() {
    let command = OperatorCommand::from_args();

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
