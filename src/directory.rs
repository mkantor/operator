use std::path::{Path, PathBuf};
use thiserror::Error;
use walkdir::{DirEntry, WalkDir};

#[derive(Error, Debug)]
#[error("Unable to use directory root.")]
pub struct DirectoryFromRootError {
    #[from]
    source: walkdir::Error,
}

#[derive(Clone)]
pub struct Directory {
    root: PathBuf,
    entries: Vec<DirEntry>,
}

impl Directory {
    pub fn from_root<P: AsRef<Path>>(root: &P) -> Result<Self, DirectoryFromRootError> {
        let root_path = root.as_ref();
        WalkDir::new(root_path)
            .into_iter()
            .collect::<Result<Vec<DirEntry>, walkdir::Error>>()
            .map(|entries| Directory {
                root: PathBuf::from(root_path),
                entries,
            })
            .map_err(DirectoryFromRootError::from)
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }
}

impl IntoIterator for Directory {
    type Item = DirEntry;
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
        let result = Directory::from_root(&".");
        assert!(result.is_ok());
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
