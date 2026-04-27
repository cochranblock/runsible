//! Error types for `runsible-test`.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, TestError>;

#[derive(Debug, Error)]
pub enum TestError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error in {path}: {message}")]
    TomlParse { path: String, message: String },

    #[error("package directory not found: {0}")]
    PackageNotFound(String),

    #[error("subprocess failed: {0}")]
    Subprocess(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
