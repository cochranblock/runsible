//! `runsible_builtin.hostname` — set the system hostname.
//!
//! Args:
//!   name = "myhost"   (required)
//!
//! Idempotence: read current `hostname` and compare.

use runsible_core::traits::{Cmd, ExecutionContext};
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};

pub struct HostnameModule;

impl DynModule for HostnameModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.hostname"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PlaybookError::TypeCheck("hostname: missing required arg `name`".into()))?
            .to_string();

        let current = read_hostname(ctx);
        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({
                "name": name,
                "current": current,
            }),
            will_change: current != name,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        if !plan.will_change {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Ok,
                elapsed_ms: 0,
                returns: serde_json::json!({"changed": false, "name": plan.diff["name"]}),
            });
        }
        let started = std::time::Instant::now();
        let name = plan.diff.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();

        // Prefer hostnamectl when available.
        let mut argv: Vec<String> = if which_ok("hostnamectl", ctx) {
            vec!["hostnamectl".into(), "set-hostname".into(), name.clone()]
        } else {
            vec!["hostname".into(), name.clone()]
        };

        // First attempt.
        let mut cmd = Cmd {
            argv: argv.clone(),
            stdin: None,
            env: vec![],
            cwd: None,
            become_: None,
            timeout: None,
            tty: false,
        };
        let mut out = ctx.connection.exec(&cmd).map_err(|e| PlaybookError::ExecFailed {
            host: ctx.host.name.clone(),
            message: e.to_string(),
        })?;
        if out.rc != 0 {
            // Fall back to writing /etc/hostname + plain hostname call.
            argv = vec!["sh".into(), "-c".into(), format!("echo {name} > /etc/hostname && hostname {name}")];
            cmd = Cmd {
                argv: argv.clone(),
                stdin: None,
                env: vec![],
                cwd: None,
                become_: None,
                timeout: None,
                tty: false,
            };
            out = ctx.connection.exec(&cmd).map_err(|e| PlaybookError::ExecFailed {
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
                        "stage": "hostname_set",
                        "rc": out.rc,
                        "stderr": String::from_utf8_lossy(&out.stderr).into_owned(),
                        "cmd": argv,
                    }),
                });
            }
        }

        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Changed,
            elapsed_ms: started.elapsed().as_millis() as u64,
            returns: serde_json::json!({"changed": true, "name": name}),
        })
    }
}

fn read_hostname(ctx: &ExecutionContext) -> String {
    let cmd = Cmd {
        argv: vec!["hostname".into()],
        stdin: None,
        env: vec![],
        cwd: None,
        become_: None,
        timeout: None,
        tty: false,
    };
    match ctx.connection.exec(&cmd) {
        Ok(o) if o.rc == 0 => String::from_utf8_lossy(&o.stdout).trim_end().to_string(),
        _ => String::new(),
    }
}

fn which_ok(bin: &str, ctx: &ExecutionContext) -> bool {
    let cmd = Cmd {
        argv: vec!["which".into(), bin.into()],
        stdin: None,
        env: vec![],
        cwd: None,
        become_: None,
        timeout: None,
        tty: false,
    };
    ctx.connection.exec(&cmd).map(|o| o.rc == 0).unwrap_or(false)
}
