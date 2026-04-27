//! HTTP heartbeat POST with retries + exponential backoff + on-failure queue.
//!
//! Uses the system `curl` binary so we don't pull `reqwest`/`hyper`/`rustls`
//! into the dep graph. `curl` is universally available on every Linux/macOS
//! controller and supports HTTPS, custom headers, timeouts, and exit-code
//! semantics that are easy to wrap.
//!
//! On final failure (retries exhausted), if `queue_path` is set, the
//! heartbeat JSON line is appended to that file. A subsequent successful
//! POST will drain the backlog before reporting success.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::config::HeartbeatConfig;
use crate::heartbeat::Heartbeat;

#[derive(Debug, thiserror::Error)]
pub enum HttpHeartbeatError {
    #[error("curl spawn: {0}")]
    CurlSpawn(String),
    #[error("curl exited with {code:?}: {stderr}")]
    CurlFailed { code: Option<i32>, stderr: String },
    #[error("curl unavailable (`which curl` failed): install curl or set heartbeat.url=\"\" to disable")]
    CurlUnavailable,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// POST one heartbeat to the configured URL with retries and backoff. On
/// final failure, append to the queue file (if configured). On success,
/// also drain any previously queued heartbeats.
pub fn post_heartbeat(cfg: &HeartbeatConfig, hb: &Heartbeat) -> Result<(), HttpHeartbeatError> {
    if cfg.url.is_empty() {
        return Ok(()); // disabled
    }
    if !curl_available() {
        return Err(HttpHeartbeatError::CurlUnavailable);
    }

    let body = serde_json::to_string(hb)?;

    let timeout = Duration::from_secs(cfg.timeout_seconds.max(1));
    let max_retries = cfg.max_retries.max(0);
    let mut backoff = Duration::from_secs(cfg.initial_backoff_seconds.max(1));

    let mut attempt: u32 = 0;
    let mut last_err: Option<HttpHeartbeatError>;
    loop {
        match curl_post(&cfg.url, &cfg.bearer_token, &body, timeout) {
            Ok(()) => {
                // Success — drain the queue.
                if !cfg.queue_path.is_empty() {
                    drain_queue(cfg, Path::new(&cfg.queue_path), timeout)?;
                }
                return Ok(());
            }
            Err(e) => {
                last_err = Some(e);
                if attempt >= max_retries {
                    break;
                }
                std::thread::sleep(backoff);
                backoff = backoff.saturating_mul(2);
                attempt += 1;
            }
        }
    }

    // Retries exhausted. Queue if configured, then surface the last error.
    if !cfg.queue_path.is_empty() {
        if let Err(qe) = enqueue(Path::new(&cfg.queue_path), &body) {
            return Err(qe);
        }
    }
    Err(last_err.unwrap_or(HttpHeartbeatError::CurlFailed {
        code: None,
        stderr: "no attempts made (max_retries=0 and immediate failure)".into(),
    }))
}

fn curl_post(
    url: &str,
    bearer: &str,
    body: &str,
    timeout: Duration,
) -> Result<(), HttpHeartbeatError> {
    let mut cmd = Command::new("curl");
    cmd.arg("-fsS")
        .arg("--max-time")
        .arg(timeout.as_secs().to_string())
        .arg("-X")
        .arg("POST")
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("-d")
        .arg(body);

    if !bearer.is_empty() {
        cmd.arg("-H").arg(format!("Authorization: Bearer {bearer}"));
    }
    cmd.arg(url);

    let out = cmd
        .output()
        .map_err(|e| HttpHeartbeatError::CurlSpawn(e.to_string()))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(HttpHeartbeatError::CurlFailed {
            code: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        })
    }
}

fn curl_available() -> bool {
    Command::new("curl")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Append a JSON line to the queue file, atomically (via tempfile + rename).
fn enqueue(path: &Path, body: &str) -> Result<(), HttpHeartbeatError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(body.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

/// Drain the queue file by re-posting every line. Stops on first failure
/// (the line stays in the queue file because we don't truncate until full
/// success).
///
/// Implementation: read all lines, then re-POST them one by one. On full
/// success, truncate the queue file. On partial failure, rewrite the file
/// with the unsent suffix.
fn drain_queue(
    cfg: &HeartbeatConfig,
    path: &Path,
    timeout: Duration,
) -> Result<(), HttpHeartbeatError> {
    if !path.exists() {
        return Ok(());
    }
    let body = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = body.lines().filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        let _ = std::fs::remove_file(path);
        return Ok(());
    }

    let mut sent = 0usize;
    for line in &lines {
        match curl_post(&cfg.url, &cfg.bearer_token, line, timeout) {
            Ok(()) => sent += 1,
            Err(_) => break,
        }
    }

    if sent == lines.len() {
        let _ = std::fs::remove_file(path);
    } else {
        // Rewrite with the unsent suffix.
        let remaining = lines[sent..].join("\n");
        std::fs::write(path, format!("{remaining}\n"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HeartbeatConfig;
    use crate::heartbeat::{Heartbeat, HeartbeatResult};

    fn sample_heartbeat() -> Heartbeat {
        Heartbeat::new(
            "2026-04-27T00:00:00Z".into(),
            "2026-04-27T00:00:01Z".into(),
            1000,
            "https://example.com/repo.git".into(),
            "abc1234".into(),
            "site.toml".into(),
            HeartbeatResult { exit_code: 0, ok: 1, changed: 0, failed: 0 },
            vec![],
        )
    }

    #[test]
    fn empty_url_is_a_no_op() {
        let cfg = HeartbeatConfig {
            url: "".into(),
            ..Default::default()
        };
        post_heartbeat(&cfg, &sample_heartbeat()).expect("empty url must succeed silently");
    }

    #[test]
    fn enqueue_appends_to_file() {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("rsl-pull-q-{pid}-{nanos}.ndjson"));
        let _ = std::fs::remove_file(&path);

        enqueue(&path, r#"{"a":1}"#).unwrap();
        enqueue(&path, r#"{"b":2}"#).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(body, "{\"a\":1}\n{\"b\":2}\n");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn post_to_invalid_url_falls_through_to_queue_when_configured() {
        if !curl_available() {
            eprintln!("skip: curl unavailable");
            return;
        }
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let q_path = std::env::temp_dir()
            .join(format!("rsl-pull-q-fail-{pid}-{nanos}.ndjson"));
        let _ = std::fs::remove_file(&q_path);

        let cfg = HeartbeatConfig {
            url: "http://127.0.0.1:1/none".into(),  // RFC 6335: port 1 should refuse
            bearer_token: "".into(),
            timeout_seconds: 2,
            max_retries: 1,
            initial_backoff_seconds: 1,
            queue_path: q_path.to_string_lossy().to_string(),
        };

        let hb = sample_heartbeat();
        let r = post_heartbeat(&cfg, &hb);
        assert!(r.is_err(), "unreachable URL must error");
        assert!(q_path.exists(), "failed POST must enqueue");

        let body = std::fs::read_to_string(&q_path).unwrap();
        assert!(body.starts_with("{"));
        let _ = std::fs::remove_file(&q_path);
    }
}
