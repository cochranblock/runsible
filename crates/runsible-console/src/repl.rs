//! REPL loop driver.
//!
//! Reads lines via `rustyline`, dispatches them through `parse::parse_line`,
//! and prints a colored summary for each invocation. The actual module work
//! is delegated to `runsible_playbook::run` via a synthetic single-task
//! playbook, the same construction `runsible` ad-hoc uses.

use std::path::PathBuf;

use colored::Colorize;
use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};
use rustyline::error::ReadlineError;
use rustyline::Editor;

use crate::errors::{ConsoleError, Result};
use crate::parse::{parse_line, ReplCommand};

// ---------------------------------------------------------------------------
// Tab completer
// ---------------------------------------------------------------------------

/// Completer for the runsible REPL. Holds the list of module names (FQCNs and
/// short aliases) to complete against. Only the **first whitespace-separated
/// word** of the line is considered a module name; subsequent tokens fall
/// through to no completion (they are `key=value` args).
pub struct ConsoleCompleter {
    pub modules: Vec<String>,
}

impl ConsoleCompleter {
    /// Build a completer from the runsible-playbook module catalog plus the
    /// short aliases users routinely type (`debug`, `ping`, …).
    pub fn from_builtins() -> Self {
        let catalog = runsible_playbook::catalog::ModuleCatalog::with_builtins();
        let mut modules: Vec<String> = catalog.names().map(String::from).collect();

        // Short aliases — the snippet `debug = { … }` form people type without
        // an explicit `[imports]` entry.
        for fqcn in catalog.names().collect::<Vec<&str>>() {
            if let Some(short) = fqcn.rsplit('.').next() {
                if short != fqcn {
                    modules.push(short.to_string());
                }
            }
        }

        // REPL meta-commands — completion on `qu<TAB>` should offer `quit`.
        modules.push("quit".to_string());
        modules.push("exit".to_string());

        // Stable order; deduplicated.
        modules.sort();
        modules.dedup();

        ConsoleCompleter { modules }
    }

    /// `Context`-free helper exposing the underlying completion logic so
    /// callers (e.g. f30 / unit tests) can drive completion without having
    /// to build a rustyline `Context`. Returns (`word_start_offset`, candidates_as_strings).
    pub fn complete_word(&self, line: &str, pos: usize) -> (usize, Vec<String>) {
        let prefix = &line[..pos.min(line.len())];
        let word_start = prefix
            .rfind(|c: char| c.is_whitespace())
            .map(|i| i + 1)
            .unwrap_or(0);
        let word = &prefix[word_start..];

        // Match the trait impl: only complete the first token.
        let typed_only_first_token = !prefix[..word_start].chars().any(|c| !c.is_whitespace());
        if !typed_only_first_token {
            return (pos, Vec::new());
        }

        let candidates: Vec<String> = self
            .modules
            .iter()
            .filter(|m| m.starts_with(word))
            .cloned()
            .collect();
        (word_start, candidates)
    }
}

impl Completer for ConsoleCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let prefix = &line[..pos.min(line.len())];
        let word_start = prefix.rfind(|c: char| c.is_whitespace())
            .map(|i| i + 1)
            .unwrap_or(0);
        let word = &prefix[word_start..];

        // Only complete the first token (module name). If a space precedes the
        // cursor we're typing args, not a module — return empty.
        let typed_only_first_token = !prefix[..word_start].chars().any(|c| !c.is_whitespace());
        if !typed_only_first_token {
            return Ok((pos, Vec::new()));
        }

        let candidates: Vec<Pair> = self
            .modules
            .iter()
            .filter(|m| m.starts_with(word))
            .map(|m| Pair {
                display: m.clone(),
                replacement: m.clone(),
            })
            .collect();
        Ok((word_start, candidates))
    }
}

// Minimal trait-boilerplate so ConsoleCompleter can serve as a Helper. All
// interesting behavior is in Completer; the rest are no-ops/defaults.
impl Hinter for ConsoleCompleter {
    type Hint = String;
}
impl Highlighter for ConsoleCompleter {}
impl Validator for ConsoleCompleter {}
impl Helper for ConsoleCompleter {}

// ---------------------------------------------------------------------------
// History path
// ---------------------------------------------------------------------------

/// Resolve `~/.runsible/console_history.txt`. Returns `None` if HOME is unset.
fn history_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let mut p = PathBuf::from(home);
    p.push(".runsible");
    Some(p.join("console_history.txt"))
}

/// Ensure the parent dir of `path` exists. Best-effort.
fn ensure_parent_dir(path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
}

// ---------------------------------------------------------------------------
// REPL
// ---------------------------------------------------------------------------

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

    let mut editor: Editor<ConsoleCompleter, rustyline::history::DefaultHistory> =
        Editor::new().map_err(|e| ConsoleError::Readline(e.to_string()))?;
    editor.set_helper(Some(ConsoleCompleter::from_builtins()));

    // Persistent history: load on startup, save on exit.
    let hist = history_path();
    if let Some(p) = &hist {
        ensure_parent_dir(p);
        let _ = editor.load_history(p); // missing file is fine
    }

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

    // Persist history on clean exit (quit / Ctrl-D).
    if let Some(p) = &hist {
        let _ = editor.save_history(p);
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rustyline::history::DefaultHistory;
    use rustyline::history::History;

    /// Construct a Context against a fresh empty history. Tests don't care
    /// about history; they just need *some* Context to feed `complete()`.
    fn fresh_context() -> DefaultHistory {
        DefaultHistory::new()
    }

    #[test]
    fn from_builtins_includes_short_aliases() {
        let c = ConsoleCompleter::from_builtins();
        // From short-alias derivation
        assert!(c.modules.iter().any(|m| m == "debug"), "should include 'debug' alias");
        assert!(c.modules.iter().any(|m| m == "ping"));
        // From FQCN names from the catalog
        assert!(c.modules.iter().any(|m| m == "runsible_builtin.debug"));
        // Meta-commands
        assert!(c.modules.iter().any(|m| m == "quit"));
        assert!(c.modules.iter().any(|m| m == "exit"));
    }

    #[test]
    fn completer_returns_debug_for_deb_prefix() {
        let c = ConsoleCompleter::from_builtins();
        let h = fresh_context();
        let ctx = Context::new(&h);
        let (start, cands) = c.complete("deb", 3, &ctx).unwrap();
        assert_eq!(start, 0);
        let names: Vec<&str> = cands.iter().map(|p| p.replacement.as_str()).collect();
        assert!(names.contains(&"debug"), "expected debug in candidates: {names:?}");
    }

    #[test]
    fn completer_starts_at_word_boundary() {
        let c = ConsoleCompleter::from_builtins();
        let h = fresh_context();
        let ctx = Context::new(&h);
        // After leading whitespace the start should be the position of the
        // first non-whitespace char.
        let (start, _cands) = c.complete("  deb", 5, &ctx).unwrap();
        assert_eq!(start, 2, "word should start after the leading whitespace");
    }

    #[test]
    fn completer_skips_arg_token() {
        let c = ConsoleCompleter::from_builtins();
        let h = fresh_context();
        let ctx = Context::new(&h);
        // Once a space has been typed, we're in args land — no completion.
        let (_start, cands) = c.complete("debug ms", 8, &ctx).unwrap();
        assert!(cands.is_empty(), "should not complete args; got {:?}",
            cands.iter().map(|p| &p.replacement).collect::<Vec<_>>());
    }

    #[test]
    fn completer_empty_word_returns_all() {
        let c = ConsoleCompleter::from_builtins();
        let h = fresh_context();
        let ctx = Context::new(&h);
        let (_start, cands) = c.complete("", 0, &ctx).unwrap();
        assert!(!cands.is_empty(), "empty prefix should match everything");
        // Sanity check that it has something we know is in the list.
        assert!(cands.iter().any(|p| p.replacement == "debug"));
    }

    #[test]
    fn completer_unknown_prefix_yields_empty() {
        let c = ConsoleCompleter::from_builtins();
        let h = fresh_context();
        let ctx = Context::new(&h);
        let (_start, cands) = c.complete("zzqqxx", 6, &ctx).unwrap();
        assert!(cands.is_empty(), "no module starts with zzqqxx");
    }

    #[test]
    fn history_path_uses_home() {
        // history_path requires HOME; ensure it's set to a known value here.
        // Snapshot existing HOME and restore it to keep tests hermetic.
        let prev = std::env::var_os("HOME");
        std::env::set_var("HOME", "/tmp/runsible-test-home");
        let p = history_path().expect("HOME set, path should resolve");
        assert!(p.ends_with(".runsible/console_history.txt"));
        if let Some(prev) = prev {
            std::env::set_var("HOME", prev);
        }
    }

    #[test]
    fn ensure_parent_dir_creates_missing_path() {
        let dir = std::env::temp_dir().join("runsible_console_test_xyz");
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("history.txt");
        ensure_parent_dir(&file);
        assert!(dir.exists(), "ensure_parent_dir should create the directory");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn from_builtins_is_sorted_and_unique() {
        let c = ConsoleCompleter::from_builtins();
        let mut sorted = c.modules.clone();
        sorted.sort();
        assert_eq!(c.modules, sorted, "modules list should be sorted");
        let mut deduped = c.modules.clone();
        deduped.dedup();
        assert_eq!(c.modules, deduped, "modules list should be deduplicated");
    }

    /// Ensure `History` trait is in scope (used to keep import alive).
    #[test]
    fn history_trait_smoke() {
        let h = fresh_context();
        let _ = h.len();
    }
}
