//! Sanity tests over a single runsible package directory (M0 scope).
//!
//! Rules:
//! - **S001** — `runsible.toml` exists at the package root.
//! - **S002** — `runsible.toml` parses as valid TOML.
//! - **S003** — `runsible.toml` has a `[package]` table with `name` and `version`.
//! - **S004** — Each declared `[[entry_points]]` `tasks` file (if present) exists
//!   on disk and parses as TOML.
//! - **S005** — All TOML files under `tasks/`, `handlers/`, `defaults/`, `vars/`
//!   parse as TOML.
//! - **S006** — Run `runsible-lint` over each task file (delegated; collects
//!   findings as `S006` entries with the underlying rule ID embedded in the
//!   message).
//! - **S007** — `tests/` directory present (warning if missing).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use runsible_galaxy::manifest::PackageManifest;
use runsible_lint::{lint_file, LintConfig, Profile, Severity as LintSeverity};

/// Severity of a sanity finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Info => write!(f, "info"),
            Severity::Warning => write!(f, "warning"),
            Severity::Error => write!(f, "error"),
        }
    }
}

/// One sanity finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanityFinding {
    pub id: String,
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    pub message: String,
}

/// Aggregate report for one package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanityReport {
    pub package: String,
    pub findings: Vec<SanityFinding>,
}

impl SanityReport {
    /// Return true if any finding has Error severity.
    pub fn has_errors(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Error)
    }

    pub fn count_by(&self, severity: Severity) -> usize {
        self.findings.iter().filter(|f| f.severity == severity).count()
    }
}

/// Run all sanity checks over a single package directory.
pub fn run_sanity(pkg_dir: &Path) -> SanityReport {
    let mut findings: Vec<SanityFinding> = Vec::new();

    // The package label defaults to the directory's last component, then is
    // overridden by the manifest name once we successfully parse it.
    let mut package = pkg_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("<unknown>")
        .to_string();

    let manifest_path = pkg_dir.join("runsible.toml");

    // ── S001: manifest exists ────────────────────────────────────────────────
    if !manifest_path.exists() {
        findings.push(SanityFinding {
            id: "S001".into(),
            severity: Severity::Error,
            file: Some(manifest_path.clone()),
            message: format!("`runsible.toml` not found at package root ({})", pkg_dir.display()),
        });
        // Without a manifest we can still try `tasks/`, `handlers/`, etc.
        check_package_toml_dirs(pkg_dir, &mut findings);
        check_tests_dir(pkg_dir, &mut findings);
        return SanityReport { package, findings };
    }

    // ── S002 / S003: parse + validate ────────────────────────────────────────
    let raw = match std::fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) => {
            findings.push(SanityFinding {
                id: "S002".into(),
                severity: Severity::Error,
                file: Some(manifest_path.clone()),
                message: format!("could not read `runsible.toml`: {e}"),
            });
            check_package_toml_dirs(pkg_dir, &mut findings);
            check_tests_dir(pkg_dir, &mut findings);
            return SanityReport { package, findings };
        }
    };

    // Try the strict typed parse first.
    match PackageManifest::from_str(&raw) {
        Ok(manifest) => {
            // Successful parse + validate — name available.
            package = manifest.package.name.clone();

            // ── S004: declared entry-point tasks files exist + parse ─────────
            for ep in &manifest.entry_points {
                if let Some(rel) = &ep.tasks {
                    let p = pkg_dir.join(rel);
                    if !p.exists() {
                        findings.push(SanityFinding {
                            id: "S004".into(),
                            severity: Severity::Error,
                            file: Some(p.clone()),
                            message: format!(
                                "entry_points.{}.tasks references `{}`, which does not exist",
                                ep.name, rel
                            ),
                        });
                    } else if let Err(e) = parse_toml_file(&p) {
                        findings.push(SanityFinding {
                            id: "S004".into(),
                            severity: Severity::Error,
                            file: Some(p.clone()),
                            message: format!(
                                "entry_points.{}.tasks file `{}` does not parse: {}",
                                ep.name, rel, e
                            ),
                        });
                    }
                }
            }
        }
        Err(e) => {
            // Distinguish S002 (TOML syntax) from S003 (missing/invalid fields).
            // The galaxy crate wraps both kinds. We do a permissive parse first
            // to decide which rule fired.
            let permissive: std::result::Result<toml::Value, _> = toml::from_str(&raw);
            match permissive {
                Err(syn_err) => {
                    findings.push(SanityFinding {
                        id: "S002".into(),
                        severity: Severity::Error,
                        file: Some(manifest_path.clone()),
                        message: format!("`runsible.toml` does not parse as TOML: {syn_err}"),
                    });
                }
                Ok(value) => {
                    // TOML parsed but the typed manifest didn't — missing
                    // required fields or schema validation failed. That's S003.
                    let pkg_table = value.get("package").and_then(|v| v.as_table());
                    if pkg_table.is_none() {
                        findings.push(SanityFinding {
                            id: "S003".into(),
                            severity: Severity::Error,
                            file: Some(manifest_path.clone()),
                            message: "`runsible.toml` is missing the `[package]` table".into(),
                        });
                    } else {
                        let t = pkg_table.unwrap();
                        let missing: Vec<&str> = ["name", "version"]
                            .iter()
                            .copied()
                            .filter(|k| !t.contains_key(*k))
                            .collect();
                        if !missing.is_empty() {
                            findings.push(SanityFinding {
                                id: "S003".into(),
                                severity: Severity::Error,
                                file: Some(manifest_path.clone()),
                                message: format!(
                                    "`runsible.toml` `[package]` is missing required field(s): {}",
                                    missing.join(", ")
                                ),
                            });
                        } else {
                            // Both present but typed parse still failed —
                            // surface the underlying validation error as S003.
                            findings.push(SanityFinding {
                                id: "S003".into(),
                                severity: Severity::Error,
                                file: Some(manifest_path.clone()),
                                message: format!(
                                    "`runsible.toml` `[package]` failed validation: {e}"
                                ),
                            });
                        }
                    }

                    // Try to set the package label even when validation failed,
                    // for nicer report output.
                    if let Some(name) = value
                        .get("package")
                        .and_then(|v| v.get("name"))
                        .and_then(|v| v.as_str())
                    {
                        package = name.to_string();
                    }
                }
            }
        }
    }

    // ── S005: every TOML file under tasks/, handlers/, defaults/, vars/ parses
    check_package_toml_dirs(pkg_dir, &mut findings);

    // ── S006: run runsible-lint over each task file ──────────────────────────
    let lint_cfg = LintConfig {
        profile: Profile::Basic,
        ..LintConfig::default()
    };
    let task_files = collect_toml_files(&pkg_dir.join("tasks"));
    for tf in task_files {
        let result = lint_file(&tf, &lint_cfg);
        for finding in result.findings {
            let sev = match finding.severity {
                LintSeverity::Info => Severity::Info,
                LintSeverity::Warning => Severity::Warning,
                LintSeverity::Error => Severity::Error,
            };
            findings.push(SanityFinding {
                id: "S006".into(),
                severity: sev,
                file: Some(tf.clone()),
                message: format!(
                    "[lint {}] {}{}",
                    finding.rule_id,
                    finding.description,
                    finding
                        .line
                        .map(|l| format!(" (line {l})"))
                        .unwrap_or_default()
                ),
            });
        }
    }

    // ── S007: tests/ directory ───────────────────────────────────────────────
    check_tests_dir(pkg_dir, &mut findings);

    SanityReport { package, findings }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Walk the four well-known TOML subdirs and assert each `*.toml` parses (S005).
fn check_package_toml_dirs(pkg_dir: &Path, findings: &mut Vec<SanityFinding>) {
    for sub in ["tasks", "handlers", "defaults", "vars"] {
        let dir = pkg_dir.join(sub);
        if !dir.exists() {
            continue;
        }
        for f in collect_toml_files(&dir) {
            if let Err(e) = parse_toml_file(&f) {
                findings.push(SanityFinding {
                    id: "S005".into(),
                    severity: Severity::Error,
                    file: Some(f.clone()),
                    message: format!("TOML parse error: {e}"),
                });
            }
        }
    }
}

fn check_tests_dir(pkg_dir: &Path, findings: &mut Vec<SanityFinding>) {
    let tests = pkg_dir.join("tests");
    if !tests.exists() {
        findings.push(SanityFinding {
            id: "S007".into(),
            severity: Severity::Warning,
            file: Some(tests),
            message: "no `tests/` directory at package root — every package should ship tests"
                .into(),
        });
    }
}

/// Recursively collect `*.toml` files under `dir`. Returns empty if the dir
/// doesn't exist or can't be read.
fn collect_toml_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !dir.exists() {
        return out;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.extend(collect_toml_files(&p));
        } else if p.extension().and_then(|e| e.to_str()) == Some("toml") {
            out.push(p);
        }
    }
    out
}

fn parse_toml_file(path: &Path) -> std::result::Result<toml::Value, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    toml::from_str(&raw).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn has_id_with_severity(report: &SanityReport, id: &str, sev: Severity) -> bool {
        report
            .findings
            .iter()
            .any(|f| f.id == id && f.severity == sev)
    }

    #[test]
    fn sanity_missing_runsible_toml_fires_s001() {
        let dir = TempDir::new().unwrap();
        let report = run_sanity(dir.path());
        assert!(
            has_id_with_severity(&report, "S001", Severity::Error),
            "expected S001 error; got: {:#?}",
            report.findings
        );
    }

    #[test]
    fn sanity_clean_package_passes() {
        let dir = TempDir::new().unwrap();
        let pkg = dir.path();
        fs::write(
            pkg.join("runsible.toml"),
            r#"
[package]
name = "demo"
version = "0.1.0"
"#,
        )
        .unwrap();
        fs::create_dir_all(pkg.join("tasks")).unwrap();
        fs::write(
            pkg.join("tasks/main.toml"),
            r#"
schema = "runsible.playbook.v1"

[imports]
debug = "runsible_builtin.debug"

[[plays]]
name = "Demo"
hosts = "localhost"

[[plays.tasks]]
name = "say hi"
debug = { msg = "hello" }
"#,
        )
        .unwrap();
        fs::create_dir_all(pkg.join("tests")).unwrap();

        let report = run_sanity(pkg);
        let errors: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "clean package should produce zero error-severity findings; got: {:#?}",
            errors
        );
    }

    #[test]
    fn sanity_invalid_toml_fires_s002() {
        let dir = TempDir::new().unwrap();
        let pkg = dir.path();
        // Unbalanced brackets — TOML syntax error.
        fs::write(pkg.join("runsible.toml"), "[package\nname = \"x\"\n").unwrap();
        let report = run_sanity(pkg);
        assert!(
            has_id_with_severity(&report, "S002", Severity::Error),
            "expected S002 error; got: {:#?}",
            report.findings
        );
    }

    #[test]
    fn sanity_missing_package_fields_fires_s003() {
        let dir = TempDir::new().unwrap();
        let pkg = dir.path();
        // Valid TOML, but `[package]` lacks `name`.
        fs::write(
            pkg.join("runsible.toml"),
            r#"
[package]
version = "0.1.0"
"#,
        )
        .unwrap();
        let report = run_sanity(pkg);
        assert!(
            has_id_with_severity(&report, "S003", Severity::Error),
            "expected S003 error; got: {:#?}",
            report.findings
        );
    }

    #[test]
    fn sanity_invalid_task_toml_fires_s005() {
        let dir = TempDir::new().unwrap();
        let pkg = dir.path();
        fs::write(
            pkg.join("runsible.toml"),
            r#"
[package]
name = "demo"
version = "0.1.0"
"#,
        )
        .unwrap();
        fs::create_dir_all(pkg.join("tasks")).unwrap();
        // Garbage TOML
        fs::write(pkg.join("tasks/main.toml"), "this is not = [valid toml\n").unwrap();
        let report = run_sanity(pkg);
        assert!(
            has_id_with_severity(&report, "S005", Severity::Error),
            "expected S005 error; got: {:#?}",
            report.findings
        );
    }
}
