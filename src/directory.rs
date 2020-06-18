use std::fs;
use std::io;
use std::path;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;
use walkdir::{DirEntry, WalkDir};

#[derive(Error, Debug)]
pub enum DirectoryFromRootError {
    #[error("Unable to use directory root '{}': {}", .root.display(), .source)]
    WalkDirError {
        root: PathBuf,
        source: walkdir::Error,
    },

    #[error(transparent)]
    DirectoryEntryError(#[from] DirectoryEntryError),
}

#[derive(Error, Debug)]
#[error("Directory entry error: {}", .message)]
pub struct DirectoryEntryError {
    message: String,
}

pub struct Directory {
    root: PathBuf,
    entries: Vec<DirectoryEntry>,
}

impl Directory {
    pub fn from_root<P: AsRef<Path>>(root: &P) -> Result<Self, DirectoryFromRootError> {
        let root_path = root.as_ref();
        let entries = WalkDir::new(root_path)
            .into_iter()
            .map(|dir_entry_result| match dir_entry_result {
                Err(walkdir_error) => Err(DirectoryFromRootError::WalkDirError {
                    source: walkdir_error,
                    root: PathBuf::from(root_path),
                }),
                Ok(walkdir_entry) => {
                    DirectoryEntry::from_root_and_walkdir_entry(root_path, walkdir_entry)
                        .map_err(DirectoryFromRootError::from)
                }
            })
            .collect::<Result<Vec<DirectoryEntry>, DirectoryFromRootError>>()?;

        Ok(Directory {
            root: PathBuf::from(root_path),
            entries,
        })
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }
}

pub struct DirectoryEntry {
    relative_path: String,
    relative_path_components: Vec<String>,
    metadata: fs::Metadata,
    file_contents: Option<fs::File>,
}
impl DirectoryEntry {
    pub const PATH_SEPARATOR: char = '/';

    fn from_root_and_walkdir_entry(
        root: &Path,
        walkdir_entry: DirEntry,
    ) -> Result<Self, DirectoryEntryError> {
        if path::MAIN_SEPARATOR != Self::PATH_SEPARATOR {
            return Err(DirectoryEntryError {
                message: format!(
                    "Platforms that use '{}' as a path separator are not supported",
                    path::MAIN_SEPARATOR
                ),
            });
        }

        let root = match root.to_str() {
            Some(unicode_root) => unicode_root,
            None => {
                return Err(DirectoryEntryError {
                    message: format!(
                        "Non-unicode directory root (path is similar to '{}')",
                        root.display(),
                    ),
                })
            }
        };

        let relative_path =
            walkdir_entry
                .path()
                .strip_prefix(root)
                .map_err(|strip_prefix_error| DirectoryEntryError {
                    message: format!(
                        "Directory entry '{}' did not start with expected prefix '{}': {}",
                        walkdir_entry.path().display(),
                        root,
                        strip_prefix_error
                    ),
                })?;

        let relative_path_components = relative_path
            .components()
            .map(|component| {
                match component {
                    Component::Normal(normal_component) => {
                        match normal_component.to_str() {
                            None => Err(DirectoryEntryError {
                                message: format!(
                                    "Non-unicode file/directory name in '{}' (relative path is similar to '{}')",
                                    root,
                                    relative_path.display(),
                                )
                            }),
                            Some("") => Err(DirectoryEntryError {
                                message: format!(
                                    "The path '{}' in '{}' has an empty file/directory name component",
                                    relative_path.display(),
                                    root,
                                )
                            }),
                            Some(nonempty_str) => {
                                if nonempty_str.contains(Self::PATH_SEPARATOR) {
                                    Err(DirectoryEntryError {
                                        message: format!(
                                            "Path '{}' in '{}' contains an unsupported character ('{}').",
                                            relative_path.display(),
                                            root,
                                            Self::PATH_SEPARATOR,
                                        )
                                    })
                                } else {
                                    Ok(String::from(nonempty_str))
                                }
                            }
                        }
                    },
                    unsupported_component => {
                        Err(DirectoryEntryError {
                            message: format!(
                                "Unable to create a directory entry from '{}' in '{}' due to an unsupported path component ({:?})",
                                relative_path.display(),
                                root,
                                unsupported_component,
                            )
                        })
                    }
                }
            })
            .collect::<Result<Vec<String>, DirectoryEntryError>>()?;

        let metadata = walkdir_entry
            .metadata()
            .map_err(|io_error| DirectoryEntryError {
                message: format!(
                    "Unable to retrieve metadata for '{}' in '{}': {}",
                    relative_path.display(),
                    root,
                    io_error
                ),
            })?;

        let file_contents = if metadata.is_file() {
            Some(
                fs::File::open(walkdir_entry.path()).map_err(|io_error| DirectoryEntryError {
                    message: format!(
                        "Unable to open file '{}' in '{}' for reading: {}",
                        relative_path.display(),
                        root,
                        io_error
                    ),
                })?,
            )
        } else {
            None
        };

        Ok(DirectoryEntry {
            relative_path: relative_path_components.join(&Self::PATH_SEPARATOR.to_string()),
            relative_path_components,
            metadata,
            file_contents,
        })
    }

    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    pub fn relative_path_components(&self) -> &[String] {
        &self.relative_path_components
    }

    pub fn metadata(&self) -> &fs::Metadata {
        &self.metadata
    }

    pub fn file_contents(self) -> Option<impl io::Read> {
        self.file_contents
    }
}

impl IntoIterator for Directory {
    type Item = DirectoryEntry;
    type IntoIter = std::vec::IntoIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lib::*;

    #[test]
    fn directory_can_be_created_from_valid_root() {
        let path = "./src";
        let result = Directory::from_root(&path);
        assert!(
            result.is_ok(),
            "Unable to use directory at '{}': {}",
            path,
            result.err().unwrap().to_string()
        );
    }

    #[test]
    fn directory_root_must_exist() {
        let result = Directory::from_root(&example_path("this/does/not/actually/exist"));
        assert!(
            result.is_err(),
            "Directory was successfully created from non-existent path",
        );
    }
}
