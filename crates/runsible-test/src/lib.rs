//! `runsible-test` — developer-facing test runner for runsible packages.
//!
//! M0 scope: single-package sanity + units + env discovery.

pub mod config;
pub mod env;
pub mod errors;
pub mod sanity;
pub mod units;

pub use env::{discover_env, EnvReport};
pub use errors::{Result, TestError};
pub use sanity::{run_sanity, SanityFinding, SanityReport, Severity};
pub use units::{run_units, UnitsReport};

// ---------------------------------------------------------------------------
// TRIPLE SIMS gate
// ---------------------------------------------------------------------------

/// Smoke gate: build a tempdir without `runsible.toml`, verify S001 fires;
/// then build a tempdir WITH a valid runsible.toml and tasks/main.toml and
/// tests/, verify zero error-severity findings. Returns 0 on success.
pub fn f30() -> i32 {
    use std::path::PathBuf;

    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    // ── Stage 1: empty tempdir → S001 must fire ──────────────────────────────
    let empty_dir: PathBuf = std::env::temp_dir().join(format!("runsible-test-f30-empty-{pid}-{nanos}"));
    if std::fs::create_dir_all(&empty_dir).is_err() {
        return 1;
    }
    let report = run_sanity(&empty_dir);
    let has_s001 = report.findings.iter().any(|f| f.id == "S001");
    if !has_s001 {
        let _ = std::fs::remove_dir_all(&empty_dir);
        return 2;
    }
    let _ = std::fs::remove_dir_all(&empty_dir);

    // ── Stage 2: well-formed package → zero error-severity findings ──────────
    let good_dir: PathBuf = std::env::temp_dir().join(format!("runsible-test-f30-good-{pid}-{nanos}"));
    if std::fs::create_dir_all(&good_dir).is_err() {
        return 3;
    }

    if std::fs::write(
        good_dir.join("runsible.toml"),
        "[package]\nname = \"f30pkg\"\nversion = \"0.0.1\"\n",
    )
    .is_err()
    {
        let _ = std::fs::remove_dir_all(&good_dir);
        return 4;
    }

    if std::fs::create_dir_all(good_dir.join("tasks")).is_err() {
        let _ = std::fs::remove_dir_all(&good_dir);
        return 5;
    }

    let main_toml = r#"
schema = "runsible.playbook.v1"

[imports]
debug = "runsible_builtin.debug"

[[plays]]
name = "Demo"
hosts = "localhost"

[[plays.tasks]]
name = "say hi"
debug = { msg = "hello" }
"#;
    if std::fs::write(good_dir.join("tasks/main.toml"), main_toml).is_err() {
        let _ = std::fs::remove_dir_all(&good_dir);
        return 6;
    }

    if std::fs::create_dir_all(good_dir.join("tests")).is_err() {
        let _ = std::fs::remove_dir_all(&good_dir);
        return 7;
    }

    let good_report = run_sanity(&good_dir);
    let errors: Vec<_> = good_report
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .collect();
    if !errors.is_empty() {
        let _ = std::fs::remove_dir_all(&good_dir);
        return 8;
    }

    let _ = std::fs::remove_dir_all(&good_dir);
    0
}
