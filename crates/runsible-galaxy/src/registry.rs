use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::errors::{GalaxyError, Result};

/// A single entry in the registry index.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageEntry {
    pub name: String,
    pub version: String,
    pub checksum: String,
    #[serde(default)]
    pub deps: IndexMap<String, String>,
}

/// The top-level registry index (`index.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegistryIndex {
    pub packages: Vec<PackageEntry>,
}

impl RegistryIndex {
    /// Parse from JSON bytes / string.
    pub fn from_json(s: &str) -> Result<Self> {
        let idx: RegistryIndex = serde_json::from_str(s)?;
        Ok(idx)
    }

    /// Serialize to pretty JSON.
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Load a file:// registry index from a directory path.
    pub fn load_from_dir(dir: &Path) -> Result<Self> {
        let index_path = dir.join("index.json");
        let s = std::fs::read_to_string(&index_path).map_err(|e| {
            GalaxyError::Registry(format!(
                "cannot read index.json from {}: {}",
                index_path.display(),
                e
            ))
        })?;
        Self::from_json(&s)
    }

    /// Write the index to a directory.
    pub fn save_to_dir(&self, dir: &Path) -> Result<()> {
        let json = self.to_json()?;
        let path = dir.join("index.json");
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Return all entries for a given package name, sorted newest→oldest.
    pub fn versions_for(&self, name: &str) -> Vec<&PackageEntry> {
        let mut entries: Vec<&PackageEntry> =
            self.packages.iter().filter(|e| e.name == name).collect();
        entries.sort_by(|a, b| {
            let va = semver::Version::parse(&a.version).unwrap_or_else(|_| semver::Version::new(0, 0, 0));
            let vb = semver::Version::parse(&b.version).unwrap_or_else(|_| semver::Version::new(0, 0, 0));
            vb.cmp(&va)
        });
        entries
    }

    /// Path to the tarball for a given package inside a registry directory.
    pub fn tarball_path(dir: &Path, name: &str, version: &str) -> PathBuf {
        dir.join(format!("{}-{}.runsible-pkg", name, version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const MINIMAL_INDEX: &str = r#"
{
  "packages": [
    { "name": "nginx", "version": "0.3.1", "checksum": "sha256:abc123", "deps": { "common": "^1" } },
    { "name": "common", "version": "1.0.0", "checksum": "sha256:def456", "deps": {} }
  ]
}
"#;

    #[test]
    fn registry_index_parse() {
        let idx = RegistryIndex::from_json(MINIMAL_INDEX).unwrap();
        assert_eq!(idx.packages.len(), 2);
        assert_eq!(idx.packages[0].name, "nginx");
        assert_eq!(idx.packages[1].name, "common");
    }
}
