//! `runsible-pull` — managed-host pull-mode for runsible.
//!
//! M0 surface: local pull, unsigned, one-shot. Fetch from a `git` source
//! (HTTPS or `file://`); spawn `runsible-playbook` against the fetched bundle;
//! write `heartbeat.json` atomically. No daemon, no HTTP heartbeat, no
//! signature verification — those land in M1+.

pub mod apply;
pub mod config;
pub mod daemon;
pub mod errors;
pub mod fetch;
pub mod heartbeat;
pub mod http_heartbeat;

pub use apply::{run_playbook, ApplyResult};
pub use config::{ApplyConfig, PathsConfig, PullConfig, SourceConfig};
pub use errors::{PullError, Result};
pub use fetch::{fetch_or_clone, FetchResult};
pub use heartbeat::{Heartbeat, HeartbeatResult, HEARTBEAT_SCHEMA};

use std::time::Instant;

/// Run one full fetch → apply → heartbeat cycle.
///
/// On success **and** on a recoverable failure (fetch fails, apply fails),
/// the heartbeat is written before returning. On unrecoverable I/O errors
/// (cannot create state_dir, cannot write heartbeat), the error is returned
/// and the heartbeat may not be updated.
pub fn pull_once(cfg: &PullConfig) -> Result<Heartbeat> {
    cfg.validate()?;

    let started_at = chrono::Utc::now();
    let started_iso = started_at.to_rfc3339();
    let t0 = Instant::now();

    let mut errors: Vec<String> = Vec::new();

    let fetched = match fetch_or_clone(cfg) {
        Ok(r) => Some(r),
        Err(e) => {
            errors.push(format!("fetch: {e}"));
            None
        }
    };

    let (source_rev, applied) = match fetched.as_ref() {
        Some(r) => {
            let result = match run_playbook(cfg, &r.bundle_dir) {
                Ok(out) => Some(out),
                Err(e) => {
                    errors.push(format!("apply: {e}"));
                    None
                }
            };
            (r.source_rev.clone(), result)
        }
        None => (String::new(), None),
    };

    let completed_at = chrono::Utc::now();
    let elapsed_ms = t0.elapsed().as_millis() as u64;

    let result = match (&fetched, &applied) {
        (Some(_), Some(a)) => HeartbeatResult {
            exit_code: a.exit_code,
            ok: a.ok,
            changed: a.changed,
            failed: a.failed,
        },
        (Some(_), None) => HeartbeatResult {
            // Apply failed before producing a result.
            exit_code: 2,
            ok: 0,
            changed: 0,
            failed: 0,
        },
        (None, _) => HeartbeatResult {
            // Fetch failed: per the plan §7.1, exit 3 = fetch failed.
            exit_code: 3,
            ok: 0,
            changed: 0,
            failed: 0,
        },
    };

    let hb = Heartbeat::new(
        started_iso,
        completed_at.to_rfc3339(),
        elapsed_ms,
        cfg.source.url.clone(),
        source_rev,
        cfg.apply.playbook.display().to_string(),
        result,
        errors,
    );

    hb.write_atomic(&cfg.paths.heartbeat_path)?;

    Ok(hb)
}

// ---------------------------------------------------------------------------
// TRIPLE SIMS gate
// ---------------------------------------------------------------------------

/// Smoke gate: build a Heartbeat, write it atomically to a tempfile, read it
/// back, verify schema field and errors round-trip exactly. Returns 0 on
/// success; non-zero codes indicate which stage failed.
pub fn f30() -> i32 {
    use std::path::PathBuf;

    // Use process id + nanosecond ts to avoid concurrent-test races.
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("runsible-pull-f30-{pid}-{nanos}"));
    if let Err(_) = std::fs::create_dir_all(&dir) {
        return 1;
    }
    let path: PathBuf = dir.join("heartbeat.json");

    let errors = vec![
        "fetch: timed out".to_string(),
        "apply: bad rc".to_string(),
    ];

    let hb = Heartbeat::new(
        "2026-04-26T12:34:56Z".into(),
        "2026-04-26T12:35:01Z".into(),
        5234,
        "https://example.com/repo.git".into(),
        "abc1234".into(),
        "playbooks/site.toml".into(),
        HeartbeatResult {
            exit_code: 0,
            ok: 1,
            changed: 0,
            failed: 0,
        },
        errors.clone(),
    );

    if hb.schema != HEARTBEAT_SCHEMA {
        let _ = std::fs::remove_dir_all(&dir);
        return 2;
    }

    if let Err(_) = hb.write_atomic(&path) {
        let _ = std::fs::remove_dir_all(&dir);
        return 3;
    }

    let read_back = match Heartbeat::read(&path) {
        Ok(h) => h,
        Err(_) => {
            let _ = std::fs::remove_dir_all(&dir);
            return 4;
        }
    };

    if read_back.schema != "runsible.pull.heartbeat.v1" {
        let _ = std::fs::remove_dir_all(&dir);
        return 5;
    }

    if read_back.errors != errors {
        let _ = std::fs::remove_dir_all(&dir);
        return 6;
    }

    if read_back != hb {
        let _ = std::fs::remove_dir_all(&dir);
        return 7;
    }

    let _ = std::fs::remove_dir_all(&dir);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fake_cfg(state: &std::path::Path) -> PullConfig {
        PullConfig {
            source: SourceConfig {
                kind: "git".into(),
                url: "file:///does/not/exist".into(),
                branch: "main".into(),
                ssh_key: None,
            },
            apply: ApplyConfig {
                playbook: PathBuf::from("site.toml"),
                extra_vars: vec![],
            },
            paths: PathsConfig {
                state_dir: state.to_path_buf(),
                heartbeat_path: state.join("heartbeat.json"),
            },
            schedule: Default::default(),
            heartbeat: Default::default(),
        }
    }

    #[test]
    fn pull_once_failed_fetch_still_writes_heartbeat() {
        if !fetch::git_available() {
            eprintln!("skipping pull_once_failed_fetch: git not on PATH");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let cfg = fake_cfg(dir.path());

        let hb = pull_once(&cfg).expect("write heartbeat even on fetch failure");
        assert_eq!(hb.result.exit_code, 3);
        assert!(!hb.errors.is_empty(), "errors must record the fetch failure");
        assert!(cfg.paths.heartbeat_path.exists());

        let read_back = Heartbeat::read(&cfg.paths.heartbeat_path).unwrap();
        assert_eq!(read_back, hb);
    }

    #[test]
    fn pull_once_with_bogus_url_records_fetch_error() {
        if !fetch::git_available() {
            eprintln!("skipping pull_once_with_bogus_url: git not on PATH");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let mut cfg = fake_cfg(dir.path());
        cfg.source.url = "https://invalid.invalid.invalid/no/such/repo.git".into();

        let hb = pull_once(&cfg).expect("heartbeat must still be written");
        // The captured error message should reference the fetch phase; the
        // exit code is the M0-defined `3 == fetch failed` value.
        assert_eq!(hb.result.exit_code, 3);
        assert!(
            hb.errors.iter().any(|e| e.starts_with("fetch:")),
            "expected a 'fetch:' error message; got: {:?}",
            hb.errors
        );
        assert!(cfg.paths.heartbeat_path.exists());
    }
}
