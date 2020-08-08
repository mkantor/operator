use actix_web::client::Client as HttpClient;
use actix_web::http::StatusCode;
use actix_web::test::unused_addr;
use content::ContentDirectory;
use mime_guess::MimeGuess;
use regex::Regex;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::hash::Hasher;
use std::io::Write;
use std::process::{Command, Stdio};
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

fn soliton_command<I, S>(args: I) -> Command
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

    let bin_path = target_dir.join(format!("soliton{}", env::consts::EXE_SUFFIX));

    let mut soliton = Command::new(bin_path);
    soliton.args(args);
    soliton
}

/// Attempts to render all non-hidden files in ContentDirectory, returning
/// them as a map of Route -> RenderedContent | ErrorMessage.
fn render_everything(content_directory: ContentDirectory) -> HashMap<String, String> {
    let mut content = HashMap::new();
    for content_file in &content_directory {
        if !content_file.is_hidden() {
            let route = content_file.relative_path_without_extensions();
            let empty_string = String::from("");
            let first_filename_extension =
                content_file.extensions().first().unwrap_or(&empty_string);

            // Target media type is just the source media type.
            let target_media_type = MimeGuess::from_ext(first_filename_extension)
                .first()
                .unwrap_or(mime::STAR_STAR);

            let result =
                render_one_thing(&content_directory, route, &target_media_type.to_string());

            let output_or_error_message = match result {
                Ok(output) => {
                    let hash = {
                        let mut hasher = DefaultHasher::new();
                        hasher.write(&output);
                        hasher.finish()
                    };
                    match String::from_utf8(output) {
                        Ok(string) => string,
                        Err(_) => format!("binary data with hash {:x}", hash),
                    }
                }
                Err(error_bytes) => {
                    String::from_utf8(error_bytes).expect("Error message was not UTF-8")
                }
            };

            content.insert(
                String::from(content_file.relative_path()),
                output_or_error_message,
            );
        }
    }
    content
}

fn render_one_thing(
    content_directory: &ContentDirectory,
    route: &str,
    accept: &str,
) -> Result<Vec<u8>, Vec<u8>> {
    let mut command = soliton_command(&[
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
    let output = command.output().expect("Failed to execute process");

    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(output.stderr)
    }
}

#[test]
fn examples_match_snapshots() {
    for content_directory in example_content_directories() {
        let content_directory_root = &content_directory.root();

        let log_prefix_regex = {
            let datetime_pattern = r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}.\d+[-+]\d{2}:\d{2}";
            let log_level_pattern = r"ERROR|WARN|INFO|DEBUG|TRACE";
            let log_prefix_pattern =
                format!("^({}) - ({}) - ", datetime_pattern, log_level_pattern);
            Regex::new(&log_prefix_pattern).unwrap()
        };

        let unordered_content = render_everything(content_directory);
        let contents = unordered_content
            .iter()
            // If dynamic content files mention where the repo is checked
            // out, redact it to keep tests portable.
            .map(|(key, value)| (key, value.replace(PROJECT_DIRECTORY, "$PROJECT_DIRECTORY")))
            // Also remove the prefix used on log messages.
            .map(|(key, value)| (key, String::from(log_prefix_regex.replace_all(&value, ""))))
            // Files can be omitted from snapshots with a naming convention.
            .filter(|(key, _)| !key.starts_with("SKIP-SNAPSHOT-"))
            .collect::<BTreeMap<_, _>>();

        let mut insta_settings = insta::Settings::clone_current();
        insta_settings.set_input_file(content_directory_root);
        let id = content_directory_root
            .strip_prefix(example_path("."))
            .unwrap()
            .to_string_lossy();
        insta_settings.set_snapshot_suffix(id);
        insta_settings.bind(|| insta::assert_yaml_snapshot!(contents));
    }
}

#[test]
fn missing_subcommand_is_error() {
    let mut command = soliton_command(&[] as &[&str]);
    let output = command.output().expect("Failed to execute process");

    assert!(
        !output.status.success(),
        "Executing `{:?}` succeeded when it should have failed",
        command
    );
}

#[test]
fn invalid_subcommand_is_error() {
    let mut command = soliton_command(&["invalid-subcommand"]);
    let output = command.output().expect("Failed to execute process");

    assert!(
        !output.status.success(),
        "Executing `{:?}` succeeded when it should have failed",
        command
    );
}

#[test]
fn render_subcommand_succeeds() {
    let input = "{{#if true}}hello world{{/if}}";
    let expected_output = "hello world";

    let mut command = soliton_command(&[
        "render",
        "--content-directory=/dev/null",
        "--media-type=text/plain",
    ]);
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
    let expected_output = "hello world\n";

    let mut command = soliton_command(&[
        "get",
        &format!(
            "--content-directory={}",
            &example_path("hello-world").to_str().unwrap()
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
    let server_address = unused_addr();

    let expected_response_body = "hello world\n";
    let mut command = soliton_command(&[
        "serve",
        &format!(
            "--content-directory={}",
            &example_path("hello-world").to_str().unwrap()
        ),
        &format!("--socket-address={}", server_address),
        "--index-route=hello",
    ]);
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let mut child = command.spawn().expect("Failed to spawn process");

    let request = HttpClient::new()
        .get(format!("http://{}/", server_address))
        .header(
            "accept",
            "application/msword, text/*;q=0.9, image/gif;q=0.1",
        );

    // Give the server a chance to start up before sending the request.
    // TODO: Would be better to poll by retrying a few times if the connection
    // is refused.
    thread::sleep(time::Duration::from_millis(500));

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
        response_content_type, "text/html",
        "Response content-type was not text/html",
    );
    assert_eq!(
        response_body, expected_response_body,
        "Response body was incorrect"
    );

    child.kill().expect("Failed to kill server");
}
