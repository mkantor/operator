use std::env;
use std::ffi::OsStr;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

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
