use actix_web::client::Client as HttpClient;
use actix_web::http::StatusCode;
use actix_web::test::unused_addr;
use std::env;
use std::ffi::OsStr;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time;

const PROJECT_DIRECTORY: &str = env!("CARGO_MANIFEST_DIR");

fn example_path(relative_path: &str) -> PathBuf {
    [PROJECT_DIRECTORY, "src", "examples", relative_path]
        .iter()
        .collect()
}

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
        "--source-media-type=text/plain",
        "--target-media-type=text/plain",
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
        "--target-media-type=text/html",
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
        .header("accept", "text/html;q=0.9, text/plain;q=0.1");

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
