//! `runsible_builtin.shell` — execute a shell command via `sh -c`.
//!
//! Args:
//!   cmd     = "echo hi | tr a-z A-Z"
//!   chdir   = "/working/dir"
//!   creates = "/path"
//!   removes = "/path"
//!   executable = "/bin/bash"   (default: /bin/sh)
//!
//! Like `command`, `shell` is non-idempotent — `will_change: true` always
//! unless creates/removes guard says otherwise.

use std::path::Path;

use runsible_core::traits::{Cmd, ExecutionContext};
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::Result;

pub struct ShellModule;

impl DynModule for ShellModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.shell"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let cmd_str = args
            .get("cmd")
            .and_then(|v| v.as_str())
            .or_else(|| args.as_str())
            .unwrap_or("")
            .to_string();
        let chdir = args.get("chdir").and_then(|v| v.as_str()).map(String::from);
        let creates = args.get("creates").and_then(|v| v.as_str()).map(String::from);
        let removes = args.get("removes").and_then(|v| v.as_str()).map(String::from);
        let executable = args
            .get("executable")
            .and_then(|v| v.as_str())
            .unwrap_or("/bin/sh")
            .to_string();

        let mut will_change = true;
        if let Some(p) = &creates {
            if ctx.connection.file_exists(Path::new(p)).unwrap_or(false) {
                will_change = false;
            }
        }
        if let Some(p) = &removes {
            if !ctx.connection.file_exists(Path::new(p)).unwrap_or(false) {
                will_change = false;
            }
        }

        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({
                "cmd": cmd_str,
                "chdir": chdir,
                "creates": creates,
                "removes": removes,
                "executable": executable,
            }),
            will_change,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        if !plan.will_change {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Skipped,
                elapsed_ms: 0,
                returns: serde_json::json!({"skipped_reason": "creates/removes guard satisfied"}),
            });
        }

        let cmd_str = plan.diff.get("cmd").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let executable = plan
            .diff
            .get("executable")
            .and_then(|v| v.as_str())
            .unwrap_or("/bin/sh")
            .to_string();
        let chdir = plan
            .diff
            .get("chdir")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from);

        let cmd = Cmd {
            argv: vec![executable, "-c".into(), cmd_str.clone()],
            stdin: None,
            env: vec![],
            cwd: chdir,
            become_: None,
            timeout: None,
            tty: false,
        };

        let started = std::time::Instant::now();
        let exec_out = ctx
            .connection
            .exec(&cmd)
            .map_err(|e| crate::errors::PlaybookError::ExecFailed {
                host: ctx.host.name.clone(),
                message: e.to_string(),
            })?;
        let elapsed_ms = started.elapsed().as_millis() as u64;

        let status = if exec_out.rc == 0 {
            OutcomeStatus::Changed
        } else {
            OutcomeStatus::Failed
        };

        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status,
            elapsed_ms,
            returns: serde_json::json!({
                "rc": exec_out.rc,
                "stdout": String::from_utf8_lossy(&exec_out.stdout).into_owned(),
                "stderr": String::from_utf8_lossy(&exec_out.stderr).into_owned(),
                "cmd": cmd_str,
            }),
        })
    }
}
