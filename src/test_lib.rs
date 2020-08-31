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

pub fn sample_content_directories() -> Vec<ContentDirectory> {
    vec![
        // The "realistic-advanced" directory is intentionally left out. It
        // contains a lot of non-deterministic output and some large files
        // which bog down the tests.
        sample_content_directory("alternative-representations"),
        sample_content_directory("empty"),
        sample_content_directory("error-handling"),
        sample_content_directory("executables"),
        sample_content_directory("hello-world"),
        sample_content_directory("hidden-content"),
        sample_content_directory("media-types"),
        sample_content_directory("multimedia"),
        sample_content_directory("partials"),
        sample_content_directory("render-context"),
        sample_content_directory("static-content"),
        sample_content_directory("invalid-duplicate-media-type-1"),
        sample_content_directory("invalid-duplicate-media-type-2"),
        sample_content_directory("invalid-duplicate-media-type-3"),
        sample_content_directory("invalid-single-extension-executable"),
        sample_content_directory("invalid-template-that-is-executable"),
        sample_content_directory("invalid-templates"),
        sample_content_directory("invalid-three-extensions-executable"),
        sample_content_directory("invalid-three-extensions-not-executable"),
        sample_content_directory("invalid-two-extensions-not-template-or-executable"),
        sample_content_directory("invalid-unsupported-static-file"),
    ]
}

pub fn sample_content_directories_with_valid_contents() -> Vec<ContentDirectory> {
    sample_content_directories()
        .into_iter()
        .filter(|content_directory| {
            sample_path_is_for_valid_content_directory(&content_directory.root())
        })
        .collect()
}

pub fn sample_content_directories_with_invalid_contents() -> Vec<ContentDirectory> {
    sample_content_directories()
        .into_iter()
        .filter(|content_directory| {
            !sample_path_is_for_valid_content_directory(&content_directory.root())
        })
        .collect()
}

pub fn arbitrary_content_directory_with_valid_content() -> ContentDirectory {
    sample_content_directory("hello-world")
}

pub fn sample_path(relative_path: &str) -> PathBuf {
    [PROJECT_DIRECTORY, "samples", relative_path]
        .iter()
        .collect()
}

pub fn sample_content_directory(relative_path: &str) -> ContentDirectory {
    let root = sample_path(relative_path);
    ContentDirectory::from_root(&root).expect(&format!(
        "Test fixture data is broken in path '{}'",
        root.display()
    ))
}

/// By convention, samples whose root folder start with "invalid-" are ones
/// that should fail when used to instantiate a ContentEngine.
fn sample_path_is_for_valid_content_directory(root: &Path) -> bool {
    let prefix_path_for_invalid = sample_path("invalid-");

    let prefix_str_for_invalid = prefix_path_for_invalid
        .to_str()
        .expect("Sample path was not UTF-8");
    let root_str = root.to_str().expect("Sample path was not UTF-8");

    !root_str.starts_with(prefix_str_for_invalid)
}
