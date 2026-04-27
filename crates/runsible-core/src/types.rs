//! Shared types: hosts, plans, events. The TOML AST and the typed Playbook
//! live here; per-area parsers live in the relevant binary crate.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub type HostName = String;
pub type GroupName = String;
pub type PackageName = String;
pub type ModuleName = String;
pub type HandlerId = String;
pub type TagName = String;
pub type Vars = BTreeMap<String, toml::Value>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Host {
    pub name: HostName,
    #[serde(default)]
    pub vars: Vars,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub name: GroupName,
    #[serde(default)]
    pub hosts: Vec<HostName>,
    #[serde(default)]
    pub children: Vec<GroupName>,
    #[serde(default)]
    pub vars: Vars,
}

/// A plan is the difference between desired and actual state for one module
/// invocation against one host. Empty plan = no-op = idempotence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub module: ModuleName,
    pub host: HostName,
    pub diff: serde_json::Value,
    pub will_change: bool,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        !self.will_change
    }
}

/// The outcome of one module invocation against one host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    pub module: ModuleName,
    pub host: HostName,
    pub status: OutcomeStatus,
    pub elapsed_ms: u64,
    pub returns: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeStatus {
    Ok,
    Changed,
    Skipped,
    Failed,
    Unreachable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_serde_roundtrip() {
        let mut vars: Vars = BTreeMap::new();
        vars.insert("region".to_string(), toml::Value::String("us-west".into()));
        let h = Host {
            name: "web-01".into(),
            vars,
        };
        let json = serde_json::to_string(&h).expect("serialize host");
        let back: Host = serde_json::from_str(&json).expect("deserialize host");
        assert_eq!(back, h);
        assert_eq!(back.name, "web-01");
        assert_eq!(
            back.vars.get("region").and_then(|v| v.as_str()),
            Some("us-west")
        );
    }

    #[test]
    fn plan_with_no_change_is_empty() {
        let p = Plan {
            module: "runsible_builtin.copy".into(),
            host: "h1".into(),
            diff: serde_json::json!({}),
            will_change: false,
        };
        assert!(p.is_empty(), "Plan with will_change=false must be empty");
    }

    #[test]
    fn plan_with_change_is_not_empty() {
        let p = Plan {
            module: "runsible_builtin.copy".into(),
            host: "h1".into(),
            diff: serde_json::json!({"path": "/etc/hi"}),
            will_change: true,
        };
        assert!(!p.is_empty(), "Plan with will_change=true must not be empty");
    }

    #[test]
    fn outcome_status_serializes_to_snake_case() {
        // Each variant must round-trip through its lowercase snake_case form.
        let cases: &[(OutcomeStatus, &str)] = &[
            (OutcomeStatus::Ok, "\"ok\""),
            (OutcomeStatus::Changed, "\"changed\""),
            (OutcomeStatus::Failed, "\"failed\""),
            (OutcomeStatus::Skipped, "\"skipped\""),
            (OutcomeStatus::Unreachable, "\"unreachable\""),
        ];
        for (status, expected_json) in cases {
            let s = serde_json::to_string(status).expect("serialize status");
            assert_eq!(&s, expected_json, "status {status:?} must serialize as {expected_json}");
            let back: OutcomeStatus =
                serde_json::from_str(&s).expect("deserialize status");
            assert_eq!(back, *status);
        }
    }

    #[test]
    fn group_default_fields_deserialize() {
        // Empty group with only name should produce empty hosts/children/vars.
        let json = r#"{"name":"webservers"}"#;
        let g: Group = serde_json::from_str(json).expect("deserialize");
        assert_eq!(g.name, "webservers");
        assert!(g.hosts.is_empty());
        assert!(g.children.is_empty());
        assert!(g.vars.is_empty());
    }
}
