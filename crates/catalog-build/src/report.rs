//! Build report: what was built, what is merely suspicious, what stops the
//! build (Section 3.5).

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct BuildReport {
    pub games: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

impl BuildReport {
    pub fn warn(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }
    pub fn error(&mut self, message: impl Into<String>) {
        self.errors.push(message.into());
    }
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }
}
