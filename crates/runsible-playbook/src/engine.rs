//! M0 engine: single-task happy-path runner.
//!
//! Execution model:
//!   parse playbook → resolve inventory → for each play / host / task:
//!     emit TaskStart → plan() → emit PlanComputed → apply() → emit TaskOutcome
//!   emit RunSummary
//!
//! At M0 apply() is always called regardless of plan.is_empty() so that
//! informational modules (debug) always fire.

use std::time::Instant;

use runsible_core::{
    event::Event,
    types::{Host, Vars},
};

use crate::{
    catalog::ModuleCatalog,
    errors::{PlaybookError, Result},
    output::{emit, OutputMode},
    parse::{parse_playbook, resolve_task},
};

/// Parse and run a playbook file against the given inventory string.
///
/// `inventory_spec` mirrors the Ansible `-i` flag:
///   - `"localhost,"` or `"host1,host2,"` — inline comma-separated host list
///   - a file path that exists on disk — loaded via runsible-inventory
///   - a bare host name without comma — treated as a single inline host
pub fn run(playbook_src: &str, inventory_spec: &str, playbook_label: &str) -> Result<RunResult> {
    let start = Instant::now();
    let mode = OutputMode::detect();
    let catalog = ModuleCatalog::with_builtins();

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

        // Resolve the host pattern against our inline host list.
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

        let mut play_stats = RunStats::default();

        for (task_idx, raw_task) in task_sequence.iter().enumerate() {
            let task = resolve_task(raw_task, &pb.imports)?;

            let module = catalog
                .get(&task.module_name)
                .ok_or_else(|| PlaybookError::ModuleNotFound(task.module_name.clone()))?;

            emit(
                &mode,
                &Event::TaskStart {
                    play_index: play_idx,
                    task_index: task_idx,
                    name: task.name.clone().unwrap_or_else(|| task.module_name.clone()),
                    module: task.module_name.clone(),
                },
            );

            for host in &play_hosts {
                let plan = module
                    .plan(&task.args, host)
                    .map_err(|e| PlaybookError::ExecFailed {
                        host: host.name.clone(),
                        message: e.to_string(),
                    })?;

                emit(
                    &mode,
                    &Event::PlanComputed {
                        play_index: play_idx,
                        task_index: task_idx,
                        plan: plan.clone(),
                    },
                );

                let outcome = module
                    .apply(&plan, host)
                    .map_err(|e| PlaybookError::ExecFailed {
                        host: host.name.clone(),
                        message: e.to_string(),
                    })?;

                use runsible_core::types::OutcomeStatus::*;
                match outcome.status {
                    Ok => play_stats.ok += 1,
                    Changed => play_stats.changed += 1,
                    Skipped => play_stats.skipped += 1,
                    Failed | Unreachable => play_stats.failed += 1,
                }

                emit(
                    &mode,
                    &Event::TaskOutcome {
                        play_index: play_idx,
                        task_index: task_idx,
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
        elapsed_ms,
    })
}

/// Parse an inventory spec into a flat host list.
fn resolve_inventory(spec: &str) -> Result<Vec<Host>> {
    // Inline host list: contains a comma or ends with a comma.
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

    // Try as a file.
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

    // Bare hostname.
    Ok(vec![Host {
        name: spec.to_string(),
        vars: Vars::new(),
    }])
}

/// Minimal pattern match for M0: exact name, group name, `all`/`*`, or glob.
fn pattern_matches(pattern: &str, host_name: &str) -> bool {
    if pattern == "all" || pattern == "*" {
        return true;
    }
    if pattern == host_name {
        return true;
    }
    // Colon-joined union: any segment matches.
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
    pub elapsed_ms: u64,
}

impl RunResult {
    /// Standard Ansible-style exit code: 0 = ok, 2 = host failures.
    pub fn exit_code(&self) -> i32 {
        if self.failed > 0 { 2 } else { 0 }
    }
}
