//! `runsible_builtin.debug` — print a message or dump a variable.
//!
//! In Ansible, `debug` is always `ok` (never `changed`). We model this by
//! setting `will_change: false` in `plan()` but still calling `apply()` —
//! at M0 the engine always calls apply() regardless of plan.is_empty().

use runsible_core::traits::ExecutionContext;
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::Result;

pub struct DebugModule;

impl DynModule for DebugModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.debug"
    }

    fn check_mode_safe(&self) -> bool {
        true
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let msg = extract_msg(args);
        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({ "msg": msg }),
            will_change: false,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        let msg = plan
            .diff
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Ok,
            elapsed_ms: 0,
            returns: serde_json::json!({ "msg": msg }),
        })
    }
}

fn extract_msg(args: &toml::Value) -> String {
    // `debug = { msg = "..." }` — most common form
    if let Some(s) = args.get("msg").and_then(|v| v.as_str()) {
        return s.to_string();
    }
    // `debug = "bare string"` — shorthand
    if let Some(s) = args.as_str() {
        return s.to_string();
    }
    // Fallback: dump the whole args as TOML inline
    toml::to_string(args).unwrap_or_else(|_| format!("{args:?}"))
}
