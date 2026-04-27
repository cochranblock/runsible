//! `runsible_builtin.ping` — connectivity check, always ok, never changes.
//!
//! Returns `{"ping": "pong"}` in both plan diff and outcome returns.

use runsible_core::traits::ExecutionContext;
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::Result;

pub struct PingModule;

impl DynModule for PingModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.ping"
    }

    fn plan(&self, _args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({ "ping": "pong" }),
            will_change: false,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Ok,
            elapsed_ms: 0,
            returns: serde_json::json!({ "ping": "pong" }),
        })
    }
}
