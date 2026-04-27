//! `runsible-pull` — managed-host pull-mode for runsible.
//!
//! M0 surface: local pull, unsigned, one-shot. Fetch from a `git` source
//! (HTTPS or `file://`); spawn `runsible-playbook` against the fetched bundle;
//! write `heartbeat.json` atomically. No daemon, no HTTP heartbeat, no
//! signature verification — those land in M1+.

pub mod apply;
pub mod config;
pub mod errors;
pub mod fetch;
pub mod heartbeat;

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
}
