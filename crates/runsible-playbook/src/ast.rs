//! Minimal M0 playbook AST.  Only the fields the engine consumes at M0 are
//! typed; everything else is ignored (no deny_unknown_fields here).

use indexmap::IndexMap;
use serde::Deserialize;

/// Top-level playbook file.
#[derive(Debug, Clone, Deserialize)]
pub struct Playbook {
    /// `schema = "runsible.playbook.v1"` — optional at M0.
    #[serde(default)]
    pub schema: String,

    /// `[imports]` — module alias → FQ module name.
    #[serde(default)]
    pub imports: IndexMap<String, String>,

    /// `[[plays]]` array.
    #[serde(default)]
    pub plays: Vec<RawPlay>,
}

/// A single `[[plays]]` element, raw (tasks kept as toml::Value for dynamic-key parsing).
#[derive(Debug, Clone, Deserialize)]
pub struct RawPlay {
    pub name: String,

    #[serde(default)]
    pub hosts: PlayHosts,

    /// `[[plays.tasks]]`
    #[serde(default)]
    pub tasks: Vec<toml::Value>,

    /// `[[plays.pre_tasks]]`
    #[serde(default)]
    pub pre_tasks: Vec<toml::Value>,

    /// `[[plays.post_tasks]]`
    #[serde(default)]
    pub post_tasks: Vec<toml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PlayHosts {
    Single(String),
    List(Vec<String>),
}

impl Default for PlayHosts {
    fn default() -> Self {
        PlayHosts::Single("all".into())
    }
}

impl PlayHosts {
    /// Flatten to a single pattern string for runsible-inventory.
    pub fn to_pattern(&self) -> String {
        match self {
            PlayHosts::Single(s) => s.clone(),
            PlayHosts::List(v) => v.join(":"),
        }
    }
}

/// A resolved task: dynamic module key extracted from the raw TOML table.
#[derive(Debug, Clone)]
pub struct Task {
    pub name: Option<String>,
    pub module_name: String,
    pub args: toml::Value,
    pub register: Option<String>,
    pub tags: Vec<String>,
}

// Task-level keys that are not a module call.
pub(crate) const TASK_META_KEYS: &[&str] = &[
    "name",
    "tags",
    "when",
    "register",
    "until",
    "retries",
    "delay_seconds",
    "failed_when",
    "changed_when",
    "notify",
    "loop",
    "loop_control",
    "delegate_to",
    "delegate_facts",
    "become",
    "no_log",
    "ignore_errors",
    "ignore_unreachable",
    "timeout_seconds",
    "vars",
    "environment",
    "async",
    "background",
    "block",
    "rescue",
    "always",
    "throttle",
    "run_once",
    "action",
    "set_fact",
    "set_fact!",
    "control",
    "id",
    "module_defaults",
    "debugger",
];
