use super::content_engine::CanonicalRoute;
use crate::content_directory::ContentFile;
use serde::Serialize;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
#[error("Failed to add route '{}' to index: {}", .failed_route, .message)]
pub struct ContentIndexUpdateError {
    failed_route: String,
    message: String,
}

/// A hierarchial tree mapping out content in the registry. Does not actually
/// contain content items, just their routes.
///
/// For example, given the following content directory:
///
/// ```text
/// content/
///   foo.txt
///   bar.html
///   bar/
///     plugh.md.hbs
///     baz/
///       quux.gif
/// ```
///
/// The content index would be:
///
/// ```yaml
/// foo: foo
/// bar: bar
/// bar/:
///   plugh: bar/plugh
///   baz/:
///     quux: bar/baz/quux
/// ```
#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum ContentIndex {
    File(CanonicalRoute),
    Directory(ContentIndexEntries),
}

#[derive(Clone, Serialize)]
pub struct ContentIndexEntries(HashMap<String, ContentIndex>);
impl ContentIndexEntries {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn try_add<C: AsRef<str>>(
        &mut self,
        canonical_route: C,
    ) -> Result<(), ContentIndexUpdateError> {
        let (dirname_components, basename) = {
            let mut path_components = canonical_route.as_ref().split(ContentFile::PATH_SEPARATOR);
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
                        // Non-leaf nodes in the index end with `/` (they
                        // represent directories).
                        .entry(format!("{}/", path_component))
                        .or_insert_with(|| ContentIndex::Directory(Self::new()));

                    node = match next_node {
                        ContentIndex::Directory(branch) => branch,
                        ContentIndex::File(CanonicalRoute(conficting_route)) => {
                            // Each component in dirname_components represents
                            // a directory along the path
                            return Err(ContentIndexUpdateError {
                            failed_route: String::from(canonical_route.as_ref()),
                              message: format!(
                                "There is already a file at '{}', but that needs to be a directory to accommodate the new route.",
                                conficting_route,
                              )
                            });
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
                        Err(ContentIndexUpdateError {
                            failed_route: String::from(canonical_route.as_ref()),
                            message: format!(
                                "There is already a {} at '{}'.",
                                entry_description,
                                canonical_route.as_ref(),
                            ),
                        })
                    }
                    None => {
                        node.0.entry(String::from(basename)).or_insert_with(|| {
                            ContentIndex::File(CanonicalRoute::new(canonical_route))
                        });
                        Ok(())
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn index_has_the_correct_structure() {
        let mut index = ContentIndexEntries::new();
        index.try_add("foo").unwrap();
        index.try_add("bar").unwrap();
        index.try_add("bar/plugh").unwrap();
        index.try_add("bar/baz/quux").unwrap();

        let actual_json = serde_json::to_value(index).unwrap();
        let expected_json = json!({
            "foo": "foo",
            "bar": "bar",
            "bar/": {
              "plugh": "bar/plugh",
              "baz/": {
                "quux": "bar/baz/quux"
              }
            }
        });
        assert_eq!(actual_json, expected_json);
    }
}
