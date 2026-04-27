//! Apply phase: spawn `runsible-playbook` against the fetched bundle.
//!
//! M0 deliberately spawns the binary instead of linking the engine in-process.
//! Process isolation per §11 of the plan: a misbehaving playbook cannot blow
//! up the pull binary. The `embed` feature path is M1+.
//!
//! The playbook is run against a synthetic `localhost,` inline inventory; the
//! pull-mode story is "this host applies its own state," so the inventory is
//! always the local host.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::PullConfig;
use crate::errors::{PullError, Result};

/// Outcome of running `runsible-playbook` in apply phase.
#[derive(Debug, Clone)]
pub struct ApplyResult {
    pub exit_code: i32,
    pub ok: u64,
    pub changed: u64,
    pub failed: u64,
    pub stdout: String,
    pub stderr: String,
}

/// Run `runsible-playbook` against the bundle.
///
/// `bundle_dir` is the working directory of the bundle (the git working tree).
/// `cfg.apply.playbook` is interpreted relative to `bundle_dir`.
///
/// The binary is resolved as:
///   1. `$RUNSIBLE_PLAYBOOK_BIN` env var, if set.
///   2. The `runsible-playbook` next to the current executable
///      (`std::env::current_exe()`'s parent), if present.
///   3. Bare `runsible-playbook` (relies on `$PATH`).
pub fn run_playbook(cfg: &PullConfig, bundle_dir: &Path) -> Result<ApplyResult> {
    let playbook_path = bundle_dir.join(&cfg.apply.playbook);

    let bin = resolve_playbook_bin();

    let out = Command::new(&bin)
        .arg(&playbook_path)
        .arg("-i")
        .arg("localhost,")
        .current_dir(bundle_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| PullError::Apply(format!("spawning {bin:?}: {e}")))?;

    let exit_code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();

    let (ok, changed, failed) = parse_run_summary(&stdout);

    Ok(ApplyResult {
        exit_code,
        ok,
        changed,
        failed,
        stdout,
        stderr,
    })
}

fn resolve_playbook_bin() -> PathBuf {
    if let Ok(p) = std::env::var("RUNSIBLE_PLAYBOOK_BIN") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("runsible-playbook");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from("runsible-playbook")
}

/// Best-effort scrape of the RunSummary line out of runsible-playbook's
/// stdout. The playbook's NDJSON envelope (`runsible.event.v1`) tags variants
/// with a `kind` discriminator using snake_case, so the run-summary line
/// looks like `{"kind":"run_summary","ok":1,...}`.
///
/// On any parse failure we return `(0, 0, 0)` and the caller treats the run
/// as the exit code says it did. This keeps M0 robust to the playbook's
/// output format being a moving target.
fn parse_run_summary(stdout: &str) -> (u64, u64, u64) {
    for line in stdout.lines().rev() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let kind = v
                .get("kind")
                .or_else(|| v.get("type"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            let is_summary = kind.eq_ignore_ascii_case("run_summary")
                || kind.eq_ignore_ascii_case("RunSummary");

            if is_summary {
                let ok = v.get("ok").and_then(|x| x.as_u64()).unwrap_or(0);
                let changed = v.get("changed").and_then(|x| x.as_u64()).unwrap_or(0);
                let failed = v.get("failed").and_then(|x| x.as_u64()).unwrap_or(0);
                return (ok, changed, failed);
            }
        }
    }
    (0, 0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_run_summary_extracts_counts() {
        let stdout = r#"{"kind":"task_start","name":"x"}
{"kind":"run_summary","ok":3,"changed":1,"failed":0,"unreachable":0,"skipped":0,"elapsed_ms":12}
"#;
        let (ok, changed, failed) = parse_run_summary(stdout);
        assert_eq!((ok, changed, failed), (3, 1, 0));
    }

    #[test]
    fn parse_run_summary_returns_zeros_on_garbage() {
        let stdout = "not json\nstill not json\n";
        let (ok, changed, failed) = parse_run_summary(stdout);
        assert_eq!((ok, changed, failed), (0, 0, 0));
    }

    /// Shared lock for tests that mutate `$RUNSIBLE_PLAYBOOK_BIN`. Process-wide
    /// env vars race across the rayon-style default test runner, so any test
    /// that touches this variable must take the same lock for the duration.
    static PLAYBOOK_BIN_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn resolve_playbook_bin_honors_env() {
        let _guard = PLAYBOOK_BIN_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("RUNSIBLE_PLAYBOOK_BIN", "/some/where/runsible-playbook");
        let p = resolve_playbook_bin();
        assert_eq!(p, PathBuf::from("/some/where/runsible-playbook"));
        std::env::remove_var("RUNSIBLE_PLAYBOOK_BIN");
    }

    #[test]
    fn run_playbook_with_nonexistent_env_binary_returns_apply_error() {
        // Force the resolver to a path that cannot exist; spawn must fail and
        // surface as PullError::Apply with a message naming the missing file.
        let _guard = PLAYBOOK_BIN_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let prev = std::env::var("RUNSIBLE_PLAYBOOK_BIN").ok();
        std::env::set_var(
            "RUNSIBLE_PLAYBOOK_BIN",
            "/nonexistent/bin/runsible-playbook-9f3c",
        );

        let cfg = crate::config::PullConfig {
            source: crate::config::SourceConfig {
                kind: "git".into(),
                url: "file:///nope".into(),
                branch: "main".into(),
                ssh_key: None,
            },
            apply: crate::config::ApplyConfig {
                playbook: PathBuf::from("site.toml"),
                extra_vars: vec![],
            },
            paths: crate::config::PathsConfig {
                state_dir: PathBuf::from("/tmp"),
                heartbeat_path: PathBuf::from("/tmp/heartbeat.json"),
            },
            schedule: Default::default(),
            heartbeat: Default::default(),
        };

        let err = run_playbook(&cfg, std::path::Path::new("/tmp"))
            .expect_err("must fail on missing binary");
        match err {
            PullError::Apply(msg) => {
                assert!(
                    msg.contains("/nonexistent/bin/runsible-playbook-9f3c"),
                    "Apply error should name the binary it tried; got: {msg}"
                );
            }
            other => panic!("expected PullError::Apply, got {other:?}"),
        }

        // Restore prior env state.
        match prev {
            Some(p) => std::env::set_var("RUNSIBLE_PLAYBOOK_BIN", p),
            None => std::env::remove_var("RUNSIBLE_PLAYBOOK_BIN"),
        }
    }
}
