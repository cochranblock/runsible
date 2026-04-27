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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Outcome, OutcomeStatus, Plan};

    fn sample_outcome() -> Outcome {
        Outcome {
            module: "runsible_builtin.debug".into(),
            host: "h1".into(),
            status: OutcomeStatus::Ok,
            elapsed_ms: 12,
            returns: serde_json::json!({"msg": "hi"}),
        }
    }

    fn sample_plan() -> Plan {
        Plan {
            module: "runsible_builtin.copy".into(),
            host: "h1".into(),
            diff: serde_json::json!({"path": "/x"}),
            will_change: true,
        }
    }

    #[test]
    fn run_start_to_ndjson_line_terminates_with_newline_and_kind() {
        let ev = Event::RunStart {
            playbook: "site.toml".into(),
            inventory: Some("hosts.toml".into()),
            host_count: 3,
            runsible_version: "0.0.1".into(),
        };
        let line = ev.to_ndjson_line().expect("ndjson");
        assert!(line.ends_with('\n'), "NDJSON line must end with \\n");
        let trimmed = line.trim_end_matches('\n');
        let v: serde_json::Value =
            serde_json::from_str(trimmed).expect("must be valid JSON without trailing newline");
        assert_eq!(v.get("kind").and_then(|k| k.as_str()), Some("run_start"));
        assert_eq!(v.get("playbook").and_then(|k| k.as_str()), Some("site.toml"));
        assert_eq!(v.get("host_count").and_then(|k| k.as_u64()), Some(3));
    }

    #[test]
    fn each_event_variant_emits_expected_kind() {
        // Table-driven test ensuring serde tag = "kind" with snake_case rename
        // covers every variant we ship.
        let cases: Vec<(Event, &'static str)> = vec![
            (
                Event::RunStart {
                    playbook: "p".into(),
                    inventory: None,
                    host_count: 0,
                    runsible_version: "0".into(),
                },
                "run_start",
            ),
            (
                Event::PlayStart {
                    play_index: 0,
                    name: "p".into(),
                    target_pattern: "all".into(),
                    host_count: 0,
                },
                "play_start",
            ),
            (
                Event::TaskStart {
                    play_index: 0,
                    task_index: 0,
                    name: "t".into(),
                    module: "m".into(),
                },
                "task_start",
            ),
            (
                Event::TaskOutcome {
                    play_index: 0,
                    task_index: 0,
                    outcome: sample_outcome(),
                },
                "task_outcome",
            ),
            (
                Event::PlanComputed {
                    play_index: 0,
                    task_index: 0,
                    plan: sample_plan(),
                },
                "plan_computed",
            ),
            (
                Event::HandlerFlush {
                    play_index: 0,
                    handler_id: "h".into(),
                },
                "handler_flush",
            ),
            (
                Event::PlayEnd {
                    play_index: 0,
                    ok: 0,
                    changed: 0,
                    failed: 0,
                    unreachable: 0,
                    skipped: 0,
                },
                "play_end",
            ),
            (
                Event::RunSummary {
                    ok: 0,
                    changed: 0,
                    failed: 0,
                    unreachable: 0,
                    skipped: 0,
                    elapsed_ms: 0,
                },
                "run_summary",
            ),
            (
                Event::Error {
                    host: Some("h1".into()),
                    message: "boom".into(),
                },
                "error",
            ),
        ];

        for (ev, expected_kind) in cases {
            let s = serde_json::to_string(&ev).expect("serialize");
            let v: serde_json::Value = serde_json::from_str(&s).expect("parse");
            assert_eq!(
                v.get("kind").and_then(|k| k.as_str()),
                Some(expected_kind),
                "variant {ev:?} must serialize with kind={expected_kind}"
            );
        }
    }

    #[test]
    fn event_json_round_trip_preserves_payload() {
        // Pick a couple of variants with non-trivial nested data.
        let outcome_ev = Event::TaskOutcome {
            play_index: 1,
            task_index: 4,
            outcome: sample_outcome(),
        };
        let s = serde_json::to_string(&outcome_ev).expect("serialize");
        let back: Event = serde_json::from_str(&s).expect("deserialize");
        // Re-serialize should be byte-identical (canonical map order from serde).
        let s2 = serde_json::to_string(&back).expect("re-serialize");
        assert_eq!(s, s2);

        let summary_ev = Event::RunSummary {
            ok: 3,
            changed: 2,
            failed: 1,
            unreachable: 0,
            skipped: 4,
            elapsed_ms: 9999,
        };
        let s = serde_json::to_string(&summary_ev).expect("serialize summary");
        let back: Event = serde_json::from_str(&s).expect("deserialize summary");
        let s2 = serde_json::to_string(&back).expect("re-serialize summary");
        assert_eq!(s, s2);
    }
}
