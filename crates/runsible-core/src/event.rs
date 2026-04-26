//! NDJSON event stream — `runsible.event.v1`.
//! See docs/plans/MASTER.md §10 + docs/plans/runsible-playbook.md §10.

use serde::{Deserialize, Serialize};

use crate::types::{HostName, ModuleName, Outcome, Plan};

pub const SCHEMA_VERSION: &str = "runsible.event.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    RunStart {
        playbook: String,
        inventory: Option<String>,
        host_count: usize,
        runsible_version: String,
    },
    PlayStart {
        play_index: usize,
        name: String,
        target_pattern: String,
        host_count: usize,
    },
    TaskStart {
        play_index: usize,
        task_index: usize,
        name: String,
        module: ModuleName,
    },
    TaskOutcome {
        play_index: usize,
        task_index: usize,
        outcome: Outcome,
    },
    PlanComputed {
        play_index: usize,
        task_index: usize,
        plan: Plan,
    },
    HandlerFlush {
        play_index: usize,
        handler_id: String,
    },
    PlayEnd {
        play_index: usize,
        ok: usize,
        changed: usize,
        failed: usize,
        unreachable: usize,
        skipped: usize,
    },
    RunSummary {
        ok: usize,
        changed: usize,
        failed: usize,
        unreachable: usize,
        skipped: usize,
        elapsed_ms: u64,
    },
    Error {
        host: Option<HostName>,
        message: String,
    },
}

impl Event {
    pub fn to_ndjson_line(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self).map(|s| s + "\n")
    }
}
