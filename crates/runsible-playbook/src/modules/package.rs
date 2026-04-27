//! `runsible_builtin.package` — install/remove/upgrade OS packages.
//!
//! Args:
//!   name    = "nginx"   OR   name = ["nginx", "curl"]
//!   state   = "present" | "absent" | "latest"   (default "present")
//!   manager = "apt" | "dnf" | "yum" | "auto"    (default "auto")
//!
//! Manager auto-detection runs `which apt-get` then `which dnf` via
//! ctx.connection.exec — nothing is cached. M1 simplicity.
//!
//! Idempotence:
//!   present + installed       → will_change=false
//!   absent  + not installed   → will_change=false
//!   latest                    → will_change=true (always)

use runsible_core::traits::{Cmd, ExecutionContext};
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};

pub struct PackageModule;

impl DynModule for PackageModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.package"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let names = extract_names(args).ok_or_else(|| {
            PlaybookError::TypeCheck("package: missing required arg `name`".into())
        })?;
        if names.is_empty() {
            return Err(PlaybookError::TypeCheck(
                "package: `name` must contain at least one package".into(),
            ));
        }
        let state = args
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("present")
            .to_string();
        match state.as_str() {
            "present" | "absent" | "latest" => {}
            other => {
                return Err(PlaybookError::TypeCheck(format!(
                    "package: unknown state '{other}'"
                )));
            }
        }
        let manager_arg = args
            .get("manager")
            .and_then(|v| v.as_str())
            .unwrap_or("auto")
            .to_string();

        let manager = resolve_manager(&manager_arg, ctx)?;

        let will_change = match state.as_str() {
            "latest" => true,
            "present" => names.iter().any(|n| !is_installed(&manager, n, ctx)),
            "absent" => names.iter().any(|n| is_installed(&manager, n, ctx)),
            _ => unreachable!("validated above"),
        };

        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({
                "names": names,
                "state": state,
                "manager": manager,
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
                returns: serde_json::json!({"changed": false, "names": plan.diff["names"]}),
            });
        }

        let names: Vec<String> = plan
            .diff
            .get("names")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let state = plan.diff.get("state").and_then(|v| v.as_str()).unwrap_or("present");
        let manager = plan.diff.get("manager").and_then(|v| v.as_str()).unwrap_or("apt");

        let argv = build_apply_argv(manager, state, &names)?;

        let cmd = Cmd {
            argv: argv.clone(),
            stdin: None,
            env: vec![],
            cwd: None,
            become_: None,
            timeout: None,
            tty: false,
        };

        let started = std::time::Instant::now();
        let exec_out = ctx.connection.exec(&cmd).map_err(|e| PlaybookError::ExecFailed {
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
                "manager": manager,
            }),
        })
    }
}

fn extract_names(args: &toml::Value) -> Option<Vec<String>> {
    let n = args.get("name")?;
    if let Some(s) = n.as_str() {
        return Some(vec![s.to_string()]);
    }
    if let Some(arr) = n.as_array() {
        return Some(arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
    }
    None
}

fn resolve_manager(requested: &str, ctx: &ExecutionContext) -> Result<String> {
    match requested {
        "apt" | "dnf" | "yum" => Ok(requested.to_string()),
        "auto" => {
            if which_ok("apt-get", ctx) {
                Ok("apt".into())
            } else if which_ok("dnf", ctx) {
                Ok("dnf".into())
            } else if which_ok("yum", ctx) {
                Ok("yum".into())
            } else {
                Err(PlaybookError::TypeCheck(
                    "package: could not auto-detect a package manager (no apt-get/dnf/yum)".into(),
                ))
            }
        }
        other => Err(PlaybookError::TypeCheck(format!(
            "package: unknown manager '{other}'"
        ))),
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

fn is_installed(manager: &str, name: &str, ctx: &ExecutionContext) -> bool {
    let argv = match manager {
        "apt" => vec!["dpkg".into(), "-l".into(), name.into()],
        "dnf" | "yum" => vec!["rpm".into(), "-q".into(), name.into()],
        _ => return false,
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
    ctx.connection.exec(&cmd).map(|o| o.rc == 0).unwrap_or(false)
}

fn build_apply_argv(manager: &str, state: &str, names: &[String]) -> Result<Vec<String>> {
    let mut argv: Vec<String> = match (manager, state) {
        ("apt", "present") => vec!["apt-get".into(), "install".into(), "-y".into()],
        ("apt", "absent") => vec!["apt-get".into(), "remove".into(), "-y".into()],
        ("apt", "latest") => vec![
            "apt-get".into(),
            "install".into(),
            "--only-upgrade".into(),
            "-y".into(),
        ],
        ("dnf", "present") => vec!["dnf".into(), "install".into(), "-y".into()],
        ("dnf", "absent") => vec!["dnf".into(), "remove".into(), "-y".into()],
        ("dnf", "latest") => vec!["dnf".into(), "upgrade".into(), "-y".into()],
        ("yum", "present") => vec!["yum".into(), "install".into(), "-y".into()],
        ("yum", "absent") => vec!["yum".into(), "remove".into(), "-y".into()],
        ("yum", "latest") => vec!["yum".into(), "upgrade".into(), "-y".into()],
        (m, s) => {
            return Err(PlaybookError::TypeCheck(format!(
                "package: unsupported manager/state combination ({m}/{s})"
            )));
        }
    };
    argv.extend(names.iter().cloned());
    Ok(argv)
}
