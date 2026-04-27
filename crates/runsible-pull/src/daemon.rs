//! Daemon mode: fetch + apply on a configured interval, with jitter.
//!
//! Each cycle calls `pull_once`, then (if configured) POSTs the heartbeat
//! to the configured URL via `http_heartbeat::post_heartbeat`.
//!
//! The loop respects SIGTERM and SIGINT (cleanly returning after the
//! in-flight cycle completes). For clean shutdown we use an
//! `AtomicBool` that the signal handlers flip.
//!
//! Jitter is computed from a deterministic LCG keyed on (cycle index, pid).
//! Actual cryptographic randomness isn't important here — we just want to
//! avoid a thundering herd of pull-mode clients waking up at the same
//! second of each minute.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::{parse_duration, PullConfig};
use crate::errors::Result;
use crate::http_heartbeat::post_heartbeat;
use crate::pull_once;

/// Run the daemon loop until `stop` flips to true. Returns the number of
/// completed cycles.
pub fn run_daemon(cfg: &PullConfig, stop: Arc<AtomicBool>) -> Result<u64> {
    let interval_secs = parse_duration(&cfg.schedule.interval)
        .map_err(|e| crate::errors::PullError::InvalidConfigToml {
            path: std::path::PathBuf::from("<schedule.interval>"),
            source: bad_value(&e),
        })?;
    let jitter_secs = parse_duration(&cfg.schedule.jitter).unwrap_or(0);

    let mut cycle: u64 = 0;
    while !stop.load(Ordering::SeqCst) {
        let cycle_start = Instant::now();
        // One cycle.
        let cycle_result = pull_once(cfg);
        if let Ok(hb) = &cycle_result {
            // POST the heartbeat (no-op if cfg.heartbeat.url is empty).
            if let Err(e) = post_heartbeat(&cfg.heartbeat, hb) {
                eprintln!("heartbeat POST: {e}");
            }
        } else if let Err(e) = &cycle_result {
            eprintln!("cycle {cycle}: pull_once failed: {e}");
        }
        cycle += 1;
        let elapsed = cycle_start.elapsed();

        // Sleep until next cycle, sliced into 1s ticks so we can react to stop.
        let jitter = compute_jitter_seconds(cycle, jitter_secs);
        let total_sleep = interval_secs.saturating_add(jitter);
        let target = Duration::from_secs(total_sleep).saturating_sub(elapsed);
        let mut remaining = target;
        while remaining > Duration::ZERO && !stop.load(Ordering::SeqCst) {
            let tick = remaining.min(Duration::from_secs(1));
            std::thread::sleep(tick);
            remaining = remaining.saturating_sub(tick);
        }
    }
    Ok(cycle)
}

/// Deterministic per-cycle jitter using a simple LCG. Returns a value in
/// `[0, jitter_max_seconds]`. Returns 0 when `jitter_max_seconds == 0`.
fn compute_jitter_seconds(cycle: u64, jitter_max_seconds: u64) -> u64 {
    if jitter_max_seconds == 0 {
        return 0;
    }
    // Knuth's MMIX LCG constants, with the cycle and pid as the seed.
    let pid = std::process::id() as u64;
    let mut x = cycle.wrapping_mul(6_364_136_223_846_793_005_u64)
        .wrapping_add(1_442_695_040_888_963_407_u64)
        .wrapping_add(pid);
    x = x.wrapping_mul(6_364_136_223_846_793_005_u64).wrapping_add(1_442_695_040_888_963_407_u64);
    x % jitter_max_seconds
}

/// Construct a placeholder TOML-decode error for `parse_duration` failures so
/// we can fold them through the existing PullError variant. The error text is
/// the human-readable parse_duration message.
fn bad_value(msg: &str) -> toml::de::Error {
    // toml::de::Error has no public constructor; produce one via a real parse.
    // We deliberately produce a syntactically broken TOML to capture an error
    // and stringify it into our message. If that ever changes, tests will
    // reveal it because they check the error type, not the message.
    let placeholder = format!("# {msg}\n*broken*=\n");
    toml::from_str::<toml::Value>(&placeholder).unwrap_err()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use std::path::PathBuf;

    fn fake_cfg() -> PullConfig {
        PullConfig {
            source: SourceConfig {
                kind: "git".into(),
                url: "file:///nonexistent".into(),
                branch: "main".into(),
                ssh_key: None,
            },
            apply: ApplyConfig {
                playbook: PathBuf::from("site.toml"),
                extra_vars: vec![],
            },
            paths: PathsConfig {
                state_dir: std::env::temp_dir().join(format!("rsl-daemon-{}", std::process::id())),
                heartbeat_path: std::env::temp_dir()
                    .join(format!("rsl-daemon-{}-hb.json", std::process::id())),
            },
            schedule: ScheduleConfig {
                interval: "1s".into(),
                jitter: "0s".into(),
            },
            heartbeat: HeartbeatConfig::default(),
        }
    }

    #[test]
    fn jitter_zero_returns_zero() {
        assert_eq!(compute_jitter_seconds(0, 0), 0);
        assert_eq!(compute_jitter_seconds(99, 0), 0);
    }

    #[test]
    fn jitter_within_bound() {
        for cycle in 0u64..100 {
            let j = compute_jitter_seconds(cycle, 30);
            assert!(j < 30, "cycle {cycle} jitter {j} out of bound");
        }
    }

    #[test]
    fn daemon_stops_when_flag_flips_immediately() {
        // Pre-set the flag → loop exits without ever running a cycle.
        let stop = Arc::new(AtomicBool::new(true));
        let cfg = fake_cfg();
        let cycles = run_daemon(&cfg, stop).expect("daemon");
        assert_eq!(cycles, 0);
    }

    #[test]
    fn daemon_runs_at_least_one_cycle_then_stops() {
        // Spawn daemon in a thread; flip stop after a moment.
        let stop = Arc::new(AtomicBool::new(false));
        let stop_handle = stop.clone();
        let cfg = fake_cfg();
        let handle = std::thread::spawn(move || run_daemon(&cfg, stop_handle));

        // Give it time to run at least one cycle (1s interval + cycle work).
        // The pull_once with file:///nonexistent will fail fetch but write a
        // heartbeat — that's fine, the daemon just keeps going.
        std::thread::sleep(Duration::from_millis(1500));
        stop.store(true, Ordering::SeqCst);

        let cycles = handle.join().expect("thread join").expect("daemon");
        assert!(cycles >= 1, "expected at least one cycle, got {cycles}");
    }
}
