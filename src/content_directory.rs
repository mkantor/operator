use std::fs::File;
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
#[error("Content file error: {}", .message)]
pub struct ContentFileError {
    message: String,
}

pub struct ContentDirectory {
    files: Vec<ContentFile>,
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

        Ok(ContentDirectory { files })
    }
}

pub struct ContentFile {
    relative_path: String,
    is_hidden: bool,
    relative_path_without_extensions: String,
    extensions: Vec<String>,
    file: File,
}
impl ContentFile {
    pub const PATH_SEPARATOR: char = '/';

    fn from_root_and_path(
        content_directory_root: &Path,
        content_file_path: PathBuf,
    ) -> Result<Self, ContentFileError> {
        if path::MAIN_SEPARATOR != Self::PATH_SEPARATOR {
            return Err(ContentFileError {
                message: format!(
                    "Platforms that use '{}' as a path separator are not supported",
                    path::MAIN_SEPARATOR
                ),
            });
        }

        let root = match content_directory_root.to_str() {
            Some(unicode_root) => unicode_root,
            None => {
                return Err(ContentFileError {
                    message: format!(
                        "Non-unicode directory root (path is similar to '{}')",
                        content_directory_root.display(),
                    ),
                })
            }
        };

        let relative_path = content_file_path
            .strip_prefix(root)
            .map_err(|strip_prefix_error| ContentFileError {
                message: format!(
                    "Content file path '{}' did not start with expected prefix '{}': {}",
                    content_file_path.display(),
                    root,
                    strip_prefix_error
                ),
            })?
            .to_str()
            .map(String::from)
            .ok_or_else(|| ContentFileError {
                message: String::from("Path was not unicode."),
            })?;

        let file = File::open(&content_file_path).map_err(|io_error| ContentFileError {
            message: format!(
                "Unable to open file '{}' in '{}' for reading: {}",
                relative_path, root, io_error
            ),
        })?;

        let basename = content_file_path
            .file_name()
            .ok_or_else(|| ContentFileError {
                message: format!(
                    "Unable to get basename of '{}' in '{}'",
                    relative_path, root,
                ),
            })?
            .to_str()
            .ok_or_else(|| ContentFileError {
                message: String::from("File had a non-unicode basename."),
            })?;

        let (extensions, is_hidden) = if basename.starts_with('.') {
            let extensions = basename
                .split('.')
                .skip(2)
                .map(String::from)
                .collect::<Vec<String>>();
            (extensions, true)
        } else {
            let extensions = basename
                .split('.')
                .skip(1)
                .map(String::from)
                .collect::<Vec<String>>();
            (extensions, false)
        };

        let relative_path_without_extensions = String::from({
            let extensions_len = extensions.iter().fold(0, |len, extension| {
                // Extra 1 is to count . in the extensions.
                len + extension.len() + 1
            });
            &relative_path[0..(relative_path.len() - extensions_len)]
        });

        Ok(ContentFile {
            relative_path,
            relative_path_without_extensions,
            extensions,
            file,
            is_hidden,
        })
    }

    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    pub fn is_hidden(&self) -> bool {
        self.is_hidden
    }

    pub fn file_contents(self) -> File {
        self.file
    }

    pub fn relative_path_without_extensions(&self) -> &str {
        &self.relative_path_without_extensions
    }

    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }
}

impl IntoIterator for ContentDirectory {
    type Item = ContentFile;
    type IntoIter = std::vec::IntoIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.files.into_iter()
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
