use serde::Serialize;

#[derive(Clone, Copy, Serialize)]
pub struct SolitonVersion(pub &'static str);

#[derive(Clone, Serialize)]
pub struct SolitonInfo {
    pub version: SolitonVersion,
}
