//! `runsible_builtin.archive` — create a tar/zip archive on the host.
//!
//! Args:
//!   path   = "/file" or ["/a", "/b"]  (required)
//!   dest   = "/output.tar.gz"         (required)
//!   format = "gz" | "bz2" | "xz" | "zip" | "tar"   (default "gz")
//!   remove = true | false             (delete originals after archiving)
//!
//! Idempotence: if `dest` already exists we don't recreate it. Re-running with
//! the same args is a no-op once the archive is in place.

use std::path::Path;

use runsible_core::traits::{Cmd, ExecutionContext};
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};

pub struct ArchiveModule;

impl DynModule for ArchiveModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.archive"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let paths = extract_string_list(args.get("path"))
            .ok_or_else(|| PlaybookError::TypeCheck("archive: missing required arg `path`".into()))?;
        if paths.is_empty() {
            return Err(PlaybookError::TypeCheck(
                "archive: `path` must contain at least one entry".into(),
            ));
        }
        let dest = args
            .get("dest")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PlaybookError::TypeCheck("archive: missing required arg `dest`".into()))?
            .to_string();
        let format = args
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("gz")
            .to_string();
        let remove = args.get("remove").and_then(|v| v.as_bool()).unwrap_or(false);

        match format.as_str() {
            "gz" | "bz2" | "xz" | "zip" | "tar" => {}
            other => {
                return Err(PlaybookError::TypeCheck(format!(
                    "archive: unsupported format '{other}'"
                )));
            }
        }

        let exists = ctx.connection.file_exists(Path::new(&dest)).unwrap_or(false);

        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({
                "paths": paths,
                "dest": dest,
                "format": format,
                "remove": remove,
                "exists": exists,
            }),
            will_change: !exists,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        if !plan.will_change {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Ok,
                elapsed_ms: 0,
                returns: serde_json::json!({"changed": false, "dest": plan.diff["dest"]}),
            });
        }

        let started = std::time::Instant::now();
        let paths: Vec<String> = plan
            .diff
            .get("paths")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let dest = plan.diff.get("dest").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let format = plan
            .diff
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("gz")
            .to_string();
        let remove = plan.diff.get("remove").and_then(|v| v.as_bool()).unwrap_or(false);

        let argv = match format.as_str() {
            "zip" => {
                let mut a: Vec<String> = vec!["zip".into(), "-r".into(), dest.clone()];
                a.extend(paths.iter().cloned());
                a
            }
            other => {
                let flag = match other {
                    "gz" => "czf",
                    "bz2" => "cjf",
                    "xz" => "cJf",
                    "tar" => "cf",
                    _ => "czf",
                };
                let mut a: Vec<String> = vec!["tar".into(), flag.into(), dest.clone()];
                a.extend(paths.iter().cloned());
                a
            }
        };

        let cmd = Cmd {
            argv: argv.clone(),
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
                    "stage": "archive",
                    "rc": out.rc,
                    "stderr": String::from_utf8_lossy(&out.stderr).into_owned(),
                    "cmd": argv,
                }),
            });
        }

        if remove {
            for p in &paths {
                let rm = Cmd {
                    argv: vec!["rm".into(), "-rf".into(), p.clone()],
                    stdin: None,
                    env: vec![],
                    cwd: None,
                    become_: None,
                    timeout: None,
                    tty: false,
                };
                let _ = ctx.connection.exec(&rm);
            }
        }

        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Changed,
            elapsed_ms: started.elapsed().as_millis() as u64,
            returns: serde_json::json!({
                "changed": true,
                "dest": dest,
                "format": format,
            }),
        })
    }
}

fn extract_string_list(v: Option<&toml::Value>) -> Option<Vec<String>> {
    let v = v?;
    if let Some(s) = v.as_str() {
        return Some(vec![s.to_string()]);
    }
    if let Some(arr) = v.as_array() {
        return Some(arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
    }
    None
}
