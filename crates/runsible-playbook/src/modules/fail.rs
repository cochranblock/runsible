//! `runsible_builtin.fail` — always fail with a message.
//!
//! Args:
//!   msg = "string"   (default "Failed as requested")
//!
//! Used inside `when:` blocks for conditional bailouts.

use runsible_core::traits::ExecutionContext;
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::Result;

pub struct FailModule;

impl DynModule for FailModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.fail"
    }

    fn check_mode_safe(&self) -> bool {
        true
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let msg = args
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("Failed as requested")
            .to_string();
        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({"msg": msg}),
            will_change: true,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        let msg = plan
            .diff
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("Failed as requested")
            .to_string();
        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Failed,
            elapsed_ms: 0,
            returns: serde_json::json!({"failed": true, "msg": msg}),
        })
    }
}
