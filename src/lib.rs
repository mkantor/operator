use serde::Serialize;

#[derive(Serialize)]
pub struct GluonVersion(pub &'static str);
