//! `runsible_builtin.cron` — manage entries in a user's crontab.
//!
//! Args:
//!   name    = "marker"   (required)
//!   user    = "alice"    (default: invoke `crontab` without -u, current user)
//!   minute  = "0"
//!   hour    = "*"
//!   day     = "*"
//!   month   = "*"
//!   weekday = "*"
//!   job     = "command to run"
//!   state   = "present" | "absent"
//!
//! Marker line `# Ansible: <name>` precedes the cron entry — same convention
//! as Ansible's stock module.

use runsible_core::traits::{Cmd, ExecutionContext};
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};

pub struct CronModule;

impl DynModule for CronModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.cron"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PlaybookError::TypeCheck("cron: missing required arg `name`".into()))?
            .to_string();
        let state = args
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("present")
            .to_string();
        if state != "present" && state != "absent" {
            return Err(PlaybookError::TypeCheck(format!(
                "cron: unknown state '{state}'"
            )));
        }
        let user = args.get("user").and_then(|v| v.as_str()).map(String::from);
        let minute = args.get("minute").and_then(|v| v.as_str()).unwrap_or("*").to_string();
        let hour = args.get("hour").and_then(|v| v.as_str()).unwrap_or("*").to_string();
        let day = args.get("day").and_then(|v| v.as_str()).unwrap_or("*").to_string();
        let month = args.get("month").and_then(|v| v.as_str()).unwrap_or("*").to_string();
        let weekday = args.get("weekday").and_then(|v| v.as_str()).unwrap_or("*").to_string();
        let job = args.get("job").and_then(|v| v.as_str()).unwrap_or("").to_string();

        if state == "present" && job.is_empty() {
            return Err(PlaybookError::TypeCheck(
                "cron: `job` is required when state=present".into(),
            ));
        }

        let current = read_crontab(user.as_deref(), ctx);
        let new = compute_crontab(&current, &name, &state, &minute, &hour, &day, &month, &weekday, &job);

        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({
                "name": name,
                "state": state,
                "user": user,
                "minute": minute,
                "hour": hour,
                "day": day,
                "month": month,
                "weekday": weekday,
                "job": job,
                "current_crontab": current.clone(),
                "new_crontab": new.clone(),
            }),
            will_change: new != current,
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
        let user = plan.diff.get("user").and_then(|v| v.as_str()).map(String::from);
        let new_crontab = plan
            .diff
            .get("new_crontab")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut argv: Vec<String> = vec!["crontab".into()];
        if let Some(u) = user.as_deref() {
            argv.push("-u".into());
            argv.push(u.into());
        }
        argv.push("-".into());

        let cmd = Cmd {
            argv: argv.clone(),
            stdin: Some(new_crontab.into_bytes()),
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
                    "stage": "crontab_install",
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
                "name": plan.diff["name"],
            }),
        })
    }
}

fn read_crontab(user: Option<&str>, ctx: &ExecutionContext) -> String {
    let mut argv: Vec<String> = vec!["crontab".into()];
    if let Some(u) = user {
        argv.push("-u".into());
        argv.push(u.into());
    }
    argv.push("-l".into());
    let cmd = Cmd {
        argv,
        stdin: None,
        env: vec![],
        cwd: None,
        become_: None,
        timeout: None,
        tty: false,
    };
    let out = match ctx.connection.exec(&cmd) {
        Ok(o) => o,
        Err(_) => return String::new(),
    };
    if out.rc != 0 {
        return String::new();
    }
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[allow(clippy::too_many_arguments)]
fn compute_crontab(
    current: &str,
    name: &str,
    state: &str,
    minute: &str,
    hour: &str,
    day: &str,
    month: &str,
    weekday: &str,
    job: &str,
) -> String {
    let marker = format!("# Ansible: {name}");
    let lines: Vec<&str> = current.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 2);

    let mut i = 0;
    let mut existed = false;
    while i < lines.len() {
        if lines[i] == marker {
            existed = true;
            // Skip marker + the next line (the cron entry).
            i += 1;
            if i < lines.len() {
                i += 1;
            }
            continue;
        }
        out.push(lines[i].to_string());
        i += 1;
    }

    if state == "present" {
        let entry = format!("{minute} {hour} {day} {month} {weekday} {job}");
        out.push(marker);
        out.push(entry);
    } else if !existed {
        // already absent and was absent — leave file alone.
    }

    let mut s = out.join("\n");
    if !s.is_empty() {
        s.push('\n');
    }
    s
}
