#![cfg(test)]

use crate::content::ContentDirectory;
use std::path::{Path, PathBuf};

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

pub fn example_content_directories() -> Vec<ContentDirectory> {
    vec![
        example_content_directory("alternative-representations"),
        example_content_directory("empty"),
        example_content_directory("executables"),
        example_content_directory("hello-world"),
        example_content_directory("hidden-content"),
        example_content_directory("media-types"),
        example_content_directory("multimedia"),
        example_content_directory("partials"),
        example_content_directory("render-context"),
        example_content_directory("static-content"),
        example_content_directory("invalid-duplicate-media-type-1"),
        example_content_directory("invalid-duplicate-media-type-2"),
        example_content_directory("invalid-duplicate-media-type-3"),
        example_content_directory("invalid-single-extension-executable"),
        example_content_directory("invalid-template-that-is-executable"),
        example_content_directory("invalid-templates"),
        example_content_directory("invalid-three-extensions-executable"),
        example_content_directory("invalid-three-extensions-not-executable"),
        example_content_directory("invalid-two-extensions-not-template-or-executable"),
        example_content_directory("invalid-unsupported-static-file"),
    ]
}

pub fn example_content_directories_with_valid_contents() -> Vec<ContentDirectory> {
    example_content_directories()
        .into_iter()
        .filter(|content_directory| {
            example_path_is_for_valid_content_directory(&content_directory.root())
        })
        .collect()
}

pub fn example_content_directories_with_invalid_contents() -> Vec<ContentDirectory> {
    example_content_directories()
        .into_iter()
        .filter(|content_directory| {
            !example_path_is_for_valid_content_directory(&content_directory.root())
        })
        .collect()
}

pub fn arbitrary_content_directory_with_valid_content() -> ContentDirectory {
    example_content_directory("hello-world")
}

pub fn example_path(relative_path: &str) -> PathBuf {
    [PROJECT_DIRECTORY, "src", "examples", relative_path]
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

/// By convention, examples whose root folder start with "invalid-" are ones
/// that will fail when used to instantiate a ContentEngine.
fn example_path_is_for_valid_content_directory(root: &Path) -> bool {
    let prefix_path_for_invalid = example_path("invalid-");

    let prefix_str_for_invalid = prefix_path_for_invalid
        .to_str()
        .expect("Example path was not UTF-8");
    let root_str = root.to_str().expect("Example path was not UTF-8");

    !root_str.starts_with(prefix_str_for_invalid)
}
