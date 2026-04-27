//! Shared systemd helpers used by both `service` and `systemd_service`.

use runsible_core::traits::{Cmd, ExecOutcome, ExecutionContext};
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::errors::{PlaybookError, Result};

#[derive(Clone, Copy)]
pub enum SystemdScope {
    System,
    User,
}

impl SystemdScope {
    pub fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s {
            "system" => Ok(SystemdScope::System),
            "user" => Ok(SystemdScope::User),
            other => Err(format!("invalid scope '{other}' (expected 'system' or 'user')")),
        }
    }

    fn flag(self) -> Option<&'static str> {
        match self {
            SystemdScope::System => None,
            SystemdScope::User => Some("--user"),
        }
    }
}

pub fn validate_state(s: &str) -> std::result::Result<(), String> {
    match s {
        "started" | "stopped" | "restarted" | "reloaded" => Ok(()),
        other => Err(format!(
            "unknown state '{other}' (expected started|stopped|restarted|reloaded)"
        )),
    }
}

fn systemctl_argv(scope: SystemdScope, sub: &[&str]) -> Vec<String> {
    let mut argv = vec!["systemctl".to_string()];
    if let Some(flag) = scope.flag() {
        argv.push(flag.into());
    }
    argv.extend(sub.iter().map(|s| s.to_string()));
    argv
}

pub fn is_active(name: &str, scope: SystemdScope, ctx: &ExecutionContext) -> bool {
    let argv = systemctl_argv(scope, &["is-active", name]);
    let cmd = Cmd {
        argv,
        stdin: None,
        env: vec![],
        cwd: None,
        become_: None,
        timeout: None,
        tty: false,
    };
    ctx.connection.exec(&cmd).map(|o| o.rc == 0).unwrap_or(false)
}

pub fn is_enabled(name: &str, scope: SystemdScope, ctx: &ExecutionContext) -> bool {
    let argv = systemctl_argv(scope, &["is-enabled", name]);
    let cmd = Cmd {
        argv,
        stdin: None,
        env: vec![],
        cwd: None,
        become_: None,
        timeout: None,
        tty: false,
    };
    ctx.connection.exec(&cmd).map(|o| o.rc == 0).unwrap_or(false)
}

pub fn run_systemctl(
    scope: SystemdScope,
    sub: &[&str],
    ctx: &ExecutionContext,
) -> Result<ExecOutcome> {
    let argv = systemctl_argv(scope, sub);
    let cmd = Cmd {
        argv,
        stdin: None,
        env: vec![],
        cwd: None,
        become_: None,
        timeout: None,
        tty: false,
    };
    ctx.connection.exec(&cmd).map_err(|e| PlaybookError::ExecFailed {
        host: ctx.host.name.clone(),
        message: e.to_string(),
    })
}

/// Drives the apply for both service and systemd_service.
/// Reads name, state, enabled, scope, daemon_reload from `plan.diff`.
pub fn apply_state(plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
    if !plan.will_change {
        return Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Ok,
            elapsed_ms: 0,
            returns: serde_json::json!({"changed": false, "name": plan.diff["name"]}),
        });
    }

    let name = plan.diff.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let state = plan.diff.get("state").and_then(|v| v.as_str()).map(String::from);
    let enabled = plan.diff.get("enabled").and_then(|v| v.as_bool());
    let scope_str = plan.diff.get("scope").and_then(|v| v.as_str()).unwrap_or("system");
    let scope = SystemdScope::from_str(scope_str).map_err(PlaybookError::TypeCheck)?;
    let daemon_reload = plan
        .diff
        .get("daemon_reload")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let started = std::time::Instant::now();
    let mut steps: Vec<serde_json::Value> = vec![];

    if daemon_reload {
        let out = run_systemctl(scope, &["daemon-reload"], ctx)?;
        steps.push(serde_json::json!({
            "step": "daemon-reload",
            "rc": out.rc,
        }));
        if out.rc != 0 {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Failed,
                elapsed_ms: started.elapsed().as_millis() as u64,
                returns: serde_json::json!({
                    "name": name,
                    "steps": steps,
                    "stderr": String::from_utf8_lossy(&out.stderr).into_owned(),
                }),
            });
        }
    }

    if let Some(s) = &state {
        let sub = match s.as_str() {
            "started" => "start",
            "stopped" => "stop",
            "restarted" => "restart",
            "reloaded" => "reload",
            _ => unreachable!("validated in plan()"),
        };
        let out = run_systemctl(scope, &[sub, &name], ctx)?;
        steps.push(serde_json::json!({
            "step": sub,
            "rc": out.rc,
            "stderr": String::from_utf8_lossy(&out.stderr).into_owned(),
        }));
        if out.rc != 0 {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Failed,
                elapsed_ms: started.elapsed().as_millis() as u64,
                returns: serde_json::json!({
                    "name": name,
                    "steps": steps,
                }),
            });
        }
    }

    if let Some(want) = enabled {
        let sub = if want { "enable" } else { "disable" };
        let out = run_systemctl(scope, &[sub, &name], ctx)?;
        steps.push(serde_json::json!({
            "step": sub,
            "rc": out.rc,
            "stderr": String::from_utf8_lossy(&out.stderr).into_owned(),
        }));
        if out.rc != 0 {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Failed,
                elapsed_ms: started.elapsed().as_millis() as u64,
                returns: serde_json::json!({
                    "name": name,
                    "steps": steps,
                }),
            });
        }
    }

    Ok(Outcome {
        module: plan.module.clone(),
        host: ctx.host.name.clone(),
        status: OutcomeStatus::Changed,
        elapsed_ms: started.elapsed().as_millis() as u64,
        returns: serde_json::json!({
            "changed": true,
            "name": name,
            "steps": steps,
        }),
    })
}
