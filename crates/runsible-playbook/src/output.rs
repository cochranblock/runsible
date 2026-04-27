//! Event output: NDJSON when stdout is not a TTY, pretty when it is.

use std::io::{self, IsTerminal, Write};

use runsible_core::event::Event;

#[derive(Clone, Copy)]
pub enum OutputMode {
    Ndjson,
    Pretty,
}

impl OutputMode {
    pub fn detect() -> Self {
        if io::stdout().is_terminal() {
            OutputMode::Pretty
        } else {
            OutputMode::Ndjson
        }
    }
}

pub fn emit(mode: &OutputMode, event: &Event) {
    match mode {
        OutputMode::Ndjson => {
            if let Ok(line) = event.to_ndjson_line() {
                print!("{line}");
                let _ = io::stdout().flush();
            }
        }
        OutputMode::Pretty => pretty_print(event),
    }
}

fn pretty_print(event: &Event) {
    match event {
        Event::RunStart { playbook, host_count, .. } => {
            let bar = "=".repeat(60);
            println!("\n{bar}");
            println!("PLAY  {playbook}  [{host_count} host(s)]");
            println!("{bar}");
        }
        Event::PlayStart { name, target_pattern, host_count, .. } => {
            let fill = "*".repeat(60usize.saturating_sub(name.len() + 7));
            println!("\nPLAY [{name}] {fill}");
            println!("hosts: {target_pattern}  ({host_count} matched)");
        }
        Event::TaskStart { name, module, .. } => {
            let fill = "*".repeat(60usize.saturating_sub(name.len() + 7));
            println!("\nTASK [{name}] {fill}  ({})", module);
        }
        Event::PlanComputed { plan, .. } => {
            if plan.will_change {
                println!("  plan: CHANGE");
            }
        }
        Event::TaskOutcome { outcome, .. } => {
            use runsible_core::types::OutcomeStatus::*;
            let label = match outcome.status {
                Ok => "ok",
                Changed => "changed",
                Failed => "FAILED",
                Skipped => "skipped",
                Unreachable => "UNREACHABLE",
            };
            let msg = outcome
                .returns
                .get("msg")
                .and_then(|v| v.as_str())
                .map(|s| format!("  => {s}"))
                .unwrap_or_default();
            let check_tag = if outcome
                .returns
                .get("check_mode")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                " (check)"
            } else {
                ""
            };
            println!("  {label}{check_tag}: [{}]{msg}", outcome.host);
            if let Some(diff_block) = render_diff_block(&outcome.returns) {
                print!("{diff_block}");
            }
        }
        Event::PlayEnd { ok, changed, failed, unreachable, skipped, .. } => {
            println!("\n  ok={ok}  changed={changed}  failed={failed}  unreachable={unreachable}  skipped={skipped}");
        }
        Event::RunSummary { ok, changed, failed, unreachable, skipped, elapsed_ms } => {
            let bar = "=".repeat(60);
            println!("\n{bar}");
            println!("RECAP");
            println!("  ok={ok}  changed={changed}  failed={failed}  unreachable={unreachable}  skipped={skipped}");
            println!("  elapsed: {elapsed_ms}ms");
            println!("{bar}");
        }
        Event::Error { host, message } => {
            let loc = host.as_deref().unwrap_or("controller");
            eprintln!("ERROR [{loc}]: {message}");
        }
        _ => {}
    }
}

/// Pull `before`/`after` strings out of an outcome's `returns` (or its embedded
/// `diff` sub-object, as produced by check-mode synthesized outcomes) and
/// render a minimal unified-style diff. Returns `None` if neither side is
/// present.
fn render_diff_block(returns: &serde_json::Value) -> Option<String> {
    // Two shapes:
    //   1. returns.before / returns.after  (rare — modules don't currently emit)
    //   2. returns.diff.before / returns.diff.after  (check_mode synth path,
    //      and diff_mode-enabled mutating modules forwarding via plan.diff)
    let (before, after) = if let (Some(b), Some(a)) = (
        returns.get("before").and_then(|v| v.as_str()),
        returns.get("after").and_then(|v| v.as_str()),
    ) {
        (b, a)
    } else if let Some(diff) = returns.get("diff") {
        let b = diff.get("before").and_then(|v| v.as_str())?;
        let a = diff.get("after").and_then(|v| v.as_str())?;
        (b, a)
    } else {
        return None;
    };

    if before == after {
        return None;
    }

    let mut out = String::new();
    out.push_str("    --- before\n");
    out.push_str("    +++ after\n");
    for line in before.lines() {
        out.push_str("    -");
        out.push_str(line);
        out.push('\n');
    }
    for line in after.lines() {
        out.push_str("    +");
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}
