//! Error types for runsible-console.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConsoleError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("readline: {0}")]
    Readline(String),
    #[error("playbook: {0}")]
    Playbook(String),
    #[error("parse: {0}")]
    Parse(String),
}

pub type Result<T> = std::result::Result<T, ConsoleError>;
