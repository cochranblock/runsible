use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlaybookError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse: {0}")]
    Parse(String),
    #[error("type check: {0}")]
    TypeCheck(String),
    #[error("module not found: '{0}'")]
    ModuleNotFound(String),
    #[error("inventory: {0}")]
    Inventory(String),
    #[error("execution failed on {host}: {message}")]
    ExecFailed { host: String, message: String },
}

pub type Result<T> = std::result::Result<T, PlaybookError>;
