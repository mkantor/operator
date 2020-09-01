use actix_web::client::{Client as HttpClient, ClientResponse};
use actix_web::error::PayloadError;
use actix_web::http::StatusCode;
use actix_web::test::unused_addr;
use bytes::{Bytes, BytesMut};
use content::ContentDirectory;
use futures::{future, Stream, TryStreamExt};
use mime_guess::MimeGuess;
use regex::Regex;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::hash::Hasher;
use std::io::Write;
use std::net::SocketAddr;
use std::process::{Child, Command, Output, Stdio};
use std::str;
use std::thread;
use std::time;

// Pull in some utilities from the main crate.
#[path = "../src/content/mod.rs"]
mod content;
#[path = "../src/lib.rs"]
mod lib;
#[path = "../src/test_lib.rs"]
mod test_lib;

use test_lib::*;

fn operator_command<I, S>(args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let target_dir = env::current_exe()
        .ok()
        .map(|mut path| {
            path.pop();
            if path.ends_with("deps") {
                path.pop();
            }
            path
        })
        .unwrap();

    let bin_path = target_dir.join(format!("operator{}", env::consts::EXE_SUFFIX));

    let mut operator = Command::new(bin_path);
    operator.args(args);
    operator
}

struct RunningServer {
    address: SocketAddr,
    process: Child,
}

impl RunningServer {
    fn start(content_directory: &ContentDirectory) -> Result<Self, String> {
        let address = unused_addr();

        let mut command = operator_command(&[
            "serve",
            "--quiet",
            &format!(
                "--content-directory={}",
                content_directory
                    .root()
                    .to_str()
                    .expect("Content directory root path was not UTF-8")
            ),
            &format!("--bind-to={}", address),
        ]);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        let mut process = command.spawn().expect("Failed to spawn process");

        // Give the server a chance to start up.
        // TODO: It would be better to poll by retrying a few times if the
        // connection is refused.
        thread::sleep(time::Duration::from_millis(500));

        // The server may have failed to start if the content directory was invalid.
        if let Ok(Some(_)) = process.try_wait() {
            Err(match process.wait_with_output() {
                Err(error) => format!(
                    "Server for {} failed to start and output is unavailable: {}",
                    content_directory.root().to_string_lossy(),
                    error,
                ),
                Ok(output) => format!(
                    "Server for {} failed to start: {}",
                    content_directory.root().to_string_lossy(),
                    String::from_utf8_lossy(&output.stderr),
                ),
            })
        } else {
            Ok(RunningServer { address, process })
        }
    }

    fn address(&self) -> &SocketAddr {
        &self.address
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.process.kill().expect("Failed to kill server")
    }
}

/// Attempts to render all non-hidden files in ContentDirectory, returning
/// them as a map of Route -> RenderedContent | ErrorMessage.
async fn render_everything_for_snapshots(
    content_directory: &ContentDirectory,
) -> HashMap<String, String> {
    let server_result = RunningServer::start(content_directory);

    // The server should successfully start up for valid content directories
    // and fail to start for invalid ones.
    let optional_server = match server_result {
        Err(message) => {
            assert!(
                !sample_content_directories_with_valid_contents().contains(content_directory),
                "Server failed to start for {}, which should be valid: {}",
                content_directory.root().to_string_lossy(),
                message,
            );
            None
        }
        Ok(ref server) => {
            assert!(
                !sample_content_directories_with_invalid_contents().contains(content_directory),
                "Server successfully started for {}, which should be invalid",
                content_directory.root().to_string_lossy(),
            );
            Some(server)
        }
    };

    let render_operations = content_directory
        .into_iter()
        .filter(|content_file| !content_file.is_hidden())
        .map(|content_file| async move {
            let route = content_file.relative_path_without_extensions();
            let empty_string = String::from("");
            let first_filename_extension =
                content_file.extensions().first().unwrap_or(&empty_string);

            // Target media type is just the source media type.
            let target_media_type = MimeGuess::from_ext(first_filename_extension)
                .first()
                .unwrap_or(mime::STAR_STAR);

            let output = render_multiple_ways_for_snapshots(
                optional_server.map(RunningServer::address),
                content_directory,
                route,
                &target_media_type.to_string(),
            )
            .await;

            let output_or_error_message = match String::from_utf8(output) {
                Ok(string) => string,
                Err(from_utf8_error) => {
                    let hash = {
                        let mut hasher = DefaultHasher::new();
                        hasher.write(from_utf8_error.as_bytes());
                        hasher.finish()
                    };
                    format!("binary data with hash {:x}", hash)
                }
            };

            (
                String::from(content_file.relative_path()),
                output_or_error_message,
            )
        });

    let content = future::join_all(render_operations)
        .await
        .into_iter()
        .collect::<HashMap<String, String>>();
    content
}

/// Render the desired content using a few different methods and verify that
/// they all produce the same result.
/// If `server_address` is `None`, no HTTP-based rendering is performed.
async fn render_multiple_ways_for_snapshots(
    server_address: Option<&SocketAddr>,
    content_directory: &ContentDirectory,
    route: &str,
    accept: &str,
) -> Vec<u8> {
    let get_command_output = render_via_get_command(content_directory, route, accept);
    match server_address {
        None => {
            if get_command_output.status.success() {
                get_command_output.stdout
            } else {
                get_command_output.stderr
            }
        }
        Some(server_address) => {
            let (http_response_status, http_response_body_result) =
                render_via_http_request(server_address, route, accept).await;

            // This is complicated. One of the reasons is that only certain
            // types of stream errors produce payload errors when using actix
            // clients (others will just successfully return the streamed bytes
            // up to the point of failure). For every type of error I've been
            // able to produce, curl and web browsers report errors in some
            // fashion (e.g. "curl: (18) transfer closed with outstanding read
            // data remaining" or a warning in browser dev tools), so this is
            // considered a deficiency in how actix clients work and is
            // clumsily hacked around below.
            match http_response_body_result {
                Err(payload_error) => panic!(
                    "HTTP request for /{} resulted in payload error: {}",
                    route, payload_error,
                ),
                Ok(http_response_body) => {
                    // We check this down here so all the basic validations
                    // performed up to this point are still applied to files
                    // which do not get snapshotted. We don't want to look at
                    // the output though (one reason is to allow sample files
                    // that are non-deterministic, as long as they aren't part
                    // of the snapshots).
                    if is_omitted_from_snapshots(route) {
                        Vec::new()
                    } else {
                        // If the HTTP body matches what's on stdout, and the
                        // HTTP status indicates success, we're good.
                        if http_response_body == get_command_output.stdout
                            && http_response_status.is_success()
                            && get_command_output.status.success()
                        {
                            get_command_output.stdout
                        }
                        // Just like the previous case, but the get command
                        // failed. This is necessary because streaming errors
                        // result in non-zero exit code but a 200 HTTP status.
                        else if http_response_body == get_command_output.stdout
                            && http_response_status.is_success()
                            && !get_command_output.status.success()
                        {
                            get_command_output.stderr
                        }
                        // If both the get command's exit code and HTTP status
                        // indicate failure, we're good. The error messages do
                        // not need to be identical, and the CLI error is more
                        // detailed, so use that.
                        else if !http_response_status.is_success()
                            && !get_command_output.status.success()
                        {
                            get_command_output.stderr
                        }
                        // If none of the above conditions were true then the
                        // behavior of the get command and HTTP request is
                        // different enough to be considered a bug.
                        else {
                            panic!(
                                "Rendering /{} as {} produced different results when done via server and get command!\n    \
                                get command exit code: {}\n    \
                                get command stdout: {}\n    \
                                get command stderr: {}\n    \
                                HTTP status code: {}\n    \
                                HTTP response body: {}\n",
                                route,
                                accept,
                                get_command_output.status,
                                if get_command_output.stdout.len() > 64 {
                                    format!("{} bytes", get_command_output.stdout.len())
                                } else {
                                    format!("{:?}", Bytes::from(get_command_output.stdout))
                                },
                                if get_command_output.stderr.len() > 64 {
                                    format!("{} bytes", get_command_output.stderr.len())
                                } else {
                                    format!("{:?}", Bytes::from(get_command_output.stderr))
                                },
                                http_response_status,
                                if http_response_body.len() > 64 {
                                    format!("{} bytes", http_response_body.len())
                                } else {
                                    format!("{:?}", http_response_body)
                                },
                            );
                        }
                    }
                }
            }
        }
    }
}

fn render_via_get_command(
    content_directory: &ContentDirectory,
    route: &str,
    accept: &str,
) -> Output {
    let mut command = operator_command(&[
        "get",
        &format!(
            "--content-directory={}",
            content_directory
                .root()
                .to_str()
                .expect("Content directory root path was not UTF-8")
        ),
        &format!("--route={}", route),
        &format!("--accept={}", accept),
    ]);

    command.output().expect("Failed to execute process")
}

async fn render_via_http_request(
    server_address: &SocketAddr,
    route: &str,
    accept: &str,
) -> (StatusCode, Result<Bytes, PayloadError>) {
    let request = HttpClient::new()
        .get(format!("http://{}/{}", server_address, route))
        .header("accept", accept)
        .timeout(time::Duration::from_secs(15));

    match request.send().await {
        Err(send_request_error) => panic!(
            "Failed while sending request for http://{}/{}: {}",
            server_address, route, send_request_error,
        ),
        Ok(response) => {
            let response_status = response.status();
            let response_body = collect_response_body(response).await;
            (response_status, response_body)
        }
    }
}

async fn collect_response_body<S>(response: ClientResponse<S>) -> Result<Bytes, PayloadError>
where
    S: Stream<Item = Result<Bytes, PayloadError>> + Unpin,
{
    response
        .try_fold(BytesMut::new(), |mut accumulator, bytes| {
            accumulator.extend_from_slice(&bytes);
            async { Ok(accumulator) }
        })
        .await
        .map(BytesMut::freeze)
}

fn is_omitted_from_snapshots(route: &str) -> bool {
    route.starts_with("NO-SNAPSHOT-") || route.contains("/NO-SNAPSHOT-")
}

/// RenderContext::into_error_context was flagged as unused from this crate
/// (because it is). Tooling complains about this even though it's used from
/// the main crate, so here's a function to "use" it.
#[allow(dead_code)]
fn use_into_error_context() {
    use content::{ContentEngine, FilesystemBasedContentEngine};
    use std::path::Path;
    let shared_engine = FilesystemBasedContentEngine::<(), ()>::from_content_directory(
        ContentDirectory::from_root(&Path::new("/dev/null")).unwrap(),
        (),
    )
    .unwrap();
    let engine = shared_engine.read().unwrap();
    let context = engine.get_render_context("");
    context.into_error_context(());
}

// The rest of this file is the actual tests.

#[actix_rt::test]
async fn samples_match_snapshots() {
    for content_directory in sample_content_directories() {
        let content_directory_root = &content_directory.root();

        let log_prefix_regex = {
            let datetime_pattern = r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}.\d+[-+]\d{2}:\d{2}";
            let log_level_pattern = r"ERROR|WARN|INFO|DEBUG|TRACE";
            let log_prefix_pattern =
                format!("^({}) - ({}) - ", datetime_pattern, log_level_pattern);
            Regex::new(&log_prefix_pattern).unwrap()
        };

        let unordered_content = render_everything_for_snapshots(&content_directory).await;
        let contents = unordered_content
            .iter()
            // Files can be omitted from snapshots with a naming convention.
            .filter(|(key, _)| !is_omitted_from_snapshots(key))
            // If dynamic content files mention where the repo is checked
            // out, redact it to keep tests portable.
            .map(|(key, value)| (key, value.replace(PROJECT_DIRECTORY, "$PROJECT_DIRECTORY")))
            // Also remove the prefix used on log messages.
            .map(|(key, value)| (key, String::from(log_prefix_regex.replace_all(&value, ""))))
            .collect::<BTreeMap<_, _>>();

        let mut insta_settings = insta::Settings::clone_current();
        insta_settings.set_snapshot_path(sample_path("_snapshots"));
        insta_settings.set_input_file(content_directory_root);
        let sample_name = content_directory_root
            .strip_prefix(sample_path("."))
            .expect("Failed to strip samples directory prefix")
            .to_str()
            .expect("Sample path is not UTF-8");
        insta_settings.bind(|| insta::assert_yaml_snapshot!(sample_name, contents));
    }
}

#[test]
fn missing_subcommand_is_error() {
    let mut command = operator_command(&[] as &[&str]);
    let output = command.output().expect("Failed to execute process");

    assert!(
        !output.status.success(),
        "Executing `{:?}` succeeded when it should have failed",
        command
    );
}

#[test]
fn invalid_subcommand_is_error() {
    let mut command = operator_command(&["invalid-subcommand"]);
    let output = command.output().expect("Failed to execute process");

    assert!(
        !output.status.success(),
        "Executing `{:?}` succeeded when it should have failed",
        command
    );
}

#[test]
fn eval_subcommand_succeeds() {
    let input = "{{#if true}}hello world{{/if}}";
    let expected_output = "hello world";

    let mut command = operator_command(&["eval", "--content-directory=/dev/null"]);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("Failed to spawn process");
    child
        .stdin
        .as_mut()
        .expect("Failed to capture child process stdin")
        .write_all(input.as_bytes())
        .expect("Failed to write to child process stdin");
    let output = child.wait_with_output().expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Executing `{:?}` failed when it should have succeeded",
        command
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("Output was not valid UTF-8"),
        expected_output,
        "Executing `{:?}` did not produce the expected output",
        command
    );
}

#[test]
fn get_subcommand_succeeds() {
    let expected_output = "hello world";

    let mut command = operator_command(&[
        "get",
        &format!(
            "--content-directory={}",
            &sample_path("hello-world").to_str().unwrap()
        ),
        "--route=hello",
        "--accept=text/*",
    ]);
    let output = command.output().expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Executing `{:?}` failed when it should have succeeded: {}",
        command,
        String::from_utf8(output.stderr).unwrap_or(String::from(
            "Unable to display error message because stderr was not UTF-8"
        ))
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("Output was not valid UTF-8"),
        expected_output,
        "Executing `{:?}` did not produce the expected output",
        command
    );
}

#[actix_rt::test]
async fn serve_subcommand_succeeds() {
    let content_directory = ContentDirectory::from_root(&sample_path("hello-world")).unwrap();
    let server = RunningServer::start(&content_directory).expect("Server failed to start");

    let expected_response_body = "hello world";

    let request = HttpClient::new()
        .get(format!("http://{}/hello", server.address()))
        .header(
            "accept",
            "application/msword, text/*;q=0.9, image/gif;q=0.1",
        );

    let mut response = request.send().await.expect("Unable to send HTTP request");
    let response_body = response.body().await.expect("Unable to get response body");
    let response_content_type = response
        .headers()
        .get("content-type")
        .expect("Response was missing content-type header");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Response status was not 200 OK"
    );
    assert_eq!(
        response_content_type, "text/plain",
        "Response content-type was not text/plain",
    );
    assert_eq!(
        response_body, expected_response_body,
        "Response body was incorrect"
    );
}
