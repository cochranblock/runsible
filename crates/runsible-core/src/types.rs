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
