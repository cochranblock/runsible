//! `runsible_builtin.wait_for` — poll for a condition (port or file path).
//!
//! Args:
//!   host             = "localhost"   (default; for port-mode)
//!   port             = 22            (port-mode trigger)
//!   path             = "/some/file"  (file-mode trigger)
//!   state            = "started" | "stopped" | "present" | "absent"   (default "started")
//!   timeout          = 300           (seconds)
//!   delay            = 0             (initial sleep, seconds)
//!   connect_timeout  = 5             (seconds)
//!
//! Polls until the desired condition is met, or returns Failed once timeout
//! elapses.

use std::path::Path;
use std::time::{Duration, Instant};

use runsible_core::traits::ExecutionContext;
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};

pub struct WaitForModule;

impl DynModule for WaitForModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.wait_for"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let host = args
            .get("host")
            .and_then(|v| v.as_str())
            .unwrap_or("localhost")
            .to_string();
        let port = args
            .get("port")
            .and_then(|v| v.as_integer())
            .map(|i| i as u16);
        let path = args.get("path").and_then(|v| v.as_str()).map(String::from);
        let state = args
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("started")
            .to_string();
        let timeout = args
            .get("timeout")
            .and_then(|v| v.as_integer())
            .map(|i| i as u64)
            .unwrap_or(300);
        let delay = args
            .get("delay")
            .and_then(|v| v.as_integer())
            .map(|i| i as u64)
            .unwrap_or(0);
        let connect_timeout = args
            .get("connect_timeout")
            .and_then(|v| v.as_integer())
            .map(|i| i as u64)
            .unwrap_or(5);

        if port.is_none() && path.is_none() {
            return Err(PlaybookError::TypeCheck(
                "wait_for: must provide either `port` or `path`".into(),
            ));
        }

        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({
                "host": host,
                "port": port,
                "path": path,
                "state": state,
                "timeout": timeout,
                "delay": delay,
                "connect_timeout": connect_timeout,
            }),
            will_change: false,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        let started = Instant::now();
        let host = plan.diff.get("host").and_then(|v| v.as_str()).unwrap_or("localhost").to_string();
        let port = plan.diff.get("port").and_then(|v| v.as_u64()).map(|v| v as u16);
        let path = plan.diff.get("path").and_then(|v| v.as_str()).map(String::from);
        let state = plan.diff.get("state").and_then(|v| v.as_str()).unwrap_or("started").to_string();
        let timeout = plan.diff.get("timeout").and_then(|v| v.as_u64()).unwrap_or(300);
        let delay = plan.diff.get("delay").and_then(|v| v.as_u64()).unwrap_or(0);
        let connect_timeout = plan
            .diff
            .get("connect_timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(5);

        if delay > 0 {
            std::thread::sleep(Duration::from_secs(delay));
        }

        let deadline = started + Duration::from_secs(timeout);
        loop {
            let satisfied = if let Some(p) = port {
                let want_started = matches!(state.as_str(), "started" | "present");
                let got_started = port_is_open(&host, p, connect_timeout);
                got_started == want_started
            } else if let Some(pth) = &path {
                let want_present = matches!(state.as_str(), "present" | "started");
                let got_present = ctx
                    .connection
                    .file_exists(Path::new(pth))
                    .unwrap_or(false);
                got_present == want_present
            } else {
                true
            };
            if satisfied {
                return Ok(Outcome {
                    module: plan.module.clone(),
                    host: ctx.host.name.clone(),
                    status: OutcomeStatus::Ok,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    returns: serde_json::json!({
                        "matched": true,
                        "elapsed_seconds": started.elapsed().as_secs(),
                    }),
                });
            }
            if Instant::now() >= deadline {
                return Ok(Outcome {
                    module: plan.module.clone(),
                    host: ctx.host.name.clone(),
                    status: OutcomeStatus::Failed,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    returns: serde_json::json!({
                        "stage": "timeout",
                        "msg": "wait_for: timeout exceeded",
                        "host": host,
                        "port": port,
                        "path": path,
                        "state": state,
                    }),
                });
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

fn port_is_open(host: &str, port: u16, connect_timeout_secs: u64) -> bool {
    use std::net::ToSocketAddrs;
    let target = format!("{host}:{port}");
    let addr = match target.to_socket_addrs() {
        Ok(mut it) => match it.next() {
            Some(a) => a,
            None => return false,
        },
        Err(_) => return false,
    };
    std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(connect_timeout_secs)).is_ok()
}
