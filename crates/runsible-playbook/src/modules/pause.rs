//! `runsible_builtin.pause` — sleep for a duration.
//!
//! Args:
//!   seconds = 5
//!   minutes = 1   (alternative to seconds)
//!   prompt  = "string"   (M1: logged but does not actually wait for input)

use runsible_core::traits::ExecutionContext;
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::Result;

pub struct PauseModule;

impl DynModule for PauseModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.pause"
    }

    fn check_mode_safe(&self) -> bool {
        true
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let secs = if let Some(s) = args.get("seconds").and_then(|v| as_u64(v)) {
            s
        } else if let Some(m) = args.get("minutes").and_then(|v| as_u64(v)) {
            m.saturating_mul(60)
        } else {
            0
        };
        let prompt = args.get("prompt").and_then(|v| v.as_str()).map(String::from);
        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({
                "seconds": secs,
                "prompt": prompt,
            }),
            will_change: false,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        let started = std::time::Instant::now();
        let secs = plan.diff.get("seconds").and_then(|v| v.as_u64()).unwrap_or(0);
        if secs > 0 {
            std::thread::sleep(std::time::Duration::from_secs(secs));
        }
        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Ok,
            elapsed_ms: started.elapsed().as_millis() as u64,
            returns: serde_json::json!({
                "paused_seconds": secs,
                "prompt": plan.diff.get("prompt"),
            }),
        })
    }
}

fn as_u64(v: &toml::Value) -> Option<u64> {
    if let Some(n) = v.as_integer() {
        if n >= 0 {
            return Some(n as u64);
        }
    }
    if let Some(f) = v.as_float() {
        if f >= 0.0 {
            return Some(f as u64);
        }
    }
    None
}
