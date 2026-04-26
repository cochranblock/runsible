use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("config: {0}")]
    Config(#[from] ConfigError),

    #[error("parse: {0}")]
    Parse(#[from] ParseError),

    #[error("type: {0}")]
    Type(#[from] TypeError),

    #[error("plan: {0}")]
    Plan(#[from] PlanError),

    #[error("apply: {0}")]
    Apply(#[from] ApplyError),

    #[error("connection: {0}")]
    Connection(#[from] ConnectionError),

    #[error("vault: {0}")]
    Vault(#[from] VaultError),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config file not found at any search-path location")]
    NotFound,

    #[error("invalid TOML at {path}: {source}")]
    InvalidToml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("unknown config key: {0}")]
    UnknownKey(String),

    #[error("schema version {found} not supported (need {required})")]
    UnsupportedSchemaVersion { found: u32, required: u32 },

    #[error("config file at {path} is world-writable; refusing to read for safety")]
    WorldWritable { path: PathBuf },
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("invalid TOML: {0}")]
    InvalidToml(#[from] toml::de::Error),

    #[error("missing required field: {0}")]
    MissingField(String),

    #[error("invalid value for {field}: {message}")]
    InvalidValue { field: String, message: String },
}

#[derive(Debug, Error)]
pub enum TypeError {
    #[error("undeclared tag: {0}")]
    UndeclaredTag(String),

    #[error("unknown handler id: {0}")]
    UnknownHandlerId(String),

    #[error("unknown module reference: {0}")]
    UnknownModuleReference(String),

    #[error("type mismatch on {field}: expected {expected}, got {got}")]
    Mismatch {
        field: String,
        expected: String,
        got: String,
    },
}

#[derive(Debug, Error)]
pub enum PlanError {
    #[error("plan synthesis failed for module {module}: {message}")]
    Synthesis { module: String, message: String },
}

#[derive(Debug, Error)]
pub enum ApplyError {
    #[error("apply failed for module {module} on host {host}: {message}")]
    Failed {
        module: String,
        host: String,
        message: String,
    },

    #[error("post-apply verify failed: plan should be empty but is not")]
    VerifyNonEmpty,
}

#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("unable to connect to {host}: {message}")]
    Unreachable { host: String, message: String },

    #[error("authentication failed for {user}@{host}")]
    AuthFailed { user: String, host: String },

    #[error("preflight failed on {host}: {message}")]
    PreflightFailed { host: String, message: String },

    #[error("become failed on {host}: {message}")]
    BecomeFailed { host: String, message: String },

    #[error("command timed out after {seconds}s on {host}")]
    Timeout { host: String, seconds: u64 },
}

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("no recipient in our key set could decrypt this vault file")]
    NoRecipientMatch,

    #[error("vault file authentication failure (MAC check failed)")]
    AuthenticationFailure,

    #[error("unsupported vault envelope version: {0}")]
    UnsupportedVersion(String),
}
