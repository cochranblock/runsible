//! `runsible_builtin.command` — execute a command without a shell.
//!
//! Args:
//!   argv = ["bin", "arg1", "arg2"]    (preferred)
//!   cmd  = "bin arg1 arg2"             (whitespace-split, no shell semantics)
//!   creates = "/path"                  (skip if path exists)
//!   removes = "/path"                  (skip if path absent)
//!   chdir  = "/working/dir"
//!
//! `command` is NOT idempotent — every run executes the binary. We mark
//! `will_change: true` so it always fires; the apply outcome is `Changed` on
//! rc=0, `Failed` otherwise.

use std::path::Path;

use runsible_core::traits::{Cmd, ExecutionContext};
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::Result;

pub struct CommandModule;

impl DynModule for CommandModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.command"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let argv = extract_argv(args);
        let creates = args.get("creates").and_then(|v| v.as_str()).map(String::from);
        let removes = args.get("removes").and_then(|v| v.as_str()).map(String::from);
        let chdir = args.get("chdir").and_then(|v| v.as_str()).map(String::from);

        // creates/removes guard — if the path indicates a no-op, mark as not changing.
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
                "argv": argv,
                "chdir": chdir,
                "creates": creates,
                "removes": removes,
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

        let argv: Vec<String> = plan
            .diff
            .get("argv")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let chdir = plan
            .diff
            .get("chdir")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from);

        let cmd = Cmd {
            argv: argv.clone(),
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

        let stdout = String::from_utf8_lossy(&exec_out.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&exec_out.stderr).into_owned();
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
                "stdout": stdout,
                "stderr": stderr,
                "cmd": argv,
            }),
        })
    }
}

fn extract_argv(args: &toml::Value) -> Vec<String> {
    if let Some(arr) = args.get("argv").and_then(|v| v.as_array()) {
        return arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
    }
    if let Some(s) = args.get("cmd").and_then(|v| v.as_str()) {
        return s.split_whitespace().map(String::from).collect();
    }
    if let Some(s) = args.as_str() {
        return s.split_whitespace().map(String::from).collect();
    }
    vec![]
}
