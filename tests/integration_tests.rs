use std::env;
use std::ffi::OsStr;
use std::process::Command;

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
