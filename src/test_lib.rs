#![cfg(test)]

use crate::content_directory::ContentDirectory;
use std::path::PathBuf;

pub use crate::lib::*;

pub const PROJECT_DIRECTORY: &str = env!("CARGO_MANIFEST_DIR");

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
        example_content_directory("valid/media-types"),
        example_content_directory("valid/changing-context"),
        example_content_directory("valid/executables"),
    ]
}

pub fn content_directories_with_invalid_contents() -> Vec<ContentDirectory> {
    vec![
        example_content_directory("invalid/invalid-templates"),
        example_content_directory("invalid/unsupported-static-file"),
        example_content_directory("invalid/single-extension-executable"),
        example_content_directory("invalid/two-extensions-not-template-or-executable"),
        example_content_directory("invalid/template-that-is-executable"),
        example_content_directory("invalid/three-extensions-not-executable"),
        example_content_directory("invalid/three-extensions-executable"),
    ]
}

pub fn arbitrary_content_directory_with_valid_content() -> ContentDirectory {
    example_content_directory("valid/hello-world")
}

pub fn example_path(relative_path: &str) -> PathBuf {
    [PROJECT_DIRECTORY, "examples", relative_path]
        .iter()
        .collect()
}

pub fn example_content_directory(relative_path: &str) -> ContentDirectory {
    let root = example_path(relative_path);
    ContentDirectory::from_root(&root).expect(&format!(
        "Test fixture data is broken in path '{}'",
        root.display()
    ))
}
