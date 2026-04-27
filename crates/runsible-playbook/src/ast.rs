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

    /// `[plays.handlers.<id>]` — each value is a raw TOML table containing a single module call.
    #[serde(default)]
    pub handlers: IndexMap<String, toml::Value>,

    /// `[[plays.roles]]` — array of role references.
    #[serde(default)]
    pub roles: Vec<RoleRef>,

    /// `[plays.vars]`
    #[serde(default)]
    pub vars: IndexMap<String, toml::Value>,

    /// `tags = [...]` applied to every task in the play.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Whether to auto-prepend a `setup` task before pre_tasks/role tasks/tasks/post_tasks.
    ///
    /// NOTE: runsible defaults this to `false` (per poor-decisions §12) — the
    /// opposite of Ansible's default `true`. If you actually want facts you must
    /// either set `gather_facts = true` on the play or call `setup` explicitly
    /// from a task.
    #[serde(default = "default_gather_facts")]
    pub gather_facts: bool,

    /// `vars_files = ["path1.toml", ...]` — flat TOML files merged at "play
    /// vars" precedence (between host vars and inline play.vars). Missing
    /// files are silently skipped at M1 (M2: emit a warning event).
    #[serde(default)]
    pub vars_files: Vec<String>,

    /// `[plays.module_defaults."<fq_module>"]` — per-module default args
    /// merged into every matching task call before templating. Task-level args
    /// always win on key collision.
    #[serde(default)]
    pub module_defaults: IndexMap<String, toml::Value>,
}

fn default_gather_facts() -> bool {
    false
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
    /// Optional `when = { expr = "..." }` Jinja boolean expression.
    pub when: Option<String>,
    /// Optional `notify = ["handler_id", ...]`.
    pub notify: Vec<String>,
    /// Optional `loop = [...]` — list of items; task runs once per item with
    /// the item bound to `loop_control.loop_var` (default `item`).
    pub loop_items: Option<Vec<toml::Value>>,
    /// `loop_control.loop_var` — defaults to "item".
    pub loop_var: String,
    /// `loop_control.label` — Jinja-rendered per-item label for events.
    pub loop_label: Option<String>,
    /// Optional `until = { expr = "..." }` — re-run the task until expr is true.
    pub until: Option<String>,
    /// `retries` — max attempts with `until`. Default 3.
    pub retries: u32,
    /// `delay_seconds` — sleep between retries. Default 5.
    pub delay_seconds: u64,
    /// `block = [[...]]` — child tasks. When non-empty, this task is a block
    /// (module_name set to the `_block_` sentinel; module dispatch is skipped).
    pub block: Vec<toml::Value>,
    /// `rescue = [[...]]` — runs only if any block child fails.
    pub rescue: Vec<toml::Value>,
    /// `always = [[...]]` — runs after block (and rescue if applicable).
    pub always: Vec<toml::Value>,
    /// `delegate_to = "<host>"` — when set, the engine substitutes this name
    /// into the ExecutionContext's host so the outcome reports the delegate.
    /// (At M1 the connection used is still the engine's local one — true
    /// remote delegation lands in M2.)
    pub delegate_to: Option<String>,
    /// `run_once = true` — execute on the first matching host only and skip
    /// subsequent hosts in the per-host loop.
    pub run_once: bool,
}

/// Sentinel `module_name` value indicating the task is a block, not a module call.
pub const BLOCK_SENTINEL: &str = "_block_";

/// Sentinel `module_name` value indicating the task is an `include_tasks` /
/// `import_tasks` directive, not a module call. The task's `args` is a string
/// holding the include path.
pub const INCLUDE_SENTINEL: &str = "_include_tasks_";

/// One `[[plays.roles]]` entry.
#[derive(Debug, Clone, Deserialize)]
pub struct RoleRef {
    pub name: String,
    /// Defaults to "main".
    #[serde(default = "default_entry_point")]
    pub entry_point: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// `[plays.roles.vars]` — overrides role defaults.
    #[serde(default)]
    pub vars: IndexMap<String, toml::Value>,
}

fn default_entry_point() -> String {
    "main".to_string()
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
    "control",
    "id",
    "module_defaults",
    "debugger",
    "include_tasks",
    "import_tasks",
];
