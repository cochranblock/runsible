//! `runsible_builtin.service` — manage a service via systemctl.
//!
//! Args:
//!   name    = "nginx"   (required)
//!   state   = "started" | "stopped" | "restarted" | "reloaded"  (optional)
//!   enabled = true | false                                       (optional)
//!
//! M1 implementation always uses systemctl. Other init systems are M2.
//!
//! Idempotence:
//!   started/stopped → query `systemctl is-active`; skip if matches desired
//!   restarted/reloaded → always will_change=true
//!   enabled → query `systemctl is-enabled`; skip if matches desired

use runsible_core::traits::ExecutionContext;
use runsible_core::types::{Outcome, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};
use crate::modules::systemd_helpers::{
    apply_state, is_active, is_enabled, validate_state, SystemdScope,
};

pub struct ServiceModule;

impl DynModule for ServiceModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.service"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PlaybookError::TypeCheck("service: missing required arg `name`".into()))?
            .to_string();
        let state = args.get("state").and_then(|v| v.as_str()).map(String::from);
        if let Some(s) = &state {
            validate_state(s).map_err(PlaybookError::TypeCheck)?;
        }
        let enabled = args.get("enabled").and_then(|v| v.as_bool());

        if state.is_none() && enabled.is_none() {
            return Err(PlaybookError::TypeCheck(
                "service: must provide at least one of `state` or `enabled`".into(),
            ));
        }

        let scope = SystemdScope::System;
        let will_change_state = match state.as_deref() {
            Some("started") => !is_active(&name, scope, ctx),
            Some("stopped") => is_active(&name, scope, ctx),
            Some("restarted") | Some("reloaded") => true,
            Some(_) => unreachable!("validated above"),
            None => false,
        };

        let will_change_enabled = match enabled {
            Some(true) => !is_enabled(&name, scope, ctx),
            Some(false) => is_enabled(&name, scope, ctx),
            None => false,
        };

        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({
                "name": name,
                "state": state,
                "enabled": enabled,
                "scope": "system",
                "daemon_reload": false,
            }),
            will_change: will_change_state || will_change_enabled,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        apply_state(plan, ctx)
    }
}
