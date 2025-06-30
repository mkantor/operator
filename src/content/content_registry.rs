use super::content_item::*;
use super::*;
use std::collections::HashMap;

pub struct ContentRegistry(HashMap<Route, ContentRepresentations>);
impl ContentRegistry {
    pub fn new() -> Self {
        ContentRegistry(HashMap::new())
    }

    /// Routes that begin with underscore are ignored for external requests
    /// (they always 404).
    pub fn get(&self, route: &Route) -> Option<&ContentRepresentations> {
        if route.as_ref().contains("/_") {
            None
        } else {
            self.get_internal(route)
        }
    }

    pub fn get_internal(&self, route: &Route) -> Option<&ContentRepresentations> {
        self.0.get(route)
    }

    pub fn entry_or_insert_default(&mut self, key: Route) -> &mut ContentRepresentations {
        self.0.entry(key).or_default()
    }
}

/// Alternative representations of the same resource.
pub type ContentRepresentations = HashMap<MediaType, RegisteredContent>;

/// A renderable item from the content directory.
pub enum RegisteredContent {
    StaticContentItem(StaticContentItem),
    RegisteredTemplate(RegisteredTemplate),
    Executable(Executable),
}

impl Render for ContentRepresentations {
    type Output = Box<dyn ByteStream>;
    fn render<'accept, ServerInfo, Engine, Accept>(
        &self,
        context: RenderContext<ServerInfo, Engine>,
        acceptable_media_ranges: Accept,
    ) -> Result<Media<Self::Output>, RenderError>
    where
        ServerInfo: Clone + Serialize,
        Engine: ContentEngine<ServerInfo>,
        Accept: IntoIterator<Item = &'accept MediaRange>,
        Self::Output: ByteStream,
    {
        let mut errors = Vec::new();
        for acceptable_media_range in acceptable_media_ranges {
            for (registered_media_type, content) in self {
                if registered_media_type.is_within_media_range(acceptable_media_range) {
                    let render_result = match content {
                        RegisteredContent::StaticContentItem(renderable) => {
                            renderable.render_to_native_media_type().map(box_media)
                        }
                        RegisteredContent::RegisteredTemplate(renderable) => renderable
                            .render_to_native_media_type(
                                context.content_engine.handlebars_registry(),
                                context.data.clone(),
                                context.handlebars_render_context.clone(),
                            )
                            .map(box_media),
                        RegisteredContent::Executable(renderable) => renderable
                            .render_to_native_media_type(
                                context.data.clone(),
                                context.handlebars_render_context.as_ref().and_then(
                                    |handlebars_render_context| {
                                        handlebars_render_context
                                            .context()
                                            .map(|context| context.data().clone())
                                    },
                                ),
                            )
                            .map(box_media),
                    };

                    // If rendering succeeded, return immediately. Otherwise
                    // keep trying.
                    match render_result {
                        Ok(rendered) => {
                            return if &rendered.media_type != registered_media_type {
                                Err(RenderError::Bug(format!(
                                    "The actual rendered media type ({}) did not match the \
                                        media type this content was registered for ({}).",
                                    rendered.media_type, registered_media_type,
                                )))
                            } else {
                                Ok(rendered)
                            }
                        }
                        Err(error) => {
                            log::warn!("Rendering failure: {error}");
                            errors.push(error)
                        }
                    };
                }
            }
        }

        // If execution makes it down here it means we cannot successfully
        // render the content into an acceptable media type, so we return
        // an error.
        //
        // If the loop didn't accumulate any errors it means there weren't even
        // any attempted renders because none of the available media ranges
        // were acceptable. Otherwise rendering was attempted and failed,
        // perhaps multiple times, in which case the first error is returned.
        Err(match errors.into_iter().next() {
            None => RenderError::CannotProvideAcceptableMediaType,
            Some(first_error) => RenderError::RenderingFailed(first_error),
        })
    }
}

fn box_media<'o, O: ByteStream + 'o>(media: Media<O>) -> Media<Box<dyn ByteStream + 'o>> {
    Media {
        content: Box::new(media.content),
        media_type: media.media_type,
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_lib::*;
    use super::*;
    use crate::test_lib::*;
    use maplit::hashmap;
    use tempfile::tempfile;
    use test_log::test;

    /// All of these will render to an empty string with media type text/plain
    /// or text/html.
    fn fixtures() -> (impl ContentEngine<()>, Vec<ContentRepresentations>) {
        let text_plain = MediaType::from_media_range(::mime::TEXT_PLAIN).unwrap();
        let text_html = MediaType::from_media_range(::mime::TEXT_HTML).unwrap();
        let mut content_engine = MockContentEngine::new();
        content_engine
            .register_template("registered-template", "")
            .unwrap();
        let empty_file = tempfile().expect("Failed to create temporary file");
        (
            content_engine,
            vec![
                hashmap![
                    text_plain.clone() => RegisteredContent::StaticContentItem(StaticContentItem::new(
                        empty_file.try_clone().unwrap(),
                        text_plain.clone(),
                    )),
                    text_html.clone() => RegisteredContent::StaticContentItem(StaticContentItem::new(
                        empty_file.try_clone().unwrap(),
                        text_html.clone(),
                    )),
                ],
                hashmap![
                    text_plain.clone() => RegisteredContent::Executable(Executable::new(
                        "true",
                        PROJECT_DIRECTORY,
                        text_plain.clone(),
                    )),
                    text_html.clone() => RegisteredContent::Executable(Executable::new(
                        "true",
                        PROJECT_DIRECTORY,
                        text_html.clone(),
                    )),
                ],
                hashmap![
                    text_plain.clone() => RegisteredContent::RegisteredTemplate(RegisteredTemplate::new(
                        "registered-template",
                        text_plain.clone(),
                    )),
                    text_html.clone() => RegisteredContent::RegisteredTemplate(RegisteredTemplate::new(
                        "registered-template",
                        text_html.clone(),
                    )),
                ],
            ],
        )
    }

    #[test]
    fn rendering_with_empty_acceptable_media_ranges_should_fail() {
        let (mock_engine, renderables) = fixtures();
        for (index, renderable) in renderables.iter().enumerate() {
            let render_result = renderable.render(
                mock_engine.render_context(None, hashmap![], hashmap![]),
                &[],
            );
            assert!(
                render_result.is_err(),
                "Rendering item {} with an empty list of acceptable media types did not fail as expected",
                index,
            )
        }
    }

    #[test]
    fn rendering_with_unacceptable_specific_media_ranges_should_fail() {
        let (mock_engine, renderables) = fixtures();
        for (index, renderable) in renderables.iter().enumerate() {
            let render_result = renderable.render(
                mock_engine.render_context(None, hashmap![], hashmap![]),
                &[::mime::IMAGE_GIF, ::mime::APPLICATION_PDF, ::mime::TEXT_CSS],
            );
            assert!(
                render_result.is_err(),
                "Rendering item {} with unacceptable media types did not fail as expected",
                index,
            )
        }
    }

    #[test]
    fn rendering_with_unacceptable_general_media_range_should_fail() {
        let (mock_engine, renderables) = fixtures();
        for (index, renderable) in renderables.iter().enumerate() {
            let render_result = renderable.render(
                mock_engine.render_context(None, hashmap![], hashmap![]),
                &[::mime::IMAGE_STAR],
            );
            assert!(
                render_result.is_err(),
                "Rendering item {} with unacceptable media types did not fail as expected",
                index,
            )
        }
    }

    #[test]
    fn rendering_with_acceptable_media_range_that_is_not_most_preferred_should_succeed() {
        let (mock_engine, renderables) = fixtures();
        for (index, renderable) in renderables.iter().enumerate() {
            let render_result = renderable.render(
                mock_engine.render_context(None, hashmap![], hashmap![]),
                &[::mime::IMAGE_GIF, ::mime::TEXT_PLAIN, ::mime::TEXT_CSS],
            );
            assert!(
                render_result.is_ok(),
                "Rendering item {} with acceptable media type did not succeed as expected: {}",
                index,
                render_result.err().unwrap(),
            );
            assert!(
                render_result.unwrap().media_type
                    == MediaType::from_media_range(::mime::TEXT_PLAIN).unwrap(),
                "Rendering item {} did not produce the expected media type",
                index,
            );
        }
    }

    #[test]
    fn rendering_with_acceptable_range_star_star_should_succeed() {
        let (mock_engine, renderables) = fixtures();
        for (index, renderable) in renderables.iter().enumerate() {
            let render_result = renderable.render(
                mock_engine.render_context(None, hashmap![], hashmap![]),
                &[::mime::STAR_STAR],
            );
            assert!(
                render_result.is_ok(),
                "Rendering item {} with acceptable media type did not succeed as expected: {}",
                index,
                render_result.err().unwrap(),
            );
        }
    }

    #[test]
    fn rendering_with_acceptable_range_text_star_should_succeed() {
        let (mock_engine, renderables) = fixtures();
        for (index, renderable) in renderables.iter().enumerate() {
            let render_result = renderable.render(
                mock_engine.render_context(None, hashmap![], hashmap![]),
                &[::mime::TEXT_STAR],
            );
            assert!(
                render_result.is_ok(),
                "Rendering item {} with acceptable media type did not succeed as expected: {}",
                index,
                render_result.err().unwrap(),
            );
            assert!(
                render_result
                    .unwrap()
                    .media_type
                    .is_within_media_range(&::mime::TEXT_STAR),
                "Rendering item {} did not produce an acceptable media type",
                index,
            );
        }
    }

    #[test]
    fn can_render_same_content_with_different_representations() {
        let (mock_engine, renderables) = fixtures();
        for (index, renderable) in renderables.iter().enumerate() {
            let text_plain_result = renderable.render(
                mock_engine.render_context(None, hashmap![], hashmap![]),
                &[::mime::TEXT_PLAIN],
            );
            assert!(
                text_plain_result.is_ok(),
                "Rendering item {} with acceptable media type did not succeed as expected: {}",
                index,
                text_plain_result.err().unwrap(),
            );
            assert!(
                text_plain_result.unwrap().media_type == ::mime::TEXT_PLAIN,
                "Rendering item {} did not produce the expected media type",
                index,
            );

            let text_html_result = renderable.render(
                mock_engine.render_context(None, hashmap![], hashmap![]),
                &[::mime::TEXT_HTML],
            );
            assert!(
                text_html_result.is_ok(),
                "Rendering item {} with acceptable media type did not succeed as expected: {}",
                index,
                text_html_result.err().unwrap(),
            );
            assert!(
                text_html_result.unwrap().media_type == ::mime::TEXT_HTML,
                "Rendering item {} did not produce the expected media type",
                index,
            );
        }
    }
}
