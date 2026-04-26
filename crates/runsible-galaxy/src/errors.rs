use thiserror::Error;

pub type Result<T> = std::result::Result<T, GalaxyError>;

#[derive(Debug, Error)]
pub enum GalaxyError {
    #[error("manifest parse error: {0}")]
    ManifestParse(String),

    #[error("manifest validation error: {0}")]
    ManifestValidation(String),

    #[error("registry error: {0}")]
    Registry(String),

    #[error("resolver error: {0}")]
    Resolver(String),

    #[error("dependency conflict: {0}")]
    Conflict(String),

    #[error("cycle detected involving package '{0}'")]
    Cycle(String),

    #[error("lockfile error: {0}")]
    Lockfile(String),

    #[error("tarball error: {0}")]
    Tarball(String),

    #[error("checksum mismatch for '{path}': expected {expected}, got {actual}")]
    ChecksumMismatch {
        path: String,
        expected: String,
        actual: String,
    },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML deserialize error: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("TOML serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("semver error: {0}")]
    Semver(#[from] semver::Error),
}
