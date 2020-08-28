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
    #[cfg(test)]
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
            .min_depth(1);
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
            #[cfg(test)]
            root: absolute_root_path.into(),
        })
    }

    #[cfg(test)]
    pub fn root(&self) -> &Path {
        &self.root
    }
}

pub struct ContentFile {
    absolute_path: String,
    relative_path: String,
    is_hidden: bool,
    is_executable: bool,
    relative_path_without_extensions: String,
    extensions: Vec<String>,
    file: File,
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
        let (extensions, is_hidden, is_executable) = if !cfg!(unix) {
            return Err(ContentFileError(format!(
                "Operator does not currently support your operating system ({})",
                env::consts::OS,
            )));
        } else {
            // If the basename begins with `.` its first chunk isn't considered an "extension".
            let non_extension_components = if basename.starts_with('.') { 2 } else { 1 };
            let extensions = basename
                .split('.')
                .skip(non_extension_components)
                .map(String::from)
                .collect::<Vec<String>>();

            // The file is hidden if any of its relative path components starts
            // with a dot.
            let is_hidden = relative_path.starts_with('.')
                || relative_path.contains(&format!("{}.", Self::PATH_SEPARATOR));

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

            (extensions, is_hidden, is_executable)
        };

        let relative_path_without_extensions = String::from({
            let extensions_len = extensions.iter().fold(0, |len, extension| {
                // Extra 1 is to count . in the extensions.
                len + extension.len() + 1
            });
            &relative_path[0..(relative_path.len() - extensions_len)]
        });

        Ok(ContentFile {
            absolute_path,
            relative_path,
            relative_path_without_extensions,
            extensions,
            file,
            is_hidden,
            is_executable,
        })
    }

    pub fn absolute_path(&self) -> &str {
        &self.absolute_path
    }

    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    pub fn is_hidden(&self) -> bool {
        self.is_hidden
    }

    pub fn is_executable(&self) -> bool {
        self.is_executable
    }

    pub fn relative_path_without_extensions(&self) -> &str {
        &self.relative_path_without_extensions
    }

    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    pub fn file(&self) -> &File {
        &self.file
    }

    pub fn into_file(self) -> File {
        self.file
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

#[cfg(test)]
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
        let result = ContentDirectory::from_root(&example_path("this/does/not/actually/exist"));
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
