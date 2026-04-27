//! `runsible_builtin.user` — manage Unix users.
//!
//! Args:
//!   name        = "alice"   (required)
//!   state       = "present" | "absent"   (default "present")
//!   uid         = 1234
//!   group       = "primary"
//!   groups      = ["g1", "g2"]
//!   shell       = "/bin/bash"
//!   home        = "/home/alice"
//!   password    = "<already-hashed>"
//!   system      = true
//!   create_home = true
//!
//! Idempotence: `getent passwd <name>` to check if the user exists.

use runsible_core::traits::{Cmd, ExecutionContext};
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};

pub struct UserModule;

impl DynModule for UserModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.user"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PlaybookError::TypeCheck("user: missing required arg `name`".into()))?
            .to_string();
        let state = args
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("present")
            .to_string();
        if state != "present" && state != "absent" {
            return Err(PlaybookError::TypeCheck(format!(
                "user: unknown state '{state}'"
            )));
        }

        let uid = args.get("uid").and_then(|v| v.as_integer()).map(|n| n as i64);
        let group = args.get("group").and_then(|v| v.as_str()).map(String::from);
        let groups = args
            .get("groups")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>())
            .unwrap_or_default();
        let shell = args.get("shell").and_then(|v| v.as_str()).map(String::from);
        let home = args.get("home").and_then(|v| v.as_str()).map(String::from);
        let password = args.get("password").and_then(|v| v.as_str()).map(String::from);
        let system = args.get("system").and_then(|v| v.as_bool()).unwrap_or(false);
        let create_home = args.get("create_home").and_then(|v| v.as_bool()).unwrap_or(true);

        let exists = user_exists(&name, ctx);

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
                "uid": uid,
                "group": group,
                "groups": groups,
                "shell": shell,
                "home": home,
                "password": password,
                "system": system,
                "create_home": create_home,
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
            "absent" => vec!["userdel".into(), "-r".into(), name.clone()],
            "present" => {
                let mut a: Vec<String> = vec!["useradd".into()];
                let create_home = plan.diff.get("create_home").and_then(|v| v.as_bool()).unwrap_or(true);
                if create_home {
                    a.push("-m".into());
                }
                if plan.diff.get("system").and_then(|v| v.as_bool()).unwrap_or(false) {
                    a.push("-r".into());
                }
                if let Some(uid) = plan.diff.get("uid").and_then(|v| v.as_i64()) {
                    a.push("-u".into());
                    a.push(uid.to_string());
                }
                if let Some(g) = plan.diff.get("group").and_then(|v| v.as_str()) {
                    a.push("-g".into());
                    a.push(g.to_string());
                }
                let groups: Vec<String> = plan
                    .diff
                    .get("groups")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                if !groups.is_empty() {
                    a.push("-G".into());
                    a.push(groups.join(","));
                }
                if let Some(s) = plan.diff.get("shell").and_then(|v| v.as_str()) {
                    a.push("-s".into());
                    a.push(s.to_string());
                }
                if let Some(h) = plan.diff.get("home").and_then(|v| v.as_str()) {
                    a.push("-d".into());
                    a.push(h.to_string());
                }
                if let Some(p) = plan.diff.get("password").and_then(|v| v.as_str()) {
                    a.push("-p".into());
                    a.push(p.to_string());
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
                    "stage": "useradd_userdel",
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

fn user_exists(name: &str, ctx: &ExecutionContext) -> bool {
    let cmd = Cmd {
        argv: vec!["getent".into(), "passwd".into(), name.into()],
        stdin: None,
        env: vec![],
        cwd: None,
        become_: None,
        timeout: None,
        tty: false,
    };
    ctx.connection.exec(&cmd).map(|o| o.rc == 0).unwrap_or(false)
}
