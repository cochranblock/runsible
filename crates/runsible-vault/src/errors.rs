// SPDX-License-Identifier: Unlicense
// Contributors: Cochran Block
//! Error types for runsible-vault.

use thiserror::Error;

/// Convenience alias.
pub type Result<T> = std::result::Result<T, VaultError>;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("invalid vault header: {0}")]
    InvalidHeader(String),

    #[error("decrypt failed: {0}")]
    DecryptFailed(String),

    #[error("no private key available in key store")]
    NoPrivateKey,

    #[error("recipient parse error: {0}")]
    RecipientParse(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
