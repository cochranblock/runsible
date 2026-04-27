use indexmap::IndexMap;
use semver::{Version, VersionReq};
use std::collections::{HashMap, HashSet};

use crate::errors::{GalaxyError, Result};
use crate::registry::{PackageEntry, RegistryIndex};

/// A fully resolved dependency (name + pinned version + source info).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedDep {
    pub name: String,
    pub version: Version,
    pub registry_url: String,
    pub checksum: String,
}

/// Resolve a set of top-level dependency constraints against a registry.
///
/// M0 strategy: greedy "latest satisfying version" with cycle detection and
/// simple conflict detection (two paths requiring incompatible versions of the
/// same package).
pub fn resolve_deps(
    deps: &IndexMap<String, String>,
    registry: &RegistryIndex,
    registry_url: &str,
) -> Result<Vec<ResolvedDep>> {
    let mut resolved: HashMap<String, ResolvedDep> = HashMap::new();
    let mut visiting: HashSet<String> = HashSet::new();

    for (name, req_str) in deps {
        resolve_one(
            name,
            req_str,
            registry,
            registry_url,
            &mut resolved,
            &mut visiting,
        )?;
    }

    let mut result: Vec<ResolvedDep> = resolved.into_values().collect();
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

fn resolve_one(
    name: &str,
    req_str: &str,
    registry: &RegistryIndex,
    registry_url: &str,
    resolved: &mut HashMap<String, ResolvedDep>,
    visiting: &mut HashSet<String>,
) -> Result<()> {
    // Cycle detection.
    if visiting.contains(name) {
        return Err(GalaxyError::Cycle(name.to_string()));
    }

    let req = parse_version_req(req_str)?;

    // Already resolved — check compatibility.
    if let Some(existing) = resolved.get(name) {
        if !req.matches(&existing.version) {
            return Err(GalaxyError::Conflict(format!(
                "package '{}': already resolved to {} which does not satisfy new requirement '{}'",
                name, existing.version, req_str
            )));
        }
        return Ok(());
    }

    // Find the best (latest satisfying) version.
    let entry = find_best(name, &req, registry).ok_or_else(|| {
        GalaxyError::Resolver(format!(
            "no version of '{}' satisfies '{}' in registry",
            name, req_str
        ))
    })?;

    let version = Version::parse(&entry.version)?;

    visiting.insert(name.to_string());

    // Recurse into transitive deps first, then insert this package.
    for (dep_name, dep_req) in &entry.deps {
        resolve_one(
            dep_name,
            dep_req,
            registry,
            registry_url,
            resolved,
            visiting,
        )?;
    }

    visiting.remove(name);

    resolved.insert(
        name.to_string(),
        ResolvedDep {
            name: name.to_string(),
            version,
            registry_url: registry_url.to_string(),
            checksum: entry.checksum.clone(),
        },
    );

    Ok(())
}

/// Find the highest version in the registry that satisfies `req`.
fn find_best<'a>(
    name: &str,
    req: &VersionReq,
    registry: &'a RegistryIndex,
) -> Option<&'a PackageEntry> {
    let mut candidates: Vec<&PackageEntry> = registry
        .packages
        .iter()
        .filter(|e| e.name == name)
        .filter(|e| {
            Version::parse(&e.version)
                .map(|v| req.matches(&v))
                .unwrap_or(false)
        })
        .collect();

    candidates.sort_by(|a, b| {
        let va = Version::parse(&a.version).unwrap_or_else(|_| Version::new(0, 0, 0));
        let vb = Version::parse(&b.version).unwrap_or_else(|_| Version::new(0, 0, 0));
        vb.cmp(&va)
    });

    candidates.into_iter().next()
}

/// Parse a version requirement, accepting "*" as any, and Cargo-style "^" etc.
pub fn parse_version_req(s: &str) -> Result<VersionReq> {
    let s = s.trim();
    // "*" → any version
    if s == "*" {
        return Ok(VersionReq::STAR);
    }
    VersionReq::parse(s).map_err(|e| {
        GalaxyError::ManifestValidation(format!("invalid version requirement '{}': {}", s, e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::RegistryIndex;

    fn make_registry() -> RegistryIndex {
        RegistryIndex::from_json(
            r#"
{
  "packages": [
    { "name": "nginx", "version": "0.3.1", "checksum": "sha256:abc", "deps": { "common": "^1" } },
    { "name": "common", "version": "1.0.0", "checksum": "sha256:def", "deps": {} }
  ]
}
"#,
        )
        .unwrap()
    }

    #[test]
    fn semver_constraint_satisfied() {
        let req = parse_version_req("^1").unwrap();
        assert!(req.matches(&Version::parse("1.2.3").unwrap()));
        assert!(!req.matches(&Version::parse("2.0.0").unwrap()));
    }

    #[test]
    fn resolve_simple_deps() {
        let registry = make_registry();
        let mut deps = IndexMap::new();
        deps.insert("nginx".to_string(), "^0.3".to_string());

        let result = resolve_deps(&deps, &registry, "file:///tmp/registry").unwrap();
        let names: Vec<&str> = result.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"nginx"), "nginx should be resolved");
        assert!(names.contains(&"common"), "common should be resolved");
    }

    // ── New: single dep with no transitives resolves to 1 package ──────────
    #[test]
    fn resolve_single_no_transitives() {
        let registry = RegistryIndex::from_json(
            r#"
{
  "packages": [
    { "name": "solo", "version": "1.0.0", "checksum": "sha256:s1", "deps": {} }
  ]
}
"#,
        )
        .unwrap();

        let mut deps = IndexMap::new();
        deps.insert("solo".to_string(), "^1".to_string());

        let result = resolve_deps(&deps, &registry, "file:///tmp/r").unwrap();
        assert_eq!(result.len(), 1, "expected exactly 1 resolved pkg");
        assert_eq!(result[0].name, "solo");
        assert_eq!(result[0].version.to_string(), "1.0.0");
    }

    // ── New: diamond dependency resolves D once ────────────────────────────
    // A → B, A → C, B → D, C → D
    #[test]
    fn resolve_diamond_deduplicates() {
        let registry = RegistryIndex::from_json(
            r#"
{
  "packages": [
    { "name": "a", "version": "1.0.0", "checksum": "sha256:a", "deps": { "b": "^1", "c": "^1" } },
    { "name": "b", "version": "1.0.0", "checksum": "sha256:b", "deps": { "d": "^1" } },
    { "name": "c", "version": "1.0.0", "checksum": "sha256:c", "deps": { "d": "^1" } },
    { "name": "d", "version": "1.0.0", "checksum": "sha256:d", "deps": {} }
  ]
}
"#,
        )
        .unwrap();

        let mut deps = IndexMap::new();
        deps.insert("a".to_string(), "^1".to_string());

        let result = resolve_deps(&deps, &registry, "file:///tmp/r").unwrap();
        let names: Vec<&str> = result.iter().map(|r| r.name.as_str()).collect();
        let d_count = names.iter().filter(|n| **n == "d").count();
        assert_eq!(d_count, 1, "D should appear exactly once in diamond, got {:?}", names);
        assert_eq!(result.len(), 4, "diamond should yield 4 unique packages");
    }

    // ── New: ^1 matches 1.5.0 but not 2.0.0 (chooses latest matching) ──────
    #[test]
    fn caret_one_matches_latest_one_x() {
        let registry = RegistryIndex::from_json(
            r#"
{
  "packages": [
    { "name": "lib", "version": "1.0.0", "checksum": "sha256:l1", "deps": {} },
    { "name": "lib", "version": "1.5.0", "checksum": "sha256:l15", "deps": {} },
    { "name": "lib", "version": "2.0.0", "checksum": "sha256:l2", "deps": {} }
  ]
}
"#,
        )
        .unwrap();

        let mut deps = IndexMap::new();
        deps.insert("lib".to_string(), "^1".to_string());

        let result = resolve_deps(&deps, &registry, "file:///tmp/r").unwrap();
        assert_eq!(result.len(), 1);
        // Should pick the latest 1.x — 1.5.0 — and never 2.0.0.
        assert_eq!(
            result[0].version.to_string(),
            "1.5.0",
            "^1 should pick the highest 1.x — found {}",
            result[0].version
        );
    }

    #[test]
    fn conflict_detection() {
        // Build a registry where pkgA requires lib >=2.0 and pkgB requires lib <2.0
        let registry = RegistryIndex::from_json(
            r#"
{
  "packages": [
    { "name": "pkga", "version": "1.0.0", "checksum": "sha256:aaa", "deps": { "lib": ">=2.0" } },
    { "name": "pkgb", "version": "1.0.0", "checksum": "sha256:bbb", "deps": { "lib": "<2.0" } },
    { "name": "lib",  "version": "1.9.0", "checksum": "sha256:c19", "deps": {} },
    { "name": "lib",  "version": "2.0.0", "checksum": "sha256:c20", "deps": {} }
  ]
}
"#,
        )
        .unwrap();

        let mut deps = IndexMap::new();
        deps.insert("pkga".to_string(), "^1".to_string());
        deps.insert("pkgb".to_string(), "^1".to_string());

        let result = resolve_deps(&deps, &registry, "file:///tmp/reg");
        assert!(result.is_err(), "Expected conflict error");
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("conflict") || msg.contains("Conflict") || msg.contains("satisfy"),
            "Error message should mention conflict: {}",
            msg
        );
    }
}
