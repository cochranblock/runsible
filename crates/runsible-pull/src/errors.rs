//! Error types for `runsible-pull`.

use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, PullError>;

#[derive(Debug, Error)]
pub enum PullError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("config: {0}")]
    Config(String),

    #[error("invalid TOML at {path}: {source}")]
    InvalidConfigToml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("invalid JSON at {path}: {source}")]
    InvalidHeartbeatJson {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("serialize: {0}")]
    Serialize(#[from] serde_json::Error),

    #[error("git fetch failed: {0}")]
    Fetch(String),

    #[error("apply (runsible-playbook) failed: {0}")]
    Apply(String),

    #[error("source kind '{0}' not yet implemented at M0 (only 'git' is supported)")]
    UnsupportedSourceKind(String),

    #[error("SSH key auth not yet implemented at M0; use HTTPS or file:// URL")]
    SshKeyNotImplemented,

    #[error("unable to resolve home directory for path expansion of '{0}'")]
    HomeUnresolved(String),

    #[error("heartbeat not found at {0}")]
    HeartbeatMissing(PathBuf),
}
