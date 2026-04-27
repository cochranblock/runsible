//! M1 engine: variables, templating, when/register/tags/handlers.
//!
//! Execution per host:
//!   merge vars (host + play) → for each task:
//!     filter by tags → eval `when` (skip if false) → render args via templater
//!     → plan() → emit PlanComputed → apply() → register outcome → notify handlers
//!     → emit TaskOutcome
//!   end-of-play: flush notified handlers (fire each handler's task)

use std::time::Instant;

use indexmap::{IndexMap, IndexSet};
use runsible_connection::LocalSync;
use runsible_core::{
    event::Event,
    traits::ExecutionContext,
    types::{Host, OutcomeStatus, Vars},
};

use crate::{
    ast::Task,
    catalog::ModuleCatalog,
    errors::{PlaybookError, Result},
    output::{emit, OutputMode},
    parse::{parse_playbook, resolve_handler, resolve_task},
    templating::Templater,
};

/// CLI-supplied filters / extras.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub tags: Vec<String>,
    pub skip_tags: Vec<String>,
    pub extra_vars: Vars,
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
    let catalog = ModuleCatalog::with_builtins();
    let templater = Templater::new();
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

        let task_sequence: Vec<&toml::Value> = raw_play
            .pre_tasks
            .iter()
            .chain(raw_play.tasks.iter())
            .chain(raw_play.post_tasks.iter())
            .collect();

        // Resolve all tasks once (parse-time validation).
        let tasks: Vec<Task> = task_sequence
            .iter()
            .map(|raw| resolve_task(raw, &pb.imports))
            .collect::<Result<_>>()?;

        // Resolve all handlers once.
        let handlers: IndexMap<String, Task> = raw_play
            .handlers
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

        let mut play_stats = RunStats::default();
        let mut play_changed_handlers: IndexMap<String, IndexSet<String>> = IndexMap::new();

        for host in &play_hosts {
            // Per-host vars: host inline vars (lvl 2) → play vars (lvl 3) → extra_vars (lvl 4).
            let mut vars: Vars = host.vars.clone();
            for (k, v) in &raw_play.vars {
                vars.insert(k.clone(), v.clone());
            }
            for (k, v) in &opts.extra_vars {
                vars.insert(k.clone(), v.clone());
            }
            // Magic vars
            vars.insert(
                "inventory_hostname".into(),
                toml::Value::String(host.name.clone()),
            );

            let mut notified_for_host: IndexSet<String> = IndexSet::new();

            for (task_idx, task) in tasks.iter().enumerate() {
                let final_status = execute_one_task(
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
                    &mut play_stats,
                    &mut notified_for_host,
                    &pb.imports,
                )?;
                let _ = final_status;
            }

            // Aggregate notifications across hosts.
            for h_id in notified_for_host {
                play_changed_handlers
                    .entry(h_id)
                    .or_default()
                    .insert(host.name.clone());
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
                    check_mode: false,
                };
                let plan = module.plan(&rendered, &ctx).map_err(|e| PlaybookError::ExecFailed {
                    host: host.name.clone(),
                    message: e.to_string(),
                })?;
                let outcome = module.apply(&plan, &ctx).map_err(|e| PlaybookError::ExecFailed {
                    host: host.name.clone(),
                    message: e.to_string(),
                })?;
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
) -> Result<OutcomeStatus> {
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
            )?;
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

    let mut worst = OutcomeStatus::Ok;

    for iter_item in iterations {
        if let Some(item) = &iter_item {
            vars.insert(task.loop_var.clone(), item.clone());
        }

        let max_attempts = if task.until.is_some() { task.retries.max(1) } else { 1 };
        let mut last_outcome: Option<runsible_core::types::Outcome> = None;

        for attempt in 1..=max_attempts {
            let rendered_args = templater.render_value(&task.args, vars).map_err(|e| {
                PlaybookError::ExecFailed {
                    host: host.name.clone(),
                    message: format!("template error: {e}"),
                }
            })?;

            let ctx = ExecutionContext {
                host,
                vars,
                connection,
                check_mode: false,
            };

            let plan = module.plan(&rendered_args, &ctx).map_err(|e| {
                PlaybookError::ExecFailed {
                    host: host.name.clone(),
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

            let mut outcome = module.apply(&plan, &ctx).map_err(|e| {
                PlaybookError::ExecFailed {
                    host: host.name.clone(),
                    message: e.to_string(),
                }
            })?;

            if task.module_name == "runsible_builtin.set_fact" {
                if let Some(obj) = plan.diff.as_object() {
                    for (k, v) in obj {
                        if let Ok(tv) = json_to_toml_value(v.clone()) {
                            vars.insert(k.clone(), tv);
                        }
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
fn resolve_inventory(spec: &str) -> Result<Vec<Host>> {
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

fn pattern_matches(pattern: &str, host_name: &str) -> bool {
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

#[derive(Debug, Default)]
struct RunStats {
    ok: usize,
    changed: usize,
    failed: usize,
    skipped: usize,
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
