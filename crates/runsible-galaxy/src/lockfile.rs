use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::errors::{GalaxyError, Result};

/// A single locked package entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockedPackage {
    pub name: String,
    pub version: String,
    pub registry: String,
    pub checksum: String,
}

/// The full `runsible.lock` file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Lockfile {
    pub schema_version: u32,
    pub packages: Vec<LockedPackage>,
}

impl Lockfile {
    pub fn new() -> Self {
        Lockfile {
            schema_version: 1,
            packages: Vec::new(),
        }
    }

    /// Parse from a TOML string.
    pub fn from_str(s: &str) -> Result<Self> {
        toml::from_str(s).map_err(|e| GalaxyError::Lockfile(e.to_string()))
    }

    /// Serialize to a TOML string with the warning header.
    pub fn to_toml_string(&self) -> Result<String> {
        let body = toml::to_string_pretty(self)?;
        Ok(format!("# This file is auto-generated. Do not edit manually.\n{}", body))
    }

    /// Read from a file on disk.
    pub fn read_from_file(path: &Path) -> Result<Self> {
        let s = std::fs::read_to_string(path)?;
        Self::from_str(&s)
    }

    /// Write to a file on disk.
    pub fn write_to_file(&self, path: &Path) -> Result<()> {
        let s = self.to_toml_string()?;
        std::fs::write(path, s)?;
        Ok(())
    }
}

impl Default for Lockfile {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lockfile_roundtrip() {
        let mut lock = Lockfile::new();
        lock.packages.push(LockedPackage {
            name: "nginx".to_string(),
            version: "0.3.1".to_string(),
            registry: "file:///tmp/registry".to_string(),
            checksum: "sha256:abc123".to_string(),
        });
        lock.packages.push(LockedPackage {
            name: "common".to_string(),
            version: "1.0.0".to_string(),
            registry: "file:///tmp/registry".to_string(),
            checksum: "sha256:def456".to_string(),
        });

        let tmp = tempfile_path();
        lock.write_to_file(&tmp).unwrap();

        let loaded = Lockfile::read_from_file(&tmp).unwrap();
        assert_eq!(loaded.schema_version, 1);
        assert_eq!(loaded.packages.len(), 2);
        assert_eq!(loaded.packages[0].name, "nginx");
        assert_eq!(loaded.packages[1].name, "common");
        assert_eq!(loaded.packages[0].checksum, "sha256:abc123");

        let _ = std::fs::remove_file(&tmp);
    }

    fn tempfile_path() -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("runsible-lock-test-{}.toml", std::process::id()));
        path
    }

    // ── New: empty lockfile (0 packages) round-trips ───────────────────────
    #[test]
    fn empty_lockfile_roundtrip() {
        let lock = Lockfile::new();
        assert_eq!(lock.packages.len(), 0);

        let s = lock.to_toml_string().expect("serialize");
        // Re-parse must succeed.
        let parsed = Lockfile::from_str(&s).expect("re-parse");
        assert_eq!(parsed.schema_version, 1);
        assert_eq!(parsed.packages.len(), 0);
    }

    // ── New: lockfile with newer schema_version still parses (current behavior) ─
    // Locks current behavior: there's no version gate yet. Test documents
    // that a schema_version > 1 deserializes OK.
    #[test]
    fn lockfile_future_schema_version_deserializes() {
        let src = r#"
schema_version = 99
packages = []
"#;
        let parsed = Lockfile::from_str(src).expect("parse should succeed currently");
        assert_eq!(
            parsed.schema_version, 99,
            "current behavior: future schema_versions are accepted as-is"
        );
        assert_eq!(parsed.packages.len(), 0);
    }
}
