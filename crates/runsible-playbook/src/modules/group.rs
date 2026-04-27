//! `runsible_builtin.group` — manage Unix groups.
//!
//! Args:
//!   name   = "wheel"   (required)
//!   state  = "present" | "absent"   (default "present")
//!   gid    = 1234
//!   system = true | false

use runsible_core::traits::{Cmd, ExecutionContext};
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};

pub struct GroupModule;

impl DynModule for GroupModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.group"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PlaybookError::TypeCheck("group: missing required arg `name`".into()))?
            .to_string();
        let state = args
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("present")
            .to_string();
        if state != "present" && state != "absent" {
            return Err(PlaybookError::TypeCheck(format!(
                "group: unknown state '{state}'"
            )));
        }
        let gid = args.get("gid").and_then(|v| v.as_integer()).map(|n| n as i64);
        let system = args.get("system").and_then(|v| v.as_bool()).unwrap_or(false);

        let exists = group_exists(&name, ctx);
        let will_change = match state.as_str() {
            "present" => !exists,
            "absent" => exists,
            _ => unreachable!(),
        };

        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({
                "name": name,
                "state": state,
                "gid": gid,
                "system": system,
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
                returns: serde_json::json!({"changed": false, "name": plan.diff["name"]}),
            });
        }
        let started = std::time::Instant::now();
        let state = plan.diff.get("state").and_then(|v| v.as_str()).unwrap_or("present");
        let name = plan.diff.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let argv: Vec<String> = match state {
            "absent" => vec!["groupdel".into(), name.clone()],
            "present" => {
                let mut a: Vec<String> = vec!["groupadd".into()];
                if plan.diff.get("system").and_then(|v| v.as_bool()).unwrap_or(false) {
                    a.push("-r".into());
                }
                if let Some(gid) = plan.diff.get("gid").and_then(|v| v.as_i64()) {
                    a.push("-g".into());
                    a.push(gid.to_string());
                }
                a.push(name.clone());
                a
            }
            _ => unreachable!(),
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
                    "stage": "groupadd_groupdel",
                    "rc": out.rc,
                    "stderr": String::from_utf8_lossy(&out.stderr).into_owned(),
                    "cmd": argv,
                }),
            });
        }

        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Changed,
            elapsed_ms: started.elapsed().as_millis() as u64,
            returns: serde_json::json!({
                "changed": true,
                "name": name,
                "state": state,
            }),
        })
    }
}

fn group_exists(name: &str, ctx: &ExecutionContext) -> bool {
    let cmd = Cmd {
        argv: vec!["getent".into(), "group".into(), name.into()],
        stdin: None,
        env: vec![],
        cwd: None,
        become_: None,
        timeout: None,
        tty: false,
    };
    ctx.connection.exec(&cmd).map(|o| o.rc == 0).unwrap_or(false)
}
