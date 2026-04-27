//! `runsible_builtin.stat` — read-only inspection of a path.
//!
//! Args:
//!   path                = "/path"   (required)
//!   checksum_algorithm  = "sha256"  (default; sha1/md5/sha512 also OK)
//!   get_checksum        = true | false   (default true)
//!
//! Returns: stat dict with `exists`, `path`, `size`, `mode`, `isdir`, `isfile`,
//! `islnk`, `mtime`, `checksum` (when applicable).
//!
//! `will_change` is always false — stat doesn't mutate.

use std::path::Path;

use runsible_core::traits::{Cmd, ExecutionContext};
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};

pub struct StatModule;

impl DynModule for StatModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.stat"
    }

    fn check_mode_safe(&self) -> bool {
        true
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PlaybookError::TypeCheck("stat: missing required arg `path`".into()))?
            .to_string();
        let algo = args
            .get("checksum_algorithm")
            .and_then(|v| v.as_str())
            .unwrap_or("sha256")
            .to_string();
        let get_checksum = args
            .get("get_checksum")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({
                "path": path,
                "algo": algo,
                "get_checksum": get_checksum,
            }),
            will_change: false,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        let started = std::time::Instant::now();
        let path = plan.diff.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let algo = plan.diff.get("algo").and_then(|v| v.as_str()).unwrap_or("sha256");
        let get_checksum = plan
            .diff
            .get("get_checksum")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let exists = ctx.connection.file_exists(Path::new(path)).unwrap_or(false);
        if !exists {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Ok,
                elapsed_ms: started.elapsed().as_millis() as u64,
                returns: serde_json::json!({
                    "stat": {
                        "exists": false,
                        "path": path,
                    },
                }),
            });
        }

        // stat -c '%s|%a|%F|%Y' <path>
        let cmd = Cmd {
            argv: vec![
                "stat".into(),
                "-c".into(),
                "%s|%a|%F|%Y".into(),
                path.into(),
            ],
            stdin: None,
            env: vec![],
            cwd: None,
            become_: None,
            timeout: None,
            tty: false,
        };
        let out = ctx.connection.exec(&cmd).map_err(|e| PlaybookError::ExecFailed {
            host: ctx.host.name.clone(),
            message: e.to_string(),
        })?;
        if out.rc != 0 {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Failed,
                elapsed_ms: started.elapsed().as_millis() as u64,
                returns: serde_json::json!({
                    "stage": "stat",
                    "rc": out.rc,
                    "stderr": String::from_utf8_lossy(&out.stderr).into_owned(),
                }),
            });
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let parts: Vec<&str> = stdout.trim_end_matches('\n').split('|').collect();
        let size: u64 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
        let mode = parts.get(1).map(|s| s.to_string()).unwrap_or_default();
        let kind = parts.get(2).map(|s| s.to_string()).unwrap_or_default();
        let mtime: u64 = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
        let isdir = kind == "directory";
        let isfile = kind == "regular file" || kind == "regular empty file";
        let islnk = kind == "symbolic link";

        let mut stat = serde_json::json!({
            "exists": true,
            "path": path,
            "size": size,
            "mode": mode,
            "isdir": isdir,
            "isfile": isfile,
            "islnk": islnk,
            "mtime": mtime,
            "kind": kind,
        });

        if get_checksum && isfile {
            if let Some(cs) = compute_checksum(algo, path, ctx) {
                stat["checksum"] = serde_json::Value::String(cs);
                stat["checksum_algorithm"] = serde_json::Value::String(algo.to_string());
            }
        }

        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Ok,
            elapsed_ms: started.elapsed().as_millis() as u64,
            returns: serde_json::json!({
                "stat": stat,
                "exists": true,
                "size": size,
            }),
        })
    }
}

fn compute_checksum(algo: &str, path: &str, ctx: &ExecutionContext) -> Option<String> {
    let bin = match algo {
        "sha256" => "sha256sum",
        "sha1" => "sha1sum",
        "md5" => "md5sum",
        "sha512" => "sha512sum",
        _ => return None,
    };
    let cmd = Cmd {
        argv: vec![bin.into(), path.into()],
        stdin: None,
        env: vec![],
        cwd: None,
        become_: None,
        timeout: None,
        tty: false,
    };
    let out = ctx.connection.exec(&cmd).ok()?;
    if out.rc != 0 {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .map(|s| s.to_string())
}
