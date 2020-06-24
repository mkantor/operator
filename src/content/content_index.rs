use crate::content_directory::ContentFile;
use serde::Serialize;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
#[error("Unable to update index: {}", .message)]
pub struct ContentIndexUpdateError {
    message: String,
}

#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum ContentIndex {
    File(CanonicalAddress),
    Directory(ContentIndexEntries),
}

#[derive(Clone, Hash, Eq, PartialEq, Serialize)]
pub struct CanonicalAddress(String);
impl CanonicalAddress {
    pub fn new<C: AsRef<str>>(canonical_address: C) -> Self {
        CanonicalAddress(String::from(canonical_address.as_ref()))
    }
}

#[derive(Clone, Serialize)]
pub struct ContentIndexEntries(HashMap<String, ContentIndex>);
impl ContentIndexEntries {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn try_add<C: AsRef<str>>(
        &mut self,
        canonical_address: C,
    ) -> Result<(), ContentIndexUpdateError> {
        let (dirname_components, basename) = {
            let mut path_components = canonical_address
                .as_ref()
                .split(ContentFile::PATH_SEPARATOR);
            let basename = path_components.next_back();
            (path_components, basename)
        };

        match basename {
            None => Ok(()), // Successfully inserted nothing! 🎉
            Some(basename) => {
                // Navigate to the correct place in the index by iterating path
                // components (except the last one), creating new directories
                // along the way as needed (think mkdir -p).
                let mut node = self;
                for path_component in dirname_components {
                    let next_node = node
                        .0
                        .entry(String::from(path_component))
                        .or_insert_with(|| ContentIndex::Directory(Self::new()));

                    node = match next_node {
                        ContentIndex::Directory(branch) => branch,
                        ContentIndex::File(CanonicalAddress(conficting_address)) => {
                            return Err(ContentIndexUpdateError {
                              message: format!(
                                "Unable to add address '{}' because there is already a file at '{}', but that is a directory in the new address.",
                                canonical_address.as_ref(),
                                conficting_address,
                              )
                            })
                        }
                    };
                }

                // Use the last path component to insert a file in the index.
                match node.0.get(basename) {
                    Some(existing_entry) => {
                        let entry_description = match existing_entry {
                            ContentIndex::Directory(..) => "directory",
                            ContentIndex::File(..) => "file",
                        };
                        Err(
                      ContentIndexUpdateError {
                        message: format!(
                          "Unable to add address '{}' because there is already a {} at that address.",
                          canonical_address.as_ref(),
                          entry_description,
                        )
                      })
                    }
                    None => {
                        node.0.entry(String::from(basename)).or_insert_with(|| {
                            ContentIndex::File(CanonicalAddress::new(canonical_address))
                        });
                        Ok(())
                    }
                }
            }
        }
    }
}
