use serde::Serialize;

#[derive(Clone, Copy, Serialize)]
pub struct SolitonVersion(pub &'static str);
