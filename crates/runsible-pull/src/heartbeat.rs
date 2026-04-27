//! Heartbeat artifact: the single most operator-visible thing this binary
//! produces.
//!
//! M0 schema: `runsible.pull.heartbeat.v1` — narrower than the §5 daemon-mode
//! schema. Atomic writes only: `<path>.tmp` then `rename(2)` to the final path
//! so a reader sees either the previous full document or the new one, never
//! a torn write.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::{PullError, Result};

pub const HEARTBEAT_SCHEMA: &str = "runsible.pull.heartbeat.v1";

/// One heartbeat document. Written every cycle, even on failure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Heartbeat {
    pub schema: String,
    pub started_at: String,
    pub completed_at: String,
    pub elapsed_ms: u64,
    pub source_url: String,
    pub source_rev: String,
    pub playbook_path: String,
    pub result: HeartbeatResult,
    #[serde(default)]
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HeartbeatResult {
    pub exit_code: i32,
    pub ok: u64,
    pub changed: u64,
    pub failed: u64,
}

impl Heartbeat {
    /// Build a fresh heartbeat with the schema field pre-set.
    pub fn new(
        started_at: String,
        completed_at: String,
        elapsed_ms: u64,
        source_url: String,
        source_rev: String,
        playbook_path: String,
        result: HeartbeatResult,
        errors: Vec<String>,
    ) -> Self {
        Self {
            schema: HEARTBEAT_SCHEMA.into(),
            started_at,
            completed_at,
            elapsed_ms,
            source_url,
            source_rev,
            playbook_path,
            result,
            errors,
        }
    }

    /// Atomically write the heartbeat to `path`. Strategy:
    ///   1. Ensure `path`'s parent directory exists.
    ///   2. Serialize to JSON.
    ///   3. Write to `<path>.tmp`, fsync, then `rename(2)` over `path`.
    ///
    /// If serialization or the temp write fails, the prior `path` (if any) is
    /// untouched and any partial `.tmp` file is cleaned up.
    pub fn write_atomic(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let body = serde_json::to_vec_pretty(self)?;

        let tmp = tmp_path(path);

        // Best-effort: try to write+sync+rename. On any error, scrub the tmp.
        match write_then_rename(&tmp, path, &body) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                Err(e)
            }
        }
    }

    /// Read a heartbeat back from disk.
    pub fn read(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(PullError::HeartbeatMissing(path.to_path_buf()));
        }
        let body = std::fs::read_to_string(path)?;
        let hb: Heartbeat = serde_json::from_str(&body).map_err(|e| {
            PullError::InvalidHeartbeatJson {
                path: path.to_path_buf(),
                source: e,
            }
        })?;
        Ok(hb)
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
}

fn write_then_rename(tmp: &Path, final_path: &Path, body: &[u8]) -> Result<()> {
    use std::io::Write;
    {
        let mut f = std::fs::File::create(tmp)?;
        f.write_all(body)?;
        f.sync_all()?;
    }
    std::fs::rename(tmp, final_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Heartbeat {
        Heartbeat::new(
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
            vec![],
        )
    }

    #[test]
    fn heartbeat_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("heartbeat.json");

        let hb = sample();
        hb.write_atomic(&path).unwrap();

        let read_back = Heartbeat::read(&path).unwrap();
        assert_eq!(read_back, hb);
        assert_eq!(read_back.schema, HEARTBEAT_SCHEMA);
    }

    #[test]
    fn heartbeat_atomic_no_partial() {
        // Write a good heartbeat, then attempt a write that fails (parent
        // directory cannot be created because it is a regular file). The
        // prior heartbeat must remain intact and no `.tmp` should linger.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("heartbeat.json");
        let good = sample();
        good.write_atomic(&path).unwrap();

        // Stage a "bad path": a child path under an existing file. mkdir on
        // the parent will fail with NotADirectory.
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        let bad_path = blocker.join("nested/heartbeat.json");

        let bad = sample();
        let err = bad.write_atomic(&bad_path);
        assert!(err.is_err(), "writing under a regular file must fail");

        // Original is intact.
        let still = Heartbeat::read(&path).unwrap();
        assert_eq!(still, good);

        // No stale .tmp next to the original.
        let stale = path.with_extension("json.tmp");
        assert!(!stale.exists(), "no .tmp must remain at {:?}", stale);
    }

    #[test]
    fn read_missing_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        let err = Heartbeat::read(&path).unwrap_err();
        assert!(matches!(err, PullError::HeartbeatMissing(_)));
    }

    #[test]
    fn heartbeat_schema_constant_is_v1() {
        // Lock the wire schema; bumping it is intentionally a breaking change
        // and must trip this test until consumers are updated.
        assert_eq!(HEARTBEAT_SCHEMA, "runsible.pull.heartbeat.v1");
        let hb = sample();
        assert_eq!(hb.schema, "runsible.pull.heartbeat.v1");
    }

    #[test]
    fn heartbeat_with_errors_serializes_messages() {
        let mut hb = sample();
        hb.errors = vec!["fetch: timed out".into(), "apply: bad rc".into()];
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("heartbeat.json");
        hb.write_atomic(&path).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("fetch: timed out"));
        assert!(body.contains("apply: bad rc"));

        // And it round-trips back through the typed read path.
        let read_back = Heartbeat::read(&path).unwrap();
        assert_eq!(read_back.errors, hb.errors);
    }

    #[test]
    fn heartbeat_read_non_json_returns_invalid_heartbeat_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("heartbeat.json");
        std::fs::write(&path, b"<<not json at all>>").unwrap();
        let err = Heartbeat::read(&path).unwrap_err();
        match err {
            PullError::InvalidHeartbeatJson { path: p, source: _ } => {
                assert_eq!(p, path);
            }
            other => panic!("expected InvalidHeartbeatJson, got {other:?}"),
        }
    }
}
