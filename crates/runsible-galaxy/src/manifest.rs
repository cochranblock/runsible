use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::errors::{GalaxyError, Result};

/// Metadata in the `[package]` section of runsible.toml.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageMeta {
    /// Package name: [a-z][a-z0-9_-]*
    pub name: String,
    /// Strict semver version string.
    pub version: String,
    /// Short human description.
    pub description: Option<String>,
    pub license: Option<String>,
    pub authors: Option<Vec<String>>,
    pub repository: Option<String>,
    pub keywords: Option<Vec<String>>,
    pub categories: Option<Vec<String>>,
    pub readme: Option<String>,
    pub min_runsible_version: Option<String>,
}

/// An `[[entry_points]]` entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntryPoint {
    pub name: String,
    pub tasks: Option<String>,
    pub handlers: Option<String>,
    pub defaults: Option<String>,
    pub vars: Option<String>,
}

/// The full contents of a `runsible.toml` package manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageManifest {
    pub package: PackageMeta,

    #[serde(default)]
    pub dependencies: IndexMap<String, String>,

    #[serde(rename = "dev-dependencies", default)]
    pub dev_dependencies: IndexMap<String, String>,

    #[serde(default)]
    pub entry_points: Vec<EntryPoint>,
}

impl PackageManifest {
    /// Parse a manifest from a TOML string.
    pub fn from_str(s: &str) -> Result<Self> {
        let m: PackageManifest =
            toml::from_str(s).map_err(|e| GalaxyError::ManifestParse(e.to_string()))?;
        m.validate()?;
        Ok(m)
    }

    /// Validate invariants beyond what serde checks.
    pub fn validate(&self) -> Result<()> {
        // Name must be present (always is, since it's not Option) and valid.
        let name = &self.package.name;
        if name.is_empty() {
            return Err(GalaxyError::ManifestValidation(
                "package.name is empty".into(),
            ));
        }
        // [a-z][a-z0-9_-]*
        let mut chars = name.chars();
        let first = chars.next().unwrap();
        if !first.is_ascii_lowercase() {
            return Err(GalaxyError::ManifestValidation(format!(
                "package.name '{}' must start with a lowercase ASCII letter",
                name
            )));
        }
        for c in chars {
            if !matches!(c, 'a'..='z' | '0'..='9' | '_' | '-') {
                return Err(GalaxyError::ManifestValidation(format!(
                    "package.name '{}' contains invalid character '{}'",
                    name, c
                )));
            }
        }

        // Version must parse as semver.
        semver::Version::parse(&self.package.version).map_err(|e| {
            GalaxyError::ManifestValidation(format!(
                "package.version '{}' is not valid semver: {}",
                self.package.version, e
            ))
        })?;

        Ok(())
    }
}

/// Parse a TOML manifest from disk.
pub fn parse_manifest_file(path: &std::path::Path) -> Result<PackageManifest> {
    let s = std::fs::read_to_string(path)?;
    PackageManifest::from_str(&s)
}

/// Write a manifest to disk (round-trips through toml serialization).
pub fn write_manifest_file(path: &std::path::Path, manifest: &PackageManifest) -> Result<()> {
    let s = toml::to_string_pretty(manifest)?;
    std::fs::write(path, s)?;
    Ok(())
}

/// Scaffold a minimal manifest for `init`.
pub fn scaffold_manifest(name: &str) -> PackageManifest {
    PackageManifest {
        package: PackageMeta {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            description: Some(format!("{} runsible package", name)),
            license: Some("Apache-2.0 OR MIT".to_string()),
            authors: None,
            repository: None,
            keywords: None,
            categories: None,
            readme: Some("README.md".to_string()),
            min_runsible_version: None,
        },
        dependencies: IndexMap::new(),
        dev_dependencies: IndexMap::new(),
        entry_points: vec![EntryPoint {
            name: "main".to_string(),
            tasks: Some("tasks/main.toml".to_string()),
            handlers: Some("handlers/main.toml".to_string()),
            defaults: Some("defaults/main.toml".to_string()),
            vars: Some("vars/main.toml".to_string()),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const MINIMAL_MANIFEST: &str = r#"
[package]
name = "nginx"
version = "0.3.1"
description = "Install and configure nginx"
license = "MIT OR Apache-2.0"

[dependencies]
common = "^1"
"#;

    #[test]
    fn manifest_parse_valid() {
        let m = PackageManifest::from_str(MINIMAL_MANIFEST).unwrap();
        assert_eq!(m.package.name, "nginx");
        assert_eq!(m.package.version, "0.3.1");
        assert_eq!(
            m.package.description.as_deref(),
            Some("Install and configure nginx")
        );
        assert_eq!(m.dependencies.get("common").map(|s| s.as_str()), Some("^1"));
    }

    #[test]
    fn manifest_missing_name_errors() {
        let toml_str = r#"
[package]
version = "0.1.0"
"#;
        // serde will error because `name` is not Option
        let result = PackageManifest::from_str(toml_str);
        assert!(
            result.is_err(),
            "Expected error for missing package.name"
        );
    }
}
