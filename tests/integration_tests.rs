mod lib;

use actix_web::client::Client as HttpClient;
use actix_web::http::StatusCode;
use lib::*;
use operator::content::ContentDirectory;
use operator::test_lib::*;
use regex::Regex;
use std::collections::BTreeMap;
use std::env;
use std::io::Write;
use std::process::Stdio;
use std::str;
use test_env_log::test;

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
            // Also redact release modes in paths so we can release with
            // snapshots generated during debug builds.
            .map(|(key, value)| (key, value.replace("target/debug", "target/$PROFILE")))
            .map(|(key, value)| (key, value.replace("target/release", "target/$PROFILE")))
            // Also remove the prefix used on log messages.
            .map(|(key, value)| (key, String::from(log_prefix_regex.replace_all(&value, ""))))
            .collect::<BTreeMap<_, _>>();

        let mut insta_settings = insta::Settings::clone_current();
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
        "--route=/hello",
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
            "Accept",
            "application/msword, text/*;q=0.9, image/gif;q=0.1",
        );

    let mut response = request.send().await.expect("Unable to send HTTP request");
    let response_body = response.body().await.expect("Unable to get response body");
    let response_content_type = response
        .headers()
        .get("Content-Type")
        .expect("Response was missing Content-Type header");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Response status was not 200 OK"
    );
    assert_eq!(
        response_content_type, "text/plain",
        "Response Content-Type was not text/plain",
    );
    assert_eq!(
        response_body, expected_response_body,
        "Response body was incorrect"
    );
}
