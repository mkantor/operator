use serde::Serialize;

#[derive(Clone, Copy, Serialize)]
pub struct SolitonVersion(pub &'static str);

pub trait Render {
    type RenderArgs;
    type Error;
    fn render(&self, context: &Self::RenderArgs) -> Result<String, Self::Error>;
}
