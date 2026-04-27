//! `runsible_builtin.systemd_service` — manage a systemd unit (system or user scope).
//!
//! Args:
//!   name          = "nginx"   (required)
//!   state         = "started" | "stopped" | "restarted" | "reloaded"  (optional)
//!   enabled       = true | false                                       (optional)
//!   daemon_reload = true | false   (optional, default false)
//!   scope         = "system" | "user"  (optional, default "system")
//!
//! Idempotence semantics match `service`. The extra args (`daemon_reload`,
//! `scope`) ride along in the plan diff and are honored at apply time.

use runsible_core::traits::ExecutionContext;
use runsible_core::types::{Outcome, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};
use crate::modules::systemd_helpers::{
    apply_state, is_active, is_enabled, validate_state, SystemdScope,
};

pub struct SystemdServiceModule;

impl DynModule for SystemdServiceModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.systemd_service"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                PlaybookError::TypeCheck("systemd_service: missing required arg `name`".into())
            })?
            .to_string();
        let state = args.get("state").and_then(|v| v.as_str()).map(String::from);
        if let Some(s) = &state {
            validate_state(s).map_err(PlaybookError::TypeCheck)?;
        }
        let enabled = args.get("enabled").and_then(|v| v.as_bool());
        let daemon_reload = args
            .get("daemon_reload")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let scope_str = args
            .get("scope")
            .and_then(|v| v.as_str())
            .unwrap_or("system")
            .to_string();
        let scope = SystemdScope::from_str(&scope_str).map_err(PlaybookError::TypeCheck)?;

        if state.is_none() && enabled.is_none() && !daemon_reload {
            return Err(PlaybookError::TypeCheck(
                "systemd_service: must provide at least one of `state`, `enabled`, or `daemon_reload`".into(),
            ));
        }

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

        // daemon_reload is treated as "always do it if requested"; it forces a change.
        let will_change = will_change_state || will_change_enabled || daemon_reload;

        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({
                "name": name,
                "state": state,
                "enabled": enabled,
                "scope": scope_str,
                "daemon_reload": daemon_reload,
            }),
            will_change,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        apply_state(plan, ctx)
    }
}
