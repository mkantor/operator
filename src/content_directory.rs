use std::fs;
use std::io;
use std::path;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;
use walkdir::{DirEntry, WalkDir};

#[derive(Error, Debug)]
pub enum ContentDirectoryFromRootError {
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
    pub fn from_root<P: AsRef<Path>>(root: &P) -> Result<Self, ContentDirectoryFromRootError> {
        let root_path = root.as_ref();
        let entries = WalkDir::new(root_path)
            .follow_links(true)
            .min_depth(1)
            .into_iter()
            .filter_map(|dir_entry_result| match dir_entry_result {
                Err(walkdir_error) => Some(Err(ContentDirectoryFromRootError::WalkDirError {
                    source: walkdir_error,
                    root: PathBuf::from(root_path),
                })),
                Ok(entry) if entry.file_type().is_file() => Some(
                    ContentFile::from_root_and_walkdir_entry(root_path, entry)
                        .map_err(ContentDirectoryFromRootError::from),
                ),
                Ok(_non_file_entry) => None,
            })
            .collect::<Result<Vec<ContentFile>, ContentDirectoryFromRootError>>()?;

        Ok(ContentDirectory { files: entries })
    }
}

pub struct ContentFile {
    relative_path: String,
    relative_path_components: Vec<String>,
    file: fs::File,
}
impl ContentFile {
    pub const PATH_SEPARATOR: char = '/';

    fn from_root_and_walkdir_entry(
        root: &Path,
        walkdir_entry: DirEntry,
    ) -> Result<Self, ContentFileError> {
        if path::MAIN_SEPARATOR != Self::PATH_SEPARATOR {
            return Err(ContentFileError {
                message: format!(
                    "Platforms that use '{}' as a path separator are not supported",
                    path::MAIN_SEPARATOR
                ),
            });
        }

        let root = match root.to_str() {
            Some(unicode_root) => unicode_root,
            None => {
                return Err(ContentFileError {
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
                .map_err(|strip_prefix_error| ContentFileError {
                    message: format!(
                        "Content file path '{}' did not start with expected prefix '{}': {}",
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
                            None => Err(ContentFileError {
                                message: format!(
                                    "Non-unicode file/directory name in '{}' (relative path is similar to '{}')",
                                    root,
                                    relative_path.display(),
                                )
                            }),
                            Some("") => Err(ContentFileError {
                                message: format!(
                                    "The path '{}' in '{}' has an empty file/directory name component",
                                    relative_path.display(),
                                    root,
                                )
                            }),
                            Some(nonempty_str) => {
                                if nonempty_str.contains(Self::PATH_SEPARATOR) {
                                    Err(ContentFileError {
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
                        Err(ContentFileError {
                            message: format!(
                                "Unable to create a content file from '{}' in '{}' due to an unsupported path component ({:?})",
                                relative_path.display(),
                                root,
                                unsupported_component,
                            )
                        })
                    }
                }
            })
            .collect::<Result<Vec<String>, ContentFileError>>()?;

        if !walkdir_entry.file_type().is_file() {
            Err(ContentFileError {
                message: format!(
                    "Path '{}' in '{}' does not refer to a file",
                    relative_path.display(),
                    root
                ),
            })
        } else {
            let file =
                fs::File::open(walkdir_entry.path()).map_err(|io_error| ContentFileError {
                    message: format!(
                        "Unable to open file '{}' in '{}' for reading: {}",
                        relative_path.display(),
                        root,
                        io_error
                    ),
                })?;
            Ok(ContentFile {
                relative_path: relative_path_components.join(&Self::PATH_SEPARATOR.to_string()),
                relative_path_components,
                file,
            })
        }
    }

    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    pub fn relative_path_components(&self) -> &[String] {
        &self.relative_path_components
    }

    pub fn file_contents(self) -> impl io::Read {
        self.file
    }

    pub fn split_relative_path_extension(&self) -> Option<(&str, &str)> {
        let mut parts = self.relative_path.rsplitn(2, '.');
        let extension = parts.next();
        let prefix = parts.next();
        prefix.zip(extension)
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

    #[test]
    fn directory_can_be_created_from_valid_root() {
        let path = "./src";
        let result = ContentDirectory::from_root(&path);
        assert!(
            result.is_ok(),
            "Unable to use directory at '{}': {}",
            path,
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
}
