//! REPL loop driver.
//!
//! Reads lines via `rustyline`, dispatches them through `parse::parse_line`,
//! and prints a colored summary for each invocation. The actual module work
//! is delegated to `runsible_playbook::run` via a synthetic single-task
//! playbook, the same construction `runsible` ad-hoc uses.

use colored::Colorize;
use rustyline::error::ReadlineError;
use rustyline::Editor;

use crate::errors::{ConsoleError, Result};
use crate::parse::{parse_line, ReplCommand};

/// Run the REPL against a single startup-supplied target.
///
/// `target` is the host name (e.g. `localhost`) used as the inventory.
/// `connection` and `user` are accepted at the CLI layer but currently
/// unused at M0 — only `local` makes sense against the built-in modules.
pub fn run_repl(target: &str, connection: &str, user: Option<&str>) -> Result<()> {
    println!(
        "{} {} {} {}",
        "runsible-console".bold(),
        env!("CARGO_PKG_VERSION"),
        "—".dimmed(),
        format!("target: {target}").cyan()
    );
    let _ = (connection, user); // accepted for forward-compat; unused at M0.
    println!("{}", "Type 'quit' or Ctrl-D to exit.".dimmed());

    let mut editor: Editor<(), rustyline::history::DefaultHistory> =
        Editor::new().map_err(|e| ConsoleError::Readline(e.to_string()))?;

    loop {
        let line = match editor.readline("runsible> ") {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C: drop the in-progress line, keep the loop alive.
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl-D on an empty line: clean exit.
                println!();
                break;
            }
            Err(e) => return Err(ConsoleError::Readline(e.to_string())),
        };

        let cmd = parse_line(&line);

        // Add non-trivial lines to history so up-arrow works within a session.
        if !matches!(cmd, ReplCommand::Empty | ReplCommand::Comment) {
            let _ = editor.add_history_entry(line.as_str());
        }

        match cmd {
            ReplCommand::Empty | ReplCommand::Comment => continue,
            ReplCommand::Quit => break,
            ReplCommand::Unknown(msg) => {
                eprintln!("{} {msg}", "error:".red().bold());
            }
            ReplCommand::Invoke { module, args } => {
                if let Err(e) = invoke(&module, &args, target) {
                    eprintln!("{} {e}", "error:".red().bold());
                }
            }
        }
    }

    Ok(())
}

/// Build a synthetic single-task playbook around the user's invocation, hand
/// it to the engine, and pretty-print the resulting summary.
fn invoke(module: &str, args: &toml::Value, target: &str) -> Result<()> {
    let alias = module.rsplit('.').next().unwrap_or(module).to_string();

    let args_inline = inline_table(args);

    let playbook = format!(
        r#"schema = "runsible.playbook.v1"

[imports]
{alias} = "{module}"

[[plays]]
name = "console"
hosts = "{target}"

[[plays.tasks]]
name = "console task"
{alias} = {args_inline}
"#
    );

    // Inventory string: a trailing comma forces inline single-host parsing,
    // matching how `runsible -i localhost,` works.
    let inventory_spec = format!("{target},");

    let result = runsible_playbook::run(&playbook, &inventory_spec, "console")
        .map_err(|e| ConsoleError::Playbook(e.to_string()))?;

    print_summary(&result);

    Ok(())
}

/// Render the args table as an inline TOML value (`{}` or `{ k = "v", ... }`).
///
/// Mirrors the helper inside `runsible::build_synthetic_playbook` — kept
/// inline here to avoid pulling in the `runsible` ad-hoc binary crate just
/// for one helper.
fn inline_table(args: &toml::Value) -> String {
    let table = match args {
        toml::Value::Table(t) => t,
        _ => return "{}".to_string(),
    };

    if table.is_empty() {
        return "{}".to_string();
    }

    let pairs: Vec<String> = table
        .iter()
        .map(|(k, v)| {
            let vs = match v {
                toml::Value::String(s) => format!("\"{s}\""),
                toml::Value::Boolean(b) => b.to_string(),
                toml::Value::Integer(i) => i.to_string(),
                toml::Value::Float(f) => f.to_string(),
                other => format!("\"{other}\""),
            };
            format!("{k} = {vs}")
        })
        .collect();

    format!("{{ {} }}", pairs.join(", "))
}

/// Print one colored summary line for a finished run.
fn print_summary(result: &runsible_playbook::RunResult) {
    let ok_part = format!("ok={}", result.ok).green();
    let changed_part = format!("changed={}", result.changed).yellow();
    let failed_part = if result.failed > 0 {
        format!("failed={}", result.failed).red().bold()
    } else {
        format!("failed={}", result.failed).normal()
    };
    let elapsed = format!("({}ms)", result.elapsed_ms).dimmed();

    println!("{ok_part} {changed_part} {failed_part}  {elapsed}");
}
