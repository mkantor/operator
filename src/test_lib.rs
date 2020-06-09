#![cfg(test)]

use std::path::{Path, PathBuf};

pub const VALID_TEMPLATES: [(&str, &str); 2] = [
    ("hello world", "hello world"),
    (
        "{{#if true}}hello world{{else}}goodbye world{{/if}}",
        "hello world",
    ),
];

pub const INVALID_TEMPLATES: [&str; 3] = [
    "{{this is not valid handlebars!}}",
    "{{",
    "{{#if this is not legit}}",
];

pub const CONTENT_DIRECTORY_PATHS_WITH_VALID_CONTENTS: [&str; 3] = [
    concat!(env!("CARGO_MANIFEST_DIR"), "/examples/valid/hello-world"),
    concat!(env!("CARGO_MANIFEST_DIR"), "/examples/valid/partials"),
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/valid/examples/valid/empty"
    ),
];

pub const CONTENT_DIRECTORY_PATHS_WITH_INVALID_CONTENTS: [&str; 1] = [concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/invalid/invalid-templates"
)];

pub fn example_path(relative_path: &str) -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "examples", relative_path]
        .iter()
        .collect()
}

pub fn arbitrary_content_directory_path_with_valid_content() -> &'static Path {
    Path::new(CONTENT_DIRECTORY_PATHS_WITH_VALID_CONTENTS[0])
}
