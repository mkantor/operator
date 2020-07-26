use crate::content::*;
use crate::content_directory::ContentDirectory;
use crate::lib::*;
use mime::Mime;
use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RenderCommandError {
    #[error("Failed to read input.")]
    ReadError { source: io::Error },

    #[error("Unable to load content.")]
    ContentLoadingError {
        #[from]
        source: ContentLoadingError,
    },

    #[error("Unable to parse template from content directory.")]
    RegisteredTemplateParseError {
        #[from]
        source: RegisteredTemplateParseError,
    },

    #[error("Unable to parse template from input.")]
    UnregisteredTemplateParseError {
        #[from]
        source: UnregisteredTemplateParseError,
    },

    #[error("Unable to render content.")]
    ContentRenderingError {
        #[from]
        source: ContentRenderingError,
    },

    #[error("Failed to write output.")]
    WriteError { source: io::Error },
}

#[derive(Error, Debug)]
pub enum GetCommandError {
    #[error("Unable to load content.")]
    ContentLoadingError {
        #[from]
        source: ContentLoadingError,
    },

    #[error("Unable to parse template from content directory.")]
    RegisteredTemplateParseError {
        #[from]
        source: RegisteredTemplateParseError,
    },

    #[error("Content not found at address '{}'.", .address)]
    ContentNotFound { address: String },

    #[error("Unable to render content.")]
    ContentRenderingError {
        #[from]
        source: ContentRenderingError,
    },

    #[error("Failed to write output.")]
    WriteError { source: io::Error },
}

/// Reads a template from `input`, renders it, and writes it to `output`.
pub fn render<I: io::Read, O: io::Write>(
    content_directory: ContentDirectory,
    source_media_type: Mime,
    target_media_type: &Mime,
    soliton_version: SolitonVersion,
    input: &mut I,
    output: &mut O,
) -> Result<(), RenderCommandError> {
    let locked_engine =
        FilesystemBasedContentEngine::from_content_directory(content_directory, soliton_version)?;
    let engine = locked_engine
        .read()
        .expect("RwLock for ContentEngine has been poisoned");

    let mut template = String::new();
    input
        .read_to_string(&mut template)
        .map_err(|source| RenderCommandError::ReadError { source })?;

    let content_item = engine.new_template(&template, source_media_type)?;
    let render_context = engine.get_render_context(target_media_type);
    let rendered_output = content_item.render(&render_context)?;
    write!(output, "{}", rendered_output)
        .map_err(|source| RenderCommandError::WriteError { source })?;

    output
        .flush()
        .map_err(|source| RenderCommandError::WriteError { source })
}

/// Renders an item from the content directory and writes it to `output`.
pub fn get<O: io::Write>(
    content_directory: ContentDirectory,
    address: &str,
    target_media_type: &Mime,
    soliton_version: SolitonVersion,
    output: &mut O,
) -> Result<(), GetCommandError> {
    let locked_engine =
        FilesystemBasedContentEngine::from_content_directory(content_directory, soliton_version)?;
    let engine = locked_engine
        .read()
        .expect("RwLock for ContentEngine has been poisoned");

    let content_item = engine
        .get(address)
        .ok_or(GetCommandError::ContentNotFound {
            address: String::from(address),
        })?;
    let render_context = engine.get_render_context(target_media_type);
    let rendered_output = content_item.render(&render_context)?;
    write!(output, "{}", rendered_output)
        .map_err(|source| GetCommandError::WriteError { source })?;

    output
        .flush()
        .map_err(|source| GetCommandError::WriteError { source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lib::*;
    use mime_guess::MimeGuess;
    use std::collections::{BTreeMap, HashMap};
    use std::str;

    /// Attempts to render all non-hidden files in ContentDirectory, returning
    /// them as a map of Address -> RenderedContent | ErrorMessage.
    fn render_everything(
        content_directory: ContentDirectory,
    ) -> Result<HashMap<String, String>, String> {
        let mut content = HashMap::new();
        let content_directory_root = content_directory.root();
        for content_file in content_directory {
            if !content_file.is_hidden() {
                // Create a separate ContentDirectory that can be consumed by
                // the CLI calls.
                let consumable_content_directory =
                    ContentDirectory::from_root(&content_directory_root).map_err(|error| {
                        format!("Could not create content directory: {:?}", error)
                    })?;
                let address = content_file.relative_path_without_extensions();
                let first_filename_extension = content_file.extensions().first().expect(&format!(
                    "Content file at '{}' does not have a filename extension",
                    content_file.absolute_path()
                ));

                // Target media type is just the source media type. This isn't
                // testing transcoding.
                let target_media_type = MimeGuess::from_ext(first_filename_extension)
                    .first()
                    .unwrap_or(mime::APPLICATION_OCTET_STREAM);

                let mut output = Vec::new();
                let result = get(
                    consumable_content_directory,
                    address,
                    &target_media_type,
                    SolitonVersion("0.0.0"),
                    &mut output,
                );

                let output_or_error_message = match result {
                    Ok(()) => String::from_utf8(output)
                        .map_err(|error| format!("Output was not valid UTF-8: {:?}", error))?,
                    Err(error) => {
                        let anyhow_error = anyhow::Error::from(error);
                        let causes = anyhow_error.chain().map(|error| error.to_string());
                        let message = causes.fold(String::new(), |acc, arg| acc + " " + &arg);
                        format!("Error:{}", message)
                    }
                };

                content.insert(
                    String::from(content_file.relative_path_without_extensions()),
                    output_or_error_message,
                );
            }
        }
        Ok(content)
    }

    #[test]
    fn examples_match_snapshots() {
        for content_directory in example_content_directories() {
            let content_directory_root = &content_directory.root();

            let unordered_content =
                render_everything(content_directory).expect("Fatal error in valid example");
            let contents = unordered_content
                .iter()
                // If dynamic content files mention where the repo is checked
                // out, redact it to keep tests portable.
                .map(|(key, value)| (key, value.replace(PROJECT_DIRECTORY, "$PROJECT_DIRECTORY")))
                // Files can be omitted from snapshots with a naming convention.
                .filter(|(key, _)| !key.ends_with("-SKIP-SNAPSHOT"))
                .collect::<BTreeMap<_, _>>();

            let mut insta_settings = insta::Settings::clone_current();
            insta_settings.set_input_file(content_directory_root);
            let id = content_directory_root
                .strip_prefix(
                    [PROJECT_DIRECTORY, "examples", "valid"]
                        .iter()
                        .collect::<std::path::PathBuf>(),
                )
                .or(content_directory_root.strip_prefix(
                    [PROJECT_DIRECTORY, "examples", "invalid"]
                        .iter()
                        .collect::<std::path::PathBuf>(),
                ))
                .unwrap()
                .to_string_lossy();
            insta_settings.set_snapshot_suffix(id);
            insta_settings.bind(|| insta::assert_yaml_snapshot!(contents));
        }
    }

    #[test]
    fn cli_renders_valid_templates() {
        for &(template, expected_output) in &VALID_TEMPLATES {
            let mut input = template.as_bytes();
            let mut output = Vec::new();
            let directory = arbitrary_content_directory_with_valid_content();
            let result = render(
                directory,
                mime::TEXT_HTML,
                &mime::TEXT_HTML,
                SolitonVersion("0.0.0"),
                &mut input,
                &mut output,
            );

            assert!(
                result.is_ok(),
                "Template rendering failed for `{}`: {}",
                template,
                result.unwrap_err(),
            );
            let output_as_str = str::from_utf8(output.as_slice()).expect("Output was not UTF-8");
            assert_eq!(
                output_as_str,
                expected_output,
                "Template rendering for `{}` did not produce the expected output (\"{}\"), instead got \"{}\"",
                template,
                expected_output,
                output_as_str
            );
        }
    }

    #[test]
    fn cli_fails_to_render_invalid_templates() {
        for &template in &INVALID_TEMPLATES {
            let mut input = template.as_bytes();
            let mut output = Vec::new();
            let directory = arbitrary_content_directory_with_valid_content();
            let result = render(
                directory,
                mime::TEXT_HTML,
                &mime::TEXT_HTML,
                SolitonVersion("0.0.0"),
                &mut input,
                &mut output,
            );

            assert!(
                result.is_err(),
                "Template rendering succeeded for `{}`, but it should have failed",
                template,
            );
        }
    }

    #[test]
    fn cli_can_get_content() {
        let mut output = Vec::new();
        let address = "hello";
        let expected_output = "hello world\n";

        let directory = arbitrary_content_directory_with_valid_content();
        let result = get(
            directory,
            address,
            &mime::TEXT_HTML,
            SolitonVersion("0.0.0"),
            &mut output,
        );

        assert!(
            result.is_ok(),
            "Template rendering failed for content at '{}': {}",
            address,
            result.unwrap_err(),
        );
        let output_as_str = str::from_utf8(output.as_slice()).expect("Output was not UTF-8");
        assert_eq!(
            output_as_str,
            expected_output,
            "Template rendering for content at '{}' did not produce the expected output (\"{}\"), instead got \"{}\"",
            address,
            expected_output,
            output_as_str
        );
    }

    #[test]
    fn cli_can_fail_to_get_content_which_does_not_exist() {
        let mut output = Vec::new();
        let address = "this-address-does-not-refer-to-any-content";

        let directory = arbitrary_content_directory_with_valid_content();
        let result = get(
            directory,
            address,
            &mime::TEXT_HTML,
            SolitonVersion("0.0.0"),
            &mut output,
        );

        match result {
            Ok(_) => panic!(
                "Getting content from '{}' succeeded, but it should have failed",
                address
            ),
            Err(GetCommandError::ContentNotFound {
                address: address_from_error,
            }) => assert_eq!(
                address_from_error, address,
                "Address from error did not match address used"
            ),
            Err(_) => panic!("Wrong type of error was produced, expected ContentNotFound"),
        };
    }

    #[test]
    fn cli_provides_target_media_type() {
        let mut output = Vec::new();
        let directory = ContentDirectory::from_root(&example_path("valid/media-types")).unwrap();
        let address = "echo-target-media-type";

        let media_type = mime::TEXT_HTML;

        let result = get(
            directory,
            address,
            &media_type,
            SolitonVersion("0.0.0"),
            &mut output,
        );
        assert!(
            result.is_ok(),
            "Template rendering failed for content at '{}': {}",
            address,
            result.unwrap_err(),
        );

        let output_as_str = str::from_utf8(output.as_slice()).expect("Output was not UTF-8");
        assert_eq!(
            output_as_str,
            media_type.essence_str(),
            "Template rendering for content at '{}' did not produce the expected output (\"{}\"), instead got \"{}\"",
            address,
            media_type.essence_str(),
            output_as_str
        );
    }
}
