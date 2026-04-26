use std::time::Duration;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, ConnectionError>;

#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("exec failed: {0}")]
    ExecFailed(String),

    #[error("timeout after {0:?}")]
    Timeout(Duration),

    #[error("ssh error: {0}")]
    Ssh(String),

    #[error("scp error: {0}")]
    Scp(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl From<ConnectionError> for runsible_core::errors::Error {
    fn from(e: ConnectionError) -> Self {
        // Map to the closest core variant. Timeout maps to Connection::Timeout,
        // everything else is Connection::Unreachable with "local" as host.
        match e {
            ConnectionError::Timeout(d) => runsible_core::errors::Error::Connection(
                runsible_core::errors::ConnectionError::Timeout {
                    host: "local".into(),
                    seconds: d.as_secs(),
                },
            ),
            other => runsible_core::errors::Error::Connection(
                runsible_core::errors::ConnectionError::Unreachable {
                    host: "local".into(),
                    message: other.to_string(),
                },
            ),
        }
    }
}
