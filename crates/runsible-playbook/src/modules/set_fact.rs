//! `runsible_builtin.set_fact` — inject arbitrary key=value pairs into the
//! host's vars.
//!
//! In Ansible, `set_fact` always reports `ok` (never `changed`) but it does
//! mutate state by adding facts. We model the mutation intent with
//! `will_change: true` (so the engine knows to merge the diff) and the
//! reported status as `Ok` to mirror Ansible.
//!
//! For now, this module just packages the args into the plan diff. The engine
//! is responsible for reading `plan.diff` and merging it into the host's vars
//! after `apply()` returns. Expression evaluation and templating happen
//! engine-side where the templater lives.

use runsible_core::types::{Host, Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::Result;

pub struct SetFactModule;

impl DynModule for SetFactModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.set_fact"
    }

    fn plan(&self, args: &toml::Value, host: &Host) -> Result<Plan> {
        // Clone the entire args table as JSON for the engine to merge later.
        let diff = toml_to_json(args);
        Ok(Plan {
            module: self.module_name().into(),
            host: host.name.clone(),
            diff,
            will_change: true,
        })
    }

    fn apply(&self, plan: &Plan, host: &Host) -> Result<Outcome> {
        Ok(Outcome {
            module: plan.module.clone(),
            host: host.name.clone(),
            status: OutcomeStatus::Ok,
            elapsed_ms: 0,
            returns: plan.diff.clone(),
        })
    }
}

/// Convert a `toml::Value` into a `serde_json::Value`. Falls back to JSON
/// `Null` if the value cannot be serialized.
fn toml_to_json(v: &toml::Value) -> serde_json::Value {
    serde_json::to_value(v).unwrap_or(serde_json::Value::Null)
}
