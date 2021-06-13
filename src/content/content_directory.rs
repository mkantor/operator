use super::Route;
use crate::bug_message;
use std::env;
use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::path;
use std::path::{Path, PathBuf};
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Error, Debug)]
pub enum ContentDirectoryFromRootError {
    #[error("Unable to use directory root '{}': {}", .root.display(), .message)]
    InvalidRootPath { root: PathBuf, message: String },

    #[error("Unable to use directory root '{}': {}", .root.display(), .source)]
    WalkDirError {
        root: PathBuf,
        source: walkdir::Error,
    },

    #[error(transparent)]
    DirectoryEntryError(#[from] ContentFileError),
}

#[derive(Error, Debug)]
#[error("Content file error: {}", .0)]
pub struct ContentFileError(String);

/// A filesystem directory containing content.
pub struct ContentDirectory {
    files: Vec<ContentFile>,
    root: PathBuf,
}

impl ContentDirectory {
    pub fn from_root<P: AsRef<Path>>(
        absolute_root: &P,
    ) -> Result<Self, ContentDirectoryFromRootError> {
        let absolute_root_path = absolute_root.as_ref();
        if !absolute_root_path.is_absolute() {
            return Err(ContentDirectoryFromRootError::InvalidRootPath {
                message: String::from("Root path must be absolute."),
                root: PathBuf::from(absolute_root_path),
            });
        }

        let mut files = Vec::new();
        let walker = WalkDir::new(absolute_root_path)
            .follow_links(true)
            .min_depth(1)
            .into_iter()
            .filter_entry(|entry| {
                // Skip hidden files/directories.
                let is_hidden = entry
                    .file_name()
                    .to_str()
                    .map(|name| name.starts_with('.'))
                    .unwrap_or(false);
                !is_hidden
            });
        for dir_entry_result in walker {
            let dir_entry = dir_entry_result.map_err(|walkdir_error| {
                ContentDirectoryFromRootError::WalkDirError {
                    source: walkdir_error,
                    root: PathBuf::from(absolute_root_path),
                }
            })?;
            {
                let entry_path = dir_entry.path().to_path_buf();
                if dir_entry.file_type().is_file() {
                    let content_file =
                        ContentFile::from_root_and_path(absolute_root_path, entry_path)
                            .map_err(ContentDirectoryFromRootError::from)?;
                    files.push(content_file);
                }
            }
        }

        Ok(ContentDirectory {
            files,
            root: PathBuf::from(absolute_root_path),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

pub struct ContentFile {
    pub route: Route,
    pub absolute_path: String,
    pub relative_path: String,
    pub extensions: Vec<String>,
    pub is_executable: bool,

    // All files are eagerly opened. The benefit is that content can be served
    // quickly (at request time we can immediately start reading from the
    // already-opened file), but the cost is that there can be many file
    // descriptors open at once (so you might need to adjust ulimits to serve
    // large content directories).
    pub file: File,
}
impl ContentFile {
    pub const PATH_SEPARATOR: char = '/';

    fn from_root_and_path(
        content_directory_root: &Path,
        absolute_content_file_path: PathBuf,
    ) -> Result<Self, ContentFileError> {
        if path::MAIN_SEPARATOR != Self::PATH_SEPARATOR {
            return Err(ContentFileError(format!(
                "Platforms that use '{}' as a path separator are not supported",
                path::MAIN_SEPARATOR
            )));
        }

        let root = match content_directory_root.to_str() {
            Some(unicode_root) => unicode_root,
            None => {
                return Err(ContentFileError(format!(
                    "Non-unicode directory root (path is similar to '{}')",
                    content_directory_root.display(),
                )))
            }
        };

        let absolute_path = String::from(
            absolute_content_file_path
                .to_str()
                .ok_or_else(|| ContentFileError(String::from("Path was not unicode.")))?,
        );

        let relative_path = absolute_content_file_path
            .strip_prefix(root)
            .map_err(|strip_prefix_error| {
                ContentFileError(format!(
                    "Content file path '{}' did not start with expected prefix '{}': {}",
                    absolute_content_file_path.display(),
                    root,
                    strip_prefix_error
                ))
            })?
            .to_str()
            .map(String::from)
            .ok_or_else(|| ContentFileError(String::from("Path was not unicode.")))?;

        let file = File::open(&absolute_content_file_path).map_err(|io_error| {
            ContentFileError(format!(
                "Unable to open file '{}' in '{}' for reading: {}",
                relative_path, root, io_error
            ))
        })?;

        let basename = absolute_content_file_path
            .file_name()
            .ok_or_else(|| {
                ContentFileError(format!(
                    "Unable to get basename of '{}' in '{}'",
                    relative_path, root,
                ))
            })?
            .to_str()
            .ok_or_else(|| ContentFileError(String::from("File had a non-unicode basename.")))?;

        // Conventions around hidden files, whether a file is executable, etc
        // differ across platforms. It wouldn't be hard to implement this, but
        // Operator does not currently run its CI checks on non-unix platforms
        // so it would be too easy to introduce regressions.
        let (extensions, is_executable) = if !cfg!(unix) {
            return Err(ContentFileError(format!(
                "Operator does not currently support your operating system ({})",
                env::consts::OS,
            )));
        } else {
            // If the basename begins with `.` its first chunk isn't considered
            // an "extension".
            let non_extension_components = if basename.starts_with('.') { 2 } else { 1 };
            let extensions = basename
                .split('.')
                .skip(non_extension_components)
                .map(String::from)
                .collect::<Vec<String>>();

            let permissions = file
                .metadata()
                .map_err(|io_error| {
                    ContentFileError(format!(
                        "Unable to query metadata for content file '{}': {}",
                        absolute_content_file_path.display(),
                        io_error
                    ))
                })?
                .permissions();
            let is_executable = permissions.mode() & 0o111 != 0;

            (extensions, is_executable)
        };

        let route = {
            let extensions_len = extensions.iter().fold(0, |len, extension| {
                // Extra 1 is to count . in the extensions.
                len + extension.len() + 1
            });
            let relative_path_without_extensions_len = relative_path.len() - extensions_len;
            let relative_path_without_extensions =
                &relative_path[0..relative_path_without_extensions_len];

            let mut route_string = String::with_capacity(relative_path_without_extensions_len + 1);
            route_string.push(Self::PATH_SEPARATOR);
            route_string.push_str(relative_path_without_extensions);

            route_string.parse::<Route>().map_err(|error| {
                ContentFileError(format!(
                    bug_message!("This should never happen: Could not create route from path: {}"),
                    error,
                ))
            })
        }?;

        Ok(ContentFile {
            route,
            absolute_path,
            relative_path,
            extensions,
            is_executable,
            file,
        })
    }
}

impl IntoIterator for ContentDirectory {
    type Item = ContentFile;
    type IntoIter = std::vec::IntoIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.files.into_iter()
    }
}

impl<'a> IntoIterator for &'a ContentDirectory {
    type Item = &'a ContentFile;
    type IntoIter = std::slice::Iter<'a, ContentFile>;
    fn into_iter(self) -> Self::IntoIter {
        self.files.iter()
    }
}

impl PartialEq for ContentDirectory {
    fn eq(&self, other: &Self) -> bool {
        self.root() == other.root()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lib::*;
    use std::fs;
    use test_env_log::test;

    #[test]
    fn directory_can_be_created_from_valid_root() {
        let path = fs::canonicalize("./src").expect("Canonicalizing path failed");
        let result = ContentDirectory::from_root(&path);
        assert!(
            result.is_ok(),
            "Unable to use directory at '{}': {}",
            path.display(),
            result.err().unwrap().to_string()
        );
    }

    #[test]
    fn directory_root_must_exist() {
        let result = ContentDirectory::from_root(&sample_path("this/does/not/actually/exist"));
        assert!(
            result.is_err(),
            "Directory was successfully created from non-existent path",
        );
    }

    #[test]
    fn directory_root_must_be_absolute_path() {
        let non_absolute_path = "./src";
        let result = ContentDirectory::from_root(&non_absolute_path);
        assert!(
            result.is_err(),
            "ContentDirectory was successfully created from non-absolute path '{}', but this should have failed",
            non_absolute_path,
        );
    }
}
