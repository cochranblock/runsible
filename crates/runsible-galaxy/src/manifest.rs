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

    // ── New: deps and dev-deps round-trip through TOML ─────────────────────
    #[test]
    fn manifest_with_deps_and_dev_deps_roundtrip() {
        let m = PackageManifest {
            package: PackageMeta {
                name: "myapp".to_string(),
                version: "0.2.3".to_string(),
                description: Some("test app".to_string()),
                license: Some("MIT".to_string()),
                authors: None,
                repository: None,
                keywords: None,
                categories: None,
                readme: None,
                min_runsible_version: None,
            },
            dependencies: {
                let mut d = IndexMap::new();
                d.insert("nginx".to_string(), "^1.2".to_string());
                d.insert("postgres".to_string(), "=2.0.0".to_string());
                d
            },
            dev_dependencies: {
                let mut d = IndexMap::new();
                d.insert("test_helper".to_string(), "^0.1".to_string());
                d
            },
            entry_points: vec![],
        };

        let serialized = toml::to_string_pretty(&m).expect("serialize");
        let parsed = PackageManifest::from_str(&serialized).expect("re-parse");
        assert_eq!(parsed.package.name, "myapp");
        assert_eq!(parsed.dependencies.len(), 2);
        assert_eq!(parsed.dev_dependencies.len(), 1);
        assert_eq!(
            parsed.dependencies.get("nginx").map(String::as_str),
            Some("^1.2")
        );
        assert_eq!(
            parsed.dev_dependencies.get("test_helper").map(String::as_str),
            Some("^0.1")
        );
    }

    // ── New: author list (Vec<String>) parses ──────────────────────────────
    #[test]
    fn manifest_with_author_list_parses() {
        let toml_str = r#"
[package]
name = "team_pkg"
version = "1.0.0"
authors = ["Alice <a@example.com>", "Bob <b@example.com>"]
"#;
        let m = PackageManifest::from_str(toml_str).expect("should parse");
        let authors = m.package.authors.as_ref().expect("authors set");
        assert_eq!(authors.len(), 2);
        assert_eq!(authors[0], "Alice <a@example.com>");
        assert_eq!(authors[1], "Bob <b@example.com>");
    }

    // ── New: name with capital letters fails validation ────────────────────
    // Locks current behavior: validate() rejects names not matching [a-z][a-z0-9_-]*.
    #[test]
    fn manifest_with_caps_in_name_errors() {
        let toml_str = r#"
[package]
name = "Name-With-Caps"
version = "0.1.0"
"#;
        let result = PackageManifest::from_str(toml_str);
        assert!(
            result.is_err(),
            "Expected validation error for capitalized name; got: {:?}",
            result
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("lowercase") || msg.contains("invalid"),
            "Error message should mention lowercase/invalid: {}",
            msg
        );
    }
}
