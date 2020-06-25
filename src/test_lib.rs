#![cfg(test)]

use crate::content_directory::ContentDirectory;
use std::path::PathBuf;

pub use crate::lib::*;

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

pub fn content_directories_with_valid_contents() -> Vec<ContentDirectory> {
    vec![
        example_content_directory("valid/hello-world"),
        example_content_directory("valid/partials"),
        example_content_directory("valid/empty"),
        example_content_directory("valid/static-content"),
    ]
}

pub fn content_directories_with_invalid_contents() -> Vec<ContentDirectory> {
    vec![
        example_content_directory("invalid/invalid-templates"),
        example_content_directory("invalid/unsupported-static-file"),
    ]
}

pub fn arbitrary_content_directory_with_valid_content() -> ContentDirectory {
    example_content_directory("valid/hello-world")
}

pub fn example_path(relative_path: &str) -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "examples", relative_path]
        .iter()
        .collect()
}

fn example_content_directory(relative_path: &str) -> ContentDirectory {
    let root = example_path(relative_path);
    ContentDirectory::from_root(&root).expect(&format!(
        "Test fixture data is broken in path '{}'",
        root.display()
    ))
}
