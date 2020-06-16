use serde::Serialize;

#[derive(Serialize)]
pub struct GluonVersion(pub &'static str);

pub trait Render<'a> {
    type RenderArgs;
    type Error;
    fn render(&self, context: &Self::RenderArgs) -> Result<String, Self::Error>;
}
