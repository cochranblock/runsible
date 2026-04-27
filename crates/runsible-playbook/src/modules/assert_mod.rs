//! `runsible_builtin.assert` — validate Jinja boolean expressions.
//!
//! Named `assert_mod` to avoid collision with the `assert!` macro and the
//! reserved `assert` identifier in some contexts.
//!
//! In Ansible, `assert` evaluates one or more boolean expressions in `that`
//! and reports `ok` when all are true, `failed` otherwise. It never changes
//! state.
//!
//! For now this module does NOT evaluate the expressions itself — it merely
//! packages `that`, `fail_msg`, and `success_msg` into the plan diff. The
//! engine will later call the templater to evaluate each expression and
//! downgrade the outcome to `Failed` if any expression is false.

use runsible_core::types::{Host, Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::Result;

pub struct AssertModule;

impl DynModule for AssertModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.assert"
    }

    fn plan(&self, args: &toml::Value, host: &Host) -> Result<Plan> {
        let that = toml_to_json(args.get("that").unwrap_or(&toml::Value::Array(vec![])));
        let fail_msg = args
            .get("fail_msg")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let success_msg = args
            .get("success_msg")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Ok(Plan {
            module: self.module_name().into(),
            host: host.name.clone(),
            diff: serde_json::json!({
                "that": that,
                "fail_msg": fail_msg,
                "success_msg": success_msg,
            }),
            will_change: false,
        })
    }

    fn apply(&self, plan: &Plan, host: &Host) -> Result<Outcome> {
        // Engine-side evaluation comes later. For now we always report Ok and
        // hand the diff back as returns; the engine will replace this with a
        // templated evaluation pass.
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
