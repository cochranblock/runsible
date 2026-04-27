//! M1 engine: variables, templating, when/register/tags/handlers.
//!
//! Execution per host:
//!   merge vars (host + play) → for each task:
//!     filter by tags → eval `when` (skip if false) → render args via templater
//!     → plan() → emit PlanComputed → apply() → register outcome → notify handlers
//!     → emit TaskOutcome
//!   end-of-play: flush notified handlers (fire each handler's task)

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use indexmap::{IndexMap, IndexSet};
use runsible_connection::LocalSync;
use runsible_core::{
    event::Event,
    traits::ExecutionContext,
    types::{Host, OutcomeStatus, Vars},
};

use crate::{
    ast::{RawPlay, Task},
    catalog::ModuleCatalog,
    errors::{PlaybookError, Result},
    output::{emit, OutputMode},
    parse::{parse_playbook, resolve_handler, resolve_task},
    templating::Templater,
};

/// CLI-supplied filters / extras.
#[derive(Debug, Clone)]
pub struct RunOptions {
    pub tags: Vec<String>,
    pub skip_tags: Vec<String>,
    pub extra_vars: Vars,
    /// Override the default role search paths (`packages/`, `roles/`, `~/.runsible/cache/`).
    /// Used by tests to avoid chdir races; CLI consumers can leave this None.
    pub role_search_paths: Option<Vec<std::path::PathBuf>>,
    /// Dry-run: skip apply() for mutating modules, but still run safe modules.
    pub check_mode: bool,
    /// When true, mutating modules emit before/after content in plan.diff
    /// (and that propagates into outcome.returns).
    pub diff_mode: bool,
    /// Maximum number of hosts to run in parallel within a play. `1` keeps the
    /// deterministic sequential code path used by all existing tests; `>=2`
    /// dispatches work onto a Tokio runtime bounded by a semaphore.
    pub forks: usize,
    /// Skip tasks until the named task is encountered (matched by `task.name`).
    /// All preceding tasks at the top level are reported as Skipped with
    /// `skipped_reason: "start_at_task"`. Per-host: each host independently
    /// scans for the start task. M1 limitation: only matches at the top-level
    /// task sequence — does not descend into block children.
    pub start_at_task: Option<String>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            tags: Vec::new(),
            skip_tags: Vec::new(),
            extra_vars: Vars::new(),
            role_search_paths: None,
            check_mode: false,
            diff_mode: false,
            forks: 1,
            start_at_task: None,
        }
    }
}

/// Parse and run a playbook file against the given inventory string.
pub fn run(playbook_src: &str, inventory_spec: &str, playbook_label: &str) -> Result<RunResult> {
    run_with(playbook_src, inventory_spec, playbook_label, RunOptions::default())
}

pub fn run_with(
    playbook_src: &str,
    inventory_spec: &str,
    playbook_label: &str,
    opts: RunOptions,
) -> Result<RunResult> {
    let start = Instant::now();
    let mode = OutputMode::detect();
    let catalog: Arc<ModuleCatalog> = Arc::new(ModuleCatalog::with_builtins());
    let templater: Arc<Templater> = Arc::new(Templater::new());
    let connection = LocalSync;

    let hosts = resolve_inventory(inventory_spec)?;

    emit(
        &mode,
        &Event::RunStart {
            playbook: playbook_label.to_string(),
            inventory: Some(inventory_spec.to_string()),
            host_count: hosts.len(),
            runsible_version: env!("CARGO_PKG_VERSION").to_string(),
        },
    );

    let pb = parse_playbook(playbook_src)?;

    let mut total = RunStats::default();

    for (play_idx, raw_play) in pb.plays.iter().enumerate() {
        let pattern_str = raw_play.hosts.to_pattern();

        let play_hosts: Vec<&Host> = hosts
            .iter()
            .filter(|h| pattern_matches(&pattern_str, &h.name))
            .collect();

        emit(
            &mode,
            &Event::PlayStart {
                play_index: play_idx,
                name: raw_play.name.clone(),
                target_pattern: pattern_str.clone(),
                host_count: play_hosts.len(),
            },
        );

        // Load roles (find on disk, parse manifests + tasks + handlers + vars).
        let role_search: Vec<std::path::PathBuf> = opts
            .role_search_paths
            .clone()
            .unwrap_or_else(crate::roles::default_search_paths);
        let mut role_defaults: Vars = Vars::new();
        let mut role_vars: Vars = Vars::new();
        let mut role_param_vars: Vars = Vars::new();
        let mut role_task_blocks: Vec<(Vec<String>, Vars, Vec<toml::Value>)> = Vec::new(); // (extra_tags, role_params, tasks)
        let mut all_handlers_raw: IndexMap<String, toml::Value> = raw_play.handlers.clone();

        for role_ref in &raw_play.roles {
            let loaded = crate::roles::load_role(
                &role_ref.name,
                &role_ref.entry_point,
                &role_search,
            )?;
            for (k, v) in &loaded.defaults {
                role_defaults.insert(k.clone(), v.clone());
            }
            for (k, v) in &loaded.vars {
                role_vars.insert(k.clone(), v.clone());
            }
            for (k, v) in &role_ref.vars {
                role_param_vars.insert(k.clone(), v.clone());
            }
            for (id, body) in &loaded.handlers {
                all_handlers_raw.insert(id.clone(), body.clone());
            }
            let role_params: Vars = role_ref.vars.clone().into_iter().collect();
            role_task_blocks.push((role_ref.tags.clone(), role_params, loaded.tasks.clone()));
        }

        // Resolve all tasks (pre_tasks, role tasks, tasks, post_tasks) — parse-time validation.
        let mut tasks: Vec<Task> = Vec::new();

        // Auto-gather facts: when `gather_facts = true` on the play, prepend a
        // synthetic `setup` task so it runs BEFORE pre_tasks → role tasks →
        // tasks → post_tasks. Default is false (poor-decisions §12).
        if raw_play.gather_facts {
            let raw_setup: toml::Value = toml::Value::Table({
                let mut t = toml::map::Map::new();
                t.insert(
                    "name".into(),
                    toml::Value::String("Gathering Facts".into()),
                );
                t.insert(
                    "runsible_builtin.setup".into(),
                    toml::Value::Table(toml::map::Map::new()),
                );
                t
            });
            tasks.push(resolve_task(&raw_setup, &pb.imports)?);
        }

        for raw in &raw_play.pre_tasks {
            tasks.push(resolve_task(raw, &pb.imports)?);
        }
        // Role tasks: each gets the role's tags appended (so --tags applies).
        for (role_tags, _params, role_tasks) in &role_task_blocks {
            for raw in role_tasks {
                let mut t = resolve_task(raw, &pb.imports)?;
                for rt in role_tags {
                    if !t.tags.contains(rt) {
                        t.tags.push(rt.clone());
                    }
                }
                tasks.push(t);
            }
        }
        for raw in &raw_play.tasks {
            tasks.push(resolve_task(raw, &pb.imports)?);
        }
        for raw in &raw_play.post_tasks {
            tasks.push(resolve_task(raw, &pb.imports)?);
        }

        // Resolve all handlers once (play handlers + role handlers).
        let handlers: IndexMap<String, Task> = all_handlers_raw
            .iter()
            .map(|(id, raw)| resolve_handler(id, raw, &pb.imports).map(|t| (id.clone(), t)))
            .collect::<Result<_>>()?;

        // Validate: every notify ID must be a real handler.
        for t in &tasks {
            for h_id in &t.notify {
                if !handlers.contains_key(h_id) {
                    return Err(PlaybookError::TypeCheck(format!(
                        "task {:?}: notify references unknown handler '{}'",
                        t.name, h_id
                    )));
                }
            }
        }

        // Pre-compute play-wide context that doesn't depend on which host runs:
        // the inventory snapshot, play_hosts list, the immutable ansible_version
        // table, and the cross-host hostvars snapshot. Each host then only does
        // a per-host vars merge on top.
        let play_ctx = Arc::new(PlayContext {
            inv_dir: std::env::current_dir()
                .ok()
                .and_then(|p| p.to_str().map(String::from))
                .unwrap_or_default(),
            inventory_host_names: hosts
                .iter()
                .map(|h| toml::Value::String(h.name.clone()))
                .collect(),
            play_host_names: play_hosts
                .iter()
                .map(|h| toml::Value::String(h.name.clone()))
                .collect(),
            hostvars_table: build_hostvars_snapshot(&play_hosts),
            ansible_version_table: build_ansible_version_table(),
        });

        // Snapshot the resolved tasks + handlers for parallel/sequential consumers.
        let tasks_arc = Arc::new(tasks);
        let raw_play_arc = Arc::new(raw_play.clone());
        let imports_arc = Arc::new(pb.imports.clone());
        let role_defaults_arc = Arc::new(role_defaults);
        let role_vars_arc = Arc::new(role_vars);
        let role_param_vars_arc = Arc::new(role_param_vars);
        let opts_arc = Arc::new(opts.clone());

        // Per-play run_once cache: shared across all hosts (sequential or
        // forked) so the first host's outcome is reused on the rest.
        let run_once_cache: RunOnceCache = Arc::new(Mutex::new(HashMap::new()));

        let forks = opts_arc.forks.max(1);
        let host_results: Vec<HostResult> = if forks == 1 || play_hosts.len() <= 1 {
            // Sequential path: deterministic event ordering, used by all
            // existing tests.
            play_hosts
                .iter()
                .map(|h| {
                    run_host_tasks(
                        h,
                        play_idx,
                        raw_play_arc.clone(),
                        tasks_arc.clone(),
                        imports_arc.clone(),
                        catalog.clone(),
                        templater.clone(),
                        play_ctx.clone(),
                        role_defaults_arc.clone(),
                        role_vars_arc.clone(),
                        role_param_vars_arc.clone(),
                        opts_arc.clone(),
                        mode,
                        run_once_cache.clone(),
                    )
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            run_hosts_parallel(
                &play_hosts,
                play_idx,
                forks,
                raw_play_arc.clone(),
                tasks_arc.clone(),
                imports_arc.clone(),
                catalog.clone(),
                templater.clone(),
                play_ctx.clone(),
                role_defaults_arc.clone(),
                role_vars_arc.clone(),
                role_param_vars_arc.clone(),
                opts_arc.clone(),
                mode,
                run_once_cache.clone(),
            )?
        };

        // Merge per-host results into play-wide aggregates.
        let mut play_stats = RunStats::default();
        let mut play_changed_handlers: IndexMap<String, IndexSet<String>> = IndexMap::new();
        for hr in host_results {
            play_stats.ok += hr.stats.ok;
            play_stats.changed += hr.stats.changed;
            play_stats.failed += hr.stats.failed;
            play_stats.skipped += hr.stats.skipped;
            for h_id in hr.notified_handlers {
                play_changed_handlers
                    .entry(h_id)
                    .or_default()
                    .insert(hr.host_name.clone());
            }
        }

        // Flush handlers — one execution per (handler, host) pair.
        for (h_id, host_set) in &play_changed_handlers {
            let handler_task = handlers.get(h_id).expect("validated above");
            emit(
                &mode,
                &Event::HandlerFlush {
                    play_index: play_idx,
                    handler_id: h_id.clone(),
                },
            );
            let module = catalog
                .get(&handler_task.module_name)
                .ok_or_else(|| PlaybookError::ModuleNotFound(handler_task.module_name.clone()))?;
            for host in &play_hosts {
                if !host_set.contains(&host.name) {
                    continue;
                }
                let rendered = templater
                    .render_value(&handler_task.args, &host.vars)
                    .unwrap_or_else(|_| handler_task.args.clone());
                let ctx = ExecutionContext {
                    host,
                    vars: &host.vars,
                    connection: &connection,
                    check_mode: opts.check_mode,
                    diff_mode: opts.diff_mode,
                };
                let plan = module.plan(&rendered, &ctx).map_err(|e| PlaybookError::ExecFailed {
                    host: host.name.clone(),
                    message: e.to_string(),
                })?;
                let outcome = if opts.check_mode && plan.will_change && !module.check_mode_safe() {
                    synthesize_check_mode_outcome(&plan, &host.name)
                } else {
                    module.apply(&plan, &ctx).map_err(|e| PlaybookError::ExecFailed {
                        host: host.name.clone(),
                        message: e.to_string(),
                    })?
                };
                match outcome.status {
                    OutcomeStatus::Ok => play_stats.ok += 1,
                    OutcomeStatus::Changed => play_stats.changed += 1,
                    OutcomeStatus::Skipped => play_stats.skipped += 1,
                    OutcomeStatus::Failed | OutcomeStatus::Unreachable => play_stats.failed += 1,
                }
                emit(
                    &mode,
                    &Event::TaskOutcome {
                        play_index: play_idx,
                        task_index: usize::MAX,
                        outcome,
                    },
                );
            }
        }

        emit(
            &mode,
            &Event::PlayEnd {
                play_index: play_idx,
                ok: play_stats.ok,
                changed: play_stats.changed,
                failed: play_stats.failed,
                unreachable: 0,
                skipped: play_stats.skipped,
            },
        );

        total.ok += play_stats.ok;
        total.changed += play_stats.changed;
        total.failed += play_stats.failed;
        total.skipped += play_stats.skipped;
    }

    let elapsed_ms = start.elapsed().as_millis() as u64;

    emit(
        &mode,
        &Event::RunSummary {
            ok: total.ok,
            changed: total.changed,
            failed: total.failed,
            unreachable: 0,
            skipped: total.skipped,
            elapsed_ms,
        },
    );

    Ok(RunResult {
        ok: total.ok,
        changed: total.changed,
        failed: total.failed,
        skipped: total.skipped,
        elapsed_ms,
    })
}

/// Execute a single task (module call OR block) on a single host, accumulating
/// stats and notifications. Returns the worst outcome status across all
/// iterations (used by block to decide whether to enter rescue).
#[allow(clippy::too_many_arguments)]
fn execute_one_task(
    task: &Task,
    play_idx: usize,
    task_idx: usize,
    host: &Host,
    vars: &mut Vars,
    play_tags: &[String],
    opts: &RunOptions,
    catalog: &ModuleCatalog,
    templater: &Templater,
    connection: &LocalSync,
    mode: &OutputMode,
    play_stats: &mut RunStats,
    notified_for_host: &mut IndexSet<String>,
    imports: &IndexMap<String, String>,
    module_defaults: &IndexMap<String, toml::Value>,
    run_once_cache: &RunOnceCache,
) -> Result<OutcomeStatus> {
    // run_once short-circuit: if a previous host already ran this exact task
    // index with run_once=true, replay the registered outcome (so `register`
    // works on every host) and emit a Skipped outcome with a clear reason.
    if task.run_once {
        let cached = {
            let guard = run_once_cache.lock().expect("run_once cache poisoned");
            guard.get(&task_idx).cloned()
        };
        if let Some(entry) = cached {
            if let Some(reg_key) = &task.register {
                let outcome_json = serde_json::to_value(&entry.outcome).unwrap_or_default();
                if let Ok(tv) = json_to_toml_value(outcome_json) {
                    vars.insert(reg_key.clone(), tv);
                }
            }
            let outcome = runsible_core::types::Outcome {
                module: task.module_name.clone(),
                host: host.name.clone(),
                status: OutcomeStatus::Skipped,
                elapsed_ms: 0,
                returns: serde_json::json!({
                    "skipped_reason": "run_once_already_executed",
                    "first_host": entry.outcome.host,
                }),
            };
            play_stats.skipped += 1;
            emit(
                mode,
                &Event::TaskOutcome {
                    play_index: play_idx,
                    task_index: task_idx,
                    outcome,
                },
            );
            return Ok(OutcomeStatus::Skipped);
        }
    }

    // Tag filtering.
    let effective_tags: Vec<&String> = task.tags.iter().chain(play_tags.iter()).collect();
    if !tag_filter_passes(&effective_tags, &opts.tags, &opts.skip_tags) {
        return Ok(OutcomeStatus::Skipped);
    }

    emit(
        mode,
        &Event::TaskStart {
            play_index: play_idx,
            task_index: task_idx,
            name: task.name.clone().unwrap_or_else(|| task.module_name.clone()),
            module: task.module_name.clone(),
        },
    );

    // when evaluation.
    if let Some(expr) = &task.when {
        match templater.eval_bool(expr, vars) {
            Ok(false) => {
                let outcome = runsible_core::types::Outcome {
                    module: task.module_name.clone(),
                    host: host.name.clone(),
                    status: OutcomeStatus::Skipped,
                    elapsed_ms: 0,
                    returns: serde_json::json!({"skipped_reason": "when=false"}),
                };
                play_stats.skipped += 1;
                emit(
                    mode,
                    &Event::TaskOutcome {
                        play_index: play_idx,
                        task_index: task_idx,
                        outcome,
                    },
                );
                return Ok(OutcomeStatus::Skipped);
            }
            Ok(true) => {}
            Err(e) => {
                return Err(PlaybookError::ExecFailed {
                    host: host.name.clone(),
                    message: format!("when expression error: {e}"),
                });
            }
        }
    }

    // Block dispatch.
    if task.module_name == crate::ast::BLOCK_SENTINEL {
        let mut block_failed = false;
        let mut worst = OutcomeStatus::Ok;

        for (child_idx, raw) in task.block.iter().enumerate() {
            let child = crate::parse::resolve_task(raw, imports)?;
            let child_status = execute_one_task(
                &child,
                play_idx,
                task_idx * 10_000 + child_idx + 1,
                host,
                vars,
                play_tags,
                opts,
                catalog,
                templater,
                connection,
                mode,
                play_stats,
                notified_for_host,
                imports,
                module_defaults,
                run_once_cache,
            )?;
            if matches!(child_status, OutcomeStatus::Failed | OutcomeStatus::Unreachable) {
                block_failed = true;
                worst = OutcomeStatus::Failed;
                break;
            }
        }

        if block_failed {
            for (child_idx, raw) in task.rescue.iter().enumerate() {
                let child = crate::parse::resolve_task(raw, imports)?;
                let st = execute_one_task(
                    &child,
                    play_idx,
                    task_idx * 10_000 + 5_000 + child_idx,
                    host,
                    vars,
                    play_tags,
                    opts,
                    catalog,
                    templater,
                    connection,
                    mode,
                    play_stats,
                    notified_for_host,
                    imports,
                    module_defaults,
                    run_once_cache,
                )?;
                // If rescue runs cleanly, the block recovers — downgrade worst.
                if !matches!(st, OutcomeStatus::Failed | OutcomeStatus::Unreachable) {
                    worst = OutcomeStatus::Changed;
                }
            }
        }

        for (child_idx, raw) in task.always.iter().enumerate() {
            let child = crate::parse::resolve_task(raw, imports)?;
            let _ = execute_one_task(
                &child,
                play_idx,
                task_idx * 10_000 + 9_000 + child_idx,
                host,
                vars,
                play_tags,
                opts,
                catalog,
                templater,
                connection,
                mode,
                play_stats,
                notified_for_host,
                imports,
                module_defaults,
                run_once_cache,
            )?;
        }

        return Ok(worst);
    }

    // include_tasks / import_tasks dispatch. The path was rendered through
    // templater-untouched parse.rs into task.args as a String; at exec time we
    // template it (so `include_tasks = "{{ os }}.toml"` works), then read the
    // file as TOML. The file is either a top-level array of tasks or a table
    // with a `tasks = [...]` array.
    if task.module_name == crate::ast::INCLUDE_SENTINEL {
        let raw_path = task.args.as_str().unwrap_or("").to_string();
        let path = templater
            .render_str(&raw_path, vars)
            .unwrap_or(raw_path.clone());
        let body = std::fs::read_to_string(&path).map_err(|e| PlaybookError::ExecFailed {
            host: host.name.clone(),
            message: format!("include_tasks: {path}: {e}"),
        })?;
        let value: toml::Value = toml::from_str(&body)
            .map_err(|e| PlaybookError::Parse(format!("{path}: {e}")))?;

        let task_array: Vec<toml::Value> = match &value {
            toml::Value::Array(arr) => arr.clone(),
            toml::Value::Table(t) => t
                .get("tasks")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default(),
            _ => {
                return Err(PlaybookError::Parse(format!(
                    "{path}: include file must be an array of tasks or a table with `tasks = [...]`",
                )))
            }
        };

        let mut worst = OutcomeStatus::Ok;
        for (i, raw) in task_array.iter().enumerate() {
            let child = crate::parse::resolve_task(raw, imports)?;
            let st = execute_one_task(
                &child,
                play_idx,
                task_idx * 100_000 + i + 1,
                host,
                vars,
                play_tags,
                opts,
                catalog,
                templater,
                connection,
                mode,
                play_stats,
                notified_for_host,
                imports,
                module_defaults,
                run_once_cache,
            )?;
            match st {
                OutcomeStatus::Failed | OutcomeStatus::Unreachable => {
                    worst = OutcomeStatus::Failed;
                }
                OutcomeStatus::Changed if matches!(worst, OutcomeStatus::Ok) => {
                    worst = OutcomeStatus::Changed;
                }
                _ => {}
            }
        }
        return Ok(worst);
    }

    // Module dispatch path.
    let iterations: Vec<Option<toml::Value>> = match &task.loop_items {
        Some(items) => items.iter().cloned().map(Some).collect(),
        None => vec![None],
    };

    let module = catalog
        .get(&task.module_name)
        .ok_or_else(|| PlaybookError::ModuleNotFound(task.module_name.clone()))?;

    // Apply module_defaults: starting with the per-module defaults table (if
    // any), overlay the task-specified args. Task args win on key collision.
    // If either side isn't a table, fall back to task.args unchanged.
    let merged_args: toml::Value = match (
        module_defaults.get(&task.module_name),
        &task.args,
    ) {
        (Some(toml::Value::Table(defaults)), toml::Value::Table(task_args)) => {
            let mut merged = defaults.clone();
            for (k, v) in task_args {
                merged.insert(k.clone(), v.clone());
            }
            toml::Value::Table(merged)
        }
        _ => task.args.clone(),
    };

    let mut worst = OutcomeStatus::Ok;

    for iter_item in iterations {
        if let Some(item) = &iter_item {
            vars.insert(task.loop_var.clone(), item.clone());
        }

        let max_attempts = if task.until.is_some() { task.retries.max(1) } else { 1 };
        let mut last_outcome: Option<runsible_core::types::Outcome> = None;

        for attempt in 1..=max_attempts {
            let rendered_args = templater.render_value(&merged_args, vars).map_err(|e| {
                PlaybookError::ExecFailed {
                    host: host.name.clone(),
                    message: format!("template error: {e}"),
                }
            })?;

            // delegate_to: substitute the host shown in the ExecutionContext
            // (and reported in outcomes) with the named delegate. The
            // hostname is templated so `delegate_to = "{{ db_host }}"` works.
            // M1 limitation: the underlying connection is still LocalSync;
            // true cross-SSH delegation lands in M2.
            let delegate_host: Option<Host> = task.delegate_to.as_ref().map(|raw| {
                let rendered = templater
                    .render_str(raw, vars)
                    .unwrap_or_else(|_| raw.clone());
                Host {
                    name: rendered,
                    vars: runsible_core::types::Vars::new(),
                }
            });
            let exec_host: &Host = delegate_host.as_ref().unwrap_or(host);

            let ctx = ExecutionContext {
                host: exec_host,
                vars,
                connection,
                check_mode: opts.check_mode,
                diff_mode: opts.diff_mode,
            };

            let plan = module.plan(&rendered_args, &ctx).map_err(|e| {
                PlaybookError::ExecFailed {
                    host: exec_host.name.clone(),
                    message: e.to_string(),
                }
            })?;

            emit(
                mode,
                &Event::PlanComputed {
                    play_index: play_idx,
                    task_index: task_idx,
                    plan: plan.clone(),
                },
            );

            let mut outcome = if opts.check_mode && plan.will_change && !module.check_mode_safe() {
                synthesize_check_mode_outcome(&plan, &exec_host.name)
            } else {
                module.apply(&plan, &ctx).map_err(|e| {
                    PlaybookError::ExecFailed {
                        host: exec_host.name.clone(),
                        message: e.to_string(),
                    }
                })?
            };

            if task.module_name == "runsible_builtin.set_fact" {
                if let Some(obj) = plan.diff.as_object() {
                    for (k, v) in obj {
                        if let Ok(tv) = json_to_toml_value(v.clone()) {
                            vars.insert(k.clone(), tv);
                        }
                    }
                }
            }

            // Setup module: merge `ansible_facts` from outcome.returns into per-host
            // vars so subsequent tasks can template against {{ ansible_hostname }} etc.
            // Each fact is exposed both as a top-level var AND nested under
            // `ansible_facts` (so both `{{ ansible_distribution }}` and
            // `{{ ansible_facts.ansible_distribution }}` work).
            if task.module_name == "runsible_builtin.setup" {
                if let Some(facts) = outcome
                    .returns
                    .get("ansible_facts")
                    .and_then(|v| v.as_object())
                {
                    for (k, v) in facts {
                        if let Ok(tv) = json_to_toml_value(v.clone()) {
                            vars.insert(k.clone(), tv);
                        }
                    }
                    if let Ok(tv) =
                        json_to_toml_value(serde_json::Value::Object(facts.clone()))
                    {
                        vars.insert("ansible_facts".into(), tv);
                    }
                }
            }

            if task.module_name == "runsible_builtin.assert" {
                let that = plan
                    .diff
                    .get("that")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mut failed_expr: Option<String> = None;
                for expr in &that {
                    if let Some(s) = expr.as_str() {
                        match templater.eval_bool(s, vars) {
                            Ok(true) => {}
                            Ok(false) => {
                                failed_expr = Some(s.to_string());
                                break;
                            }
                            Err(e) => {
                                failed_expr = Some(format!("{s} ({e})"));
                                break;
                            }
                        }
                    }
                }
                if let Some(e) = failed_expr {
                    outcome.status = OutcomeStatus::Failed;
                    outcome.returns = serde_json::json!({
                        "msg": "assertion failed",
                        "evaluated_to": false,
                        "assertion": e,
                    });
                }
            }

            if let Some(reg_key) = &task.register {
                let outcome_json = serde_json::to_value(&outcome).unwrap_or_default();
                if let Ok(tv) = json_to_toml_value(outcome_json) {
                    vars.insert(reg_key.clone(), tv);
                }
            }

            last_outcome = Some(outcome);

            if let Some(expr) = &task.until {
                match templater.eval_bool(expr, vars) {
                    Ok(true) => break,
                    Ok(false) | Err(_) => {
                        if attempt < max_attempts {
                            std::thread::sleep(std::time::Duration::from_secs(
                                task.delay_seconds,
                            ));
                            continue;
                        }
                    }
                }
            }
            break;
        }

        let outcome = last_outcome.expect("attempt loop must run once");

        match outcome.status {
            OutcomeStatus::Ok => play_stats.ok += 1,
            OutcomeStatus::Changed => {
                play_stats.changed += 1;
                if matches!(worst, OutcomeStatus::Ok) {
                    worst = OutcomeStatus::Changed;
                }
            }
            OutcomeStatus::Skipped => play_stats.skipped += 1,
            OutcomeStatus::Failed | OutcomeStatus::Unreachable => {
                play_stats.failed += 1;
                worst = OutcomeStatus::Failed;
            }
        }

        if matches!(outcome.status, OutcomeStatus::Changed) {
            for h_id in &task.notify {
                notified_for_host.insert(h_id.clone());
            }
        }

        // run_once cache write: first host to actually execute this task
        // stashes the outcome so subsequent hosts replay it as Skipped. We
        // only write when the cache is currently empty for this task_idx —
        // a read at the top of the function would have short-circuited an
        // already-cached host before we got here.
        if task.run_once {
            if let Ok(mut guard) = run_once_cache.lock() {
                guard
                    .entry(task_idx)
                    .or_insert_with(|| RunOnceEntry {
                        outcome: outcome.clone(),
                    });
            }
        }

        emit(
            mode,
            &Event::TaskOutcome {
                play_index: play_idx,
                task_index: task_idx,
                outcome,
            },
        );
    }

    Ok(worst)
}

/// Tag filter logic:
/// - tag `always` always runs (unless explicitly skipped)
/// - tag `never` only runs if explicitly named in --tags
/// - if --tags is empty, all non-`never` tasks run
/// - if --tags is non-empty, only tasks with at least one matching tag run
/// - --skip-tags subtracts unconditionally
fn tag_filter_passes(
    task_tags: &[&String],
    cli_tags: &[String],
    cli_skip_tags: &[String],
) -> bool {
    let has_tag = |name: &str| task_tags.iter().any(|t| t.as_str() == name);

    if has_tag("always") && !cli_skip_tags.iter().any(|t| t == "always") {
        return !cli_skip_tags
            .iter()
            .any(|t| task_tags.iter().any(|tt| tt.as_str() == t.as_str()));
    }

    if cli_tags.is_empty() {
        if has_tag("never") {
            return false;
        }
    } else {
        let any_match = cli_tags
            .iter()
            .any(|wanted| task_tags.iter().any(|tt| tt.as_str() == wanted.as_str()));
        if !any_match {
            return false;
        }
    }

    if cli_skip_tags
        .iter()
        .any(|skip| task_tags.iter().any(|tt| tt.as_str() == skip.as_str()))
    {
        return false;
    }

    true
}

/// Build a synthetic Outcome for a mutating module that was not run in
/// check_mode. The module's plan is consulted for `will_change` and `diff`.
fn synthesize_check_mode_outcome(
    plan: &runsible_core::types::Plan,
    host_name: &str,
) -> runsible_core::types::Outcome {
    runsible_core::types::Outcome {
        module: plan.module.clone(),
        host: host_name.to_string(),
        status: if plan.will_change {
            OutcomeStatus::Changed
        } else {
            OutcomeStatus::Ok
        },
        elapsed_ms: 0,
        returns: serde_json::json!({
            "check_mode": true,
            "would_change": plan.will_change,
            "diff": plan.diff,
        }),
    }
}

/// Build a per-process OMIT sentinel. Ansible uses a similar trick: a
/// placeholder string that modules can compare optional args against to detect
/// "skip this arg". The trailing nonce makes accidental literal matches in
/// playbook content astronomically unlikely.
fn omit_placeholder() -> String {
    use std::sync::OnceLock;
    static OMIT: OnceLock<String> = OnceLock::new();
    OMIT.get_or_init(|| {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("__OMIT_PLACEHOLDER_{nonce:x}__")
    })
    .clone()
}

fn json_to_toml_value(v: serde_json::Value) -> std::result::Result<toml::Value, ()> {
    Ok(match v {
        serde_json::Value::Null => toml::Value::String(String::new()),
        serde_json::Value::Bool(b) => toml::Value::Boolean(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                toml::Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                toml::Value::Float(f)
            } else {
                return Err(());
            }
        }
        serde_json::Value::String(s) => toml::Value::String(s),
        serde_json::Value::Array(a) => toml::Value::Array(
            a.into_iter().filter_map(|x| json_to_toml_value(x).ok()).collect(),
        ),
        serde_json::Value::Object(o) => {
            let mut t = toml::map::Map::new();
            for (k, v) in o {
                if let Ok(tv) = json_to_toml_value(v) {
                    t.insert(k, tv);
                }
            }
            toml::Value::Table(t)
        }
    })
}

/// Parse an inventory spec into a flat host list.
pub fn resolve_inventory(spec: &str) -> Result<Vec<Host>> {
    if spec.contains(',') {
        let hosts: Vec<Host> = spec
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|name| Host {
                name: name.to_string(),
                vars: Vars::new(),
            })
            .collect();
        if hosts.is_empty() {
            return Err(PlaybookError::Inventory("empty host list".into()));
        }
        return Ok(hosts);
    }

    if std::path::Path::new(spec).exists() {
        let src = std::fs::read_to_string(spec)
            .map_err(|e| PlaybookError::Inventory(e.to_string()))?;
        let inv = runsible_inventory::parse_inventory(&src)
            .map_err(|e| PlaybookError::Inventory(e.to_string()))?;
        let hosts: Vec<Host> = inv
            .hosts
            .into_iter()
            .map(|(name, entry)| Host {
                name,
                vars: entry.vars,
            })
            .collect();
        return Ok(hosts);
    }

    Ok(vec![Host {
        name: spec.to_string(),
        vars: Vars::new(),
    }])
}

pub fn pattern_matches(pattern: &str, host_name: &str) -> bool {
    if pattern == "all" || pattern == "*" {
        return true;
    }
    if pattern == host_name {
        return true;
    }
    if pattern.contains(':') {
        return pattern
            .split(':')
            .any(|seg| pattern_matches(seg.trim(), host_name));
    }
    false
}

#[derive(Debug, Default, Clone)]
struct RunStats {
    ok: usize,
    changed: usize,
    failed: usize,
    skipped: usize,
}

/// Per-host execution result returned by [`run_host_tasks`].
struct HostResult {
    host_name: String,
    stats: RunStats,
    notified_handlers: IndexSet<String>,
}

/// Per-`run_once` task cache: when the first host runs a task with
/// `run_once = true`, its registered outcome (if any) is stashed here, keyed
/// by the task's index in the play's resolved task list. Subsequent hosts
/// short-circuit by reading the cache, copying the registered outcome into
/// their own vars under `task.register` (if set), and emitting a `Skipped`
/// outcome with reason `run_once_already_executed`.
///
/// Wrapped in `Arc<Mutex<...>>` so the parallel-fork execution path can share
/// the same cache safely. Sequential mode contends on the lock once per
/// host/task and pays no observable cost.
type RunOnceCache = Arc<Mutex<HashMap<usize, RunOnceEntry>>>;

#[derive(Clone)]
struct RunOnceEntry {
    /// Outcome from the first host's execution, used as the registered value
    /// on subsequent hosts so `register` semantics carry over.
    outcome: runsible_core::types::Outcome,
}

/// Play-wide context computed once and shared across hosts.
///
/// Built outside the per-host loop so neither sequential nor parallel paths
/// recompute identical inventory/version/hostvars data per host.
struct PlayContext {
    inv_dir: String,
    inventory_host_names: Vec<toml::Value>,
    play_host_names: Vec<toml::Value>,
    hostvars_table: toml::Value,
    ansible_version_table: toml::Value,
}

fn build_hostvars_snapshot(play_hosts: &[&Host]) -> toml::Value {
    let hostvars: serde_json::Map<String, serde_json::Value> = play_hosts
        .iter()
        .map(|h| {
            let host_vars_json = serde_json::to_value(&h.vars).unwrap_or_default();
            (h.name.clone(), host_vars_json)
        })
        .collect();
    json_to_toml_value(serde_json::Value::Object(hostvars))
        .unwrap_or_else(|_| toml::Value::Table(toml::map::Map::new()))
}

fn build_ansible_version_table() -> toml::Value {
    let v = env!("CARGO_PKG_VERSION");
    let mut parts = v.split('.');
    let major: i64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor: i64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let revision: i64 = parts
        .next()
        .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut t = toml::map::Map::new();
    t.insert("full".into(), toml::Value::String(v.to_string()));
    t.insert("major".into(), toml::Value::Integer(major));
    t.insert("minor".into(), toml::Value::Integer(minor));
    t.insert("revision".into(), toml::Value::Integer(revision));
    toml::Value::Table(t)
}

/// Parallel host dispatch for a single play. Spawns one tokio task per host,
/// each acquiring a semaphore permit (bound = `forks`), then handing the
/// per-host sync work to `tokio::task::spawn_blocking`. Returns once every
/// host has finished. Per-host failures bubble up as the play's error.
#[allow(clippy::too_many_arguments)]
fn run_hosts_parallel(
    play_hosts: &[&Host],
    play_idx: usize,
    forks: usize,
    raw_play: Arc<RawPlay>,
    tasks: Arc<Vec<Task>>,
    imports: Arc<IndexMap<String, String>>,
    catalog: Arc<ModuleCatalog>,
    templater: Arc<Templater>,
    play_ctx: Arc<PlayContext>,
    role_defaults: Arc<Vars>,
    role_vars: Arc<Vars>,
    role_param_vars: Arc<Vars>,
    opts: Arc<RunOptions>,
    mode: OutputMode,
    run_once_cache: RunOnceCache,
) -> Result<Vec<HostResult>> {
    let hosts_owned: Vec<Host> = play_hosts.iter().map(|h| (*h).clone()).collect();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(forks.min(64))
        .enable_all()
        .build()
        .map_err(|e| PlaybookError::ExecFailed {
            host: "controller".to_string(),
            message: format!("tokio runtime: {e}"),
        })?;

    runtime.block_on(async move {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(forks));
        let mut set: tokio::task::JoinSet<Result<HostResult>> = tokio::task::JoinSet::new();

        for host in hosts_owned {
            let sem = semaphore.clone();
            let raw_play = raw_play.clone();
            let tasks = tasks.clone();
            let imports = imports.clone();
            let catalog = catalog.clone();
            let templater = templater.clone();
            let play_ctx = play_ctx.clone();
            let role_defaults = role_defaults.clone();
            let role_vars = role_vars.clone();
            let role_param_vars = role_param_vars.clone();
            let opts = opts.clone();
            let run_once_cache = run_once_cache.clone();
            set.spawn(async move {
                let _permit = sem.acquire_owned().await.expect("semaphore closed");
                tokio::task::spawn_blocking(move || {
                    run_host_tasks(
                        &host,
                        play_idx,
                        raw_play,
                        tasks,
                        imports,
                        catalog,
                        templater,
                        play_ctx,
                        role_defaults,
                        role_vars,
                        role_param_vars,
                        opts,
                        mode,
                        run_once_cache,
                    )
                })
                .await
                .map_err(|e| PlaybookError::ExecFailed {
                    host: "controller".to_string(),
                    message: format!("join: {e}"),
                })?
            });
        }

        let mut out: Vec<HostResult> = Vec::new();
        while let Some(joined) = set.join_next().await {
            let res = joined.map_err(|e| PlaybookError::ExecFailed {
                host: "controller".to_string(),
                message: format!("join_next: {e}"),
            })??;
            out.push(res);
        }
        Ok(out)
    })
}

/// Build the per-host vars map (role defaults < host < play < role vars <
/// role params < extra_vars) and overlay all magic vars.
#[allow(clippy::too_many_arguments)]
fn build_host_vars(
    host: &Host,
    raw_play: &RawPlay,
    role_defaults: &Vars,
    role_vars: &Vars,
    role_param_vars: &Vars,
    opts: &RunOptions,
    play_ctx: &PlayContext,
) -> Vars {
    let mut vars: Vars = role_defaults.clone();
    for (k, v) in &host.vars {
        vars.insert(k.clone(), v.clone());
    }
    // vars_files: each file is a flat TOML table merged at "play vars"
    // precedence (between host vars and inline play.vars). Missing files and
    // unparseable bodies are silently skipped at M1 — M2 will emit a warning
    // event so users notice typos. Inline `play.vars` win on key collision.
    for vf in &raw_play.vars_files {
        let path = std::path::PathBuf::from(vf);
        if let Ok(body) = std::fs::read_to_string(&path) {
            if let Ok(toml::Value::Table(t)) = body.parse::<toml::Value>() {
                for (k, v) in t {
                    vars.insert(k, v);
                }
            }
        }
    }
    for (k, v) in &raw_play.vars {
        vars.insert(k.clone(), v.clone());
    }
    for (k, v) in role_vars {
        vars.insert(k.clone(), v.clone());
    }
    for (k, v) in role_param_vars {
        vars.insert(k.clone(), v.clone());
    }
    for (k, v) in &opts.extra_vars {
        vars.insert(k.clone(), v.clone());
    }
    vars.insert(
        "inventory_hostname".into(),
        toml::Value::String(host.name.clone()),
    );
    let short = host
        .name
        .split('.')
        .next()
        .unwrap_or(&host.name)
        .to_string();
    vars.insert(
        "inventory_hostname_short".into(),
        toml::Value::String(short),
    );
    vars.insert(
        "inventory_dir".into(),
        toml::Value::String(play_ctx.inv_dir.clone()),
    );
    vars.insert(
        "playbook_dir".into(),
        toml::Value::String(play_ctx.inv_dir.clone()),
    );
    {
        let mut g = toml::map::Map::new();
        g.insert(
            "all".into(),
            toml::Value::Array(play_ctx.inventory_host_names.clone()),
        );
        g.insert(
            "ungrouped".into(),
            toml::Value::Array(play_ctx.inventory_host_names.clone()),
        );
        vars.insert("groups".into(), toml::Value::Table(g));
    }
    vars.insert(
        "play_hosts".into(),
        toml::Value::Array(play_ctx.play_host_names.clone()),
    );
    vars.insert(
        "ansible_play_hosts".into(),
        toml::Value::Array(play_ctx.play_host_names.clone()),
    );
    vars.insert(
        "ansible_play_name".into(),
        toml::Value::String(raw_play.name.clone()),
    );
    vars.insert("omit".into(), toml::Value::String(omit_placeholder()));
    let run_tags: Vec<toml::Value> = opts
        .tags
        .iter()
        .map(|t| toml::Value::String(t.clone()))
        .collect();
    vars.insert("ansible_run_tags".into(), toml::Value::Array(run_tags));
    let skip_tags: Vec<toml::Value> = opts
        .skip_tags
        .iter()
        .map(|t| toml::Value::String(t.clone()))
        .collect();
    vars.insert("ansible_skip_tags".into(), toml::Value::Array(skip_tags));
    vars.insert(
        "ansible_check_mode".into(),
        toml::Value::Boolean(opts.check_mode),
    );
    vars.insert(
        "ansible_diff_mode".into(),
        toml::Value::Boolean(opts.diff_mode),
    );
    vars.insert(
        "ansible_version".into(),
        play_ctx.ansible_version_table.clone(),
    );
    vars.insert("hostvars".into(), play_ctx.hostvars_table.clone());
    vars
}

/// Run all tasks for a single host. Returns the host's stats + handler
/// notifications. Each parallel host gets its own connection instance and own
/// vars map; nothing is shared mutably.
#[allow(clippy::too_many_arguments)]
fn run_host_tasks(
    host: &Host,
    play_idx: usize,
    raw_play: Arc<RawPlay>,
    tasks: Arc<Vec<Task>>,
    imports: Arc<IndexMap<String, String>>,
    catalog: Arc<ModuleCatalog>,
    templater: Arc<Templater>,
    play_ctx: Arc<PlayContext>,
    role_defaults: Arc<Vars>,
    role_vars: Arc<Vars>,
    role_param_vars: Arc<Vars>,
    opts: Arc<RunOptions>,
    mode: OutputMode,
    run_once_cache: RunOnceCache,
) -> Result<HostResult> {
    let mut vars = build_host_vars(
        host,
        &raw_play,
        &role_defaults,
        &role_vars,
        &role_param_vars,
        &opts,
        &play_ctx,
    );
    let mut stats = RunStats::default();
    let mut notified_for_host: IndexSet<String> = IndexSet::new();
    let connection = LocalSync;

    // start_at_task: once `start_at_task` is set, skip every top-level task
    // until we encounter one whose `name` matches. M1 only matches at the top
    // level — block children are not searched. Per-host because each host runs
    // the same task list independently; `started` is a fresh local here.
    let mut started: bool = opts.start_at_task.is_none();

    for (task_idx, task) in tasks.iter().enumerate() {
        if !started {
            let target = opts
                .start_at_task
                .as_deref()
                .expect("started=false implies start_at_task is Some");
            if task.name.as_deref() == Some(target) {
                started = true;
            } else {
                // Emit a TaskStart + Skipped Outcome with reason
                // `start_at_task` so events surface the skip clearly.
                emit(
                    &mode,
                    &Event::TaskStart {
                        play_index: play_idx,
                        task_index: task_idx,
                        name: task
                            .name
                            .clone()
                            .unwrap_or_else(|| task.module_name.clone()),
                        module: task.module_name.clone(),
                    },
                );
                let outcome = runsible_core::types::Outcome {
                    module: task.module_name.clone(),
                    host: host.name.clone(),
                    status: OutcomeStatus::Skipped,
                    elapsed_ms: 0,
                    returns: serde_json::json!({"skipped_reason": "start_at_task"}),
                };
                stats.skipped += 1;
                emit(
                    &mode,
                    &Event::TaskOutcome {
                        play_index: play_idx,
                        task_index: task_idx,
                        outcome,
                    },
                );
                continue;
            }
        }

        let _ = execute_one_task(
            task,
            play_idx,
            task_idx,
            host,
            &mut vars,
            &raw_play.tags,
            &opts,
            &catalog,
            &templater,
            &connection,
            &mode,
            &mut stats,
            &mut notified_for_host,
            &imports,
            &raw_play.module_defaults,
            &run_once_cache,
        )?;
    }

    Ok(HostResult {
        host_name: host.name.clone(),
        stats,
        notified_handlers: notified_for_host,
    })
}

#[derive(Debug)]
pub struct RunResult {
    pub ok: usize,
    pub changed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub elapsed_ms: u64,
}

impl RunResult {
    pub fn exit_code(&self) -> i32 {
        if self.failed > 0 { 2 } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_filter_no_filters() {
        let tags: Vec<&String> = vec![];
        assert!(tag_filter_passes(&tags, &[], &[]));
    }

    #[test]
    fn tag_filter_only_matching() {
        let web = "web".to_string();
        let tags = vec![&web];
        assert!(tag_filter_passes(&tags, &["web".into()], &[]));
        assert!(!tag_filter_passes(&tags, &["db".into()], &[]));
    }

    #[test]
    fn tag_filter_skip_subtracts() {
        let web = "web".to_string();
        let tags = vec![&web];
        assert!(!tag_filter_passes(&tags, &[], &["web".into()]));
    }

    #[test]
    fn tag_filter_never_skipped_by_default() {
        let never = "never".to_string();
        let tags = vec![&never];
        assert!(!tag_filter_passes(&tags, &[], &[]));
        assert!(tag_filter_passes(&tags, &["never".into()], &[]));
    }

    #[test]
    fn tag_filter_always_runs_unless_skipped() {
        let always = "always".to_string();
        let tags = vec![&always];
        assert!(tag_filter_passes(&tags, &["other".into()], &[]));
        assert!(!tag_filter_passes(&tags, &[], &["always".into()]));
    }
}
