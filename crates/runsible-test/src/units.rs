//! Unit-test runner — invokes `cargo test` for each `crates/<sub>/` Cargo
//! workspace inside a runsible package directory (M0 scope).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::env;

/// Aggregate report from running all per-module unit suites in a package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitsReport {
    pub package: String,
    pub crates_tested: Vec<String>,
    pub passed: usize,
    pub failed: usize,
    /// Set when the package has no `crates/` directory; the runner exits 0
    /// in that case.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_reason: Option<String>,
}

impl UnitsReport {
    pub fn was_skipped(&self) -> bool {
        self.skipped_reason.is_some()
    }
}

/// Run `cargo test` for each `<pkg_dir>/crates/<sub>/Cargo.toml`.
///
/// If the package has no `crates/` subdirectory the report is returned with
/// `skipped_reason = Some(...)` and no subprocess is spawned.
pub fn run_units(pkg_dir: &Path) -> UnitsReport {
    let package = pkg_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("<unknown>")
        .to_string();

    let crates_dir = pkg_dir.join("crates");
    if !crates_dir.exists() {
        return UnitsReport {
            package,
            crates_tested: Vec::new(),
            passed: 0,
            failed: 0,
            skipped_reason: Some("no Rust modules found, skipping units".into()),
        };
    }

    let cargo = env::find_cargo()
        .unwrap_or_else(|| PathBuf::from("cargo"));

    let mut crates_tested = Vec::new();
    let mut passed = 0usize;
    let mut failed = 0usize;

    let entries = match std::fs::read_dir(&crates_dir) {
        Ok(e) => e,
        Err(_) => {
            return UnitsReport {
                package,
                crates_tested,
                passed,
                failed,
                skipped_reason: Some(format!(
                    "could not read `{}`",
                    crates_dir.display()
                )),
            };
        }
    };

    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let manifest = p.join("Cargo.toml");
        if !manifest.exists() {
            continue;
        }
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("<unknown>")
            .to_string();
        crates_tested.push(name);

        let status = Command::new(&cargo)
            .arg("test")
            .arg("--manifest-path")
            .arg(&manifest)
            .status();

        match status {
            Ok(s) if s.success() => passed += 1,
            _ => failed += 1,
        }
    }

    UnitsReport {
        package,
        crates_tested,
        passed,
        failed,
        skipped_reason: None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn units_no_crates_skips() {
        let dir = TempDir::new().unwrap();
        let report = run_units(dir.path());
        assert!(
            report.skipped_reason.is_some(),
            "expected skipped_reason to be Some; got: {:?}",
            report
        );
        assert_eq!(report.passed, 0);
        assert_eq!(report.failed, 0);
    }

    #[test]
    fn units_with_zero_test_crate_records_one_passed_zero_failed() {
        // Skip if cargo isn't reachable — units.rs can't run without it.
        if env::find_cargo().is_none() {
            eprintln!("skipping units_with_zero_test_crate: no cargo available");
            return;
        }

        let dir = TempDir::new().unwrap();
        let pkg = dir.path();
        let crate_dir = pkg.join("crates/null-mod");
        std::fs::create_dir_all(crate_dir.join("src")).unwrap();
        std::fs::write(
            crate_dir.join("Cargo.toml"),
            r#"[package]
name = "null-mod"
version = "0.0.1"
edition = "2021"

[lib]
path = "src/lib.rs"

[dependencies]
"#,
        )
        .unwrap();
        // No `#[test]` functions at all — `cargo test` should exit 0 with
        // zero tests run.
        std::fs::write(crate_dir.join("src/lib.rs"), "//! empty crate\n").unwrap();

        let report = run_units(pkg);
        assert!(
            report.skipped_reason.is_none(),
            "report must not be skipped when crates/ exists; got: {:?}",
            report
        );
        assert!(
            !report.crates_tested.is_empty(),
            "at least one crate must be tested; got: {:?}",
            report.crates_tested
        );
        assert!(
            report.crates_tested.iter().any(|c| c == "null-mod"),
            "crates_tested must list null-mod; got: {:?}",
            report.crates_tested
        );
        assert_eq!(report.failed, 0, "no failures expected; got: {:?}", report);
    }
}
