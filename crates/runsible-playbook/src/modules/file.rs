//! `runsible_builtin.file` — manage filesystem entries.
//!
//! Args:
//!   path  = "/some/path"   (required)
//!   state = "present" | "absent" | "directory" | "touch"   (default "present")
//!   mode  = "0644"   (octal string, optional)
//!
//! Idempotence:
//!   state=present + path exists → no change
//!   state=absent + path missing → no change
//!   state=directory + dir exists → no change
//!   state=touch → always changes (updates mtime)

use std::path::Path;

use runsible_core::traits::{Cmd, ExecutionContext};
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};

pub struct FileModule;

impl DynModule for FileModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.file"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PlaybookError::TypeCheck("file: missing required arg `path`".into()))?
            .to_string();
        let state = args
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("present")
            .to_string();
        let mode = args.get("mode").and_then(|v| v.as_str()).map(String::from);

        let exists = ctx.connection.file_exists(Path::new(&path)).unwrap_or(false);

        let will_change = match state.as_str() {
            "absent" => exists,
            "present" | "directory" => !exists,
            "touch" => true,
            _ => return Err(PlaybookError::TypeCheck(format!(
                "file: unknown state '{state}'"
            ))),
        };

        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({
                "path": path,
                "state": state,
                "mode": mode,
                "currently_exists": exists,
            }),
            will_change,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        if !plan.will_change {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Ok,
                elapsed_ms: 0,
                returns: serde_json::json!({"changed": false, "path": plan.diff["path"]}),
            });
        }

        let path = plan.diff.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let state = plan.diff.get("state").and_then(|v| v.as_str()).unwrap_or("present");
        let mode = plan
            .diff
            .get("mode")
            .and_then(|v| v.as_str())
            .and_then(|s| u32::from_str_radix(s.trim_start_matches('0'), 8).ok());

        let started = std::time::Instant::now();

        // Build the `cmd` to do the filesystem op via the connection so the
        // module works over both local and (future) remote sync connections.
        let argv: Vec<String> = match state {
            "absent" => vec!["rm".into(), "-rf".into(), path.into()],
            "present" => vec!["touch".into(), path.into()],
            "touch" => vec!["touch".into(), path.into()],
            "directory" => vec!["mkdir".into(), "-p".into(), path.into()],
            _ => unreachable!("validated in plan()"),
        };

        let cmd = Cmd {
            argv,
            stdin: None,
            env: vec![],
            cwd: None,
            become_: None,
            timeout: None,
            tty: false,
        };

        let exec_out = ctx.connection.exec(&cmd).map_err(|e| PlaybookError::ExecFailed {
            host: ctx.host.name.clone(),
            message: e.to_string(),
        })?;

        if exec_out.rc != 0 {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Failed,
                elapsed_ms: started.elapsed().as_millis() as u64,
                returns: serde_json::json!({
                    "rc": exec_out.rc,
                    "stderr": String::from_utf8_lossy(&exec_out.stderr).into_owned(),
                    "path": path,
                    "state": state,
                }),
            });
        }

        // Apply mode if requested and target still exists.
        if let Some(m) = mode {
            if ctx.connection.file_exists(Path::new(path)).unwrap_or(false) {
                let chmod = Cmd {
                    argv: vec!["chmod".into(), format!("{:o}", m), path.into()],
                    stdin: None,
                    env: vec![],
                    cwd: None,
                    become_: None,
                    timeout: None,
                    tty: false,
                };
                let _ = ctx.connection.exec(&chmod);
            }
        }

        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Changed,
            elapsed_ms: started.elapsed().as_millis() as u64,
            returns: serde_json::json!({
                "changed": true,
                "path": path,
                "state": state,
            }),
        })
    }
}
