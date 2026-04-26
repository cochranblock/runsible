//! runsible-lint CLI — M0
//!
//! Usage:
//!   runsible-lint [--profile <profile>] [--format text|json] [--explain <rule>]
//!                 [--list-rules] [--skip-rules <id,...>] [--strict]
//!                 [<file>...]

use std::path::PathBuf;
use std::process;

use anyhow::Result;
use clap::Parser;

use runsible_lint::{
    discover_lint_config, lint_file, list_rules, LintConfig, LintResult, Profile, Severity,
};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
#[command(
    name = "runsible-lint",
    about = "runsible-lint: TOML-native static analysis for runsible playbooks",
    version
)]
struct Cli {
    /// Files to lint. If none given, lint nothing (future: discover from project root).
    #[arg(value_name = "FILE")]
    files: Vec<PathBuf>,

    /// Override the active rule profile (min, basic, moderate, safety, shared, production).
    #[arg(long, env = "RUNSIBLE_LINT_PROFILE", value_name = "PROFILE")]
    profile: Option<String>,

    /// Output format: text (default) or json.
    #[arg(long, env = "RUNSIBLE_LINT_FORMAT", value_name = "FORMAT", default_value = "text")]
    format: String,

    /// Print extended description of one rule and exit.
    #[arg(long, value_name = "RULE_ID")]
    explain: Option<String>,

    /// List all known rules and exit.
    #[arg(long)]
    list_rules: bool,

    /// Comma-separated list of rule IDs to skip.
    #[arg(long, value_name = "ID,...")]
    skip_rules: Option<String>,

    /// Treat warnings as errors for exit-code purposes.
    #[arg(long, env = "RUNSIBLE_LINT_STRICT")]
    strict: bool,
}

// ---------------------------------------------------------------------------
// Rule explain texts (extended descriptions for --explain)
// ---------------------------------------------------------------------------

fn explain_text(id: &str) -> Option<&'static str> {
    match id {
        "L001" => Some(
            "L001 — schema field missing or wrong value\n\
             \n\
             Every runsible playbook must start with:\n\
             \n\
             \tschema = \"runsible.playbook.v1\"\n\
             \n\
             Without this field the runsible runtime refuses to load the file. \
             Correct the schema string or add the line at the top of the file.",
        ),
        "L002" => Some(
            "L002 — play missing `name` field\n\
             \n\
             Every `[[plays]]` entry should have a `name` key. \
             The name appears in run output and is required for `--start-at-play` to work. \
             Add `name = \"<descriptive name>\"` to the play.",
        ),
        "L003" => Some(
            "L003 — task missing `name` field\n\
             \n\
             Tasks without names are hard to trace in run output. \
             Add `name = \"<what this task does>\"` to each task.",
        ),
        "L004" => Some(
            "L004 — task has zero module keys\n\
             \n\
             A task table must contain exactly one key that is not in the reserved \
             meta-key list (name, tags, when, register, …). That key identifies the \
             module to invoke. A task with only meta keys is a no-op that likely \
             indicates a typo or incomplete cut-paste.",
        ),
        "L005" => Some(
            "L005 — task has multiple module keys\n\
             \n\
             Two or more non-meta keys in a single task is ambiguous: runsible \
             cannot determine which module to invoke. Split the task into two \
             separate `[[plays.tasks]]` entries.",
        ),
        "L006" => Some(
            "L006 — module alias not in [imports]\n\
             \n\
             Short module names (without a dot) must be declared in the `[imports]` \
             block before use. Either add the alias there or use the full FQCN \
             directly (e.g. `runsible_builtin.debug`).",
        ),
        "L007" => Some(
            "L007 — [[plays]] array is empty\n\
             \n\
             A playbook with no plays does nothing. Either add at least one \
             `[[plays]]` entry or remove the file.",
        ),
        "L008" => Some(
            "L008 — hosts field missing on a play\n\
             \n\
             The `hosts` key tells runsible which inventory hosts to target. \
             Without it the play cannot run. Add `hosts = \"<pattern>\"` to the play.",
        ),
        "L009" => Some(
            "L009 — duplicate play names\n\
             \n\
             Two plays in the same file share a name. `--start-at-play` becomes \
             ambiguous. Rename one of the plays.",
        ),
        "L010" => Some(
            "L010 — duplicate task names within a play\n\
             \n\
             Two tasks in the same play share a name. `--start-at-task` becomes \
             ambiguous. Rename one of the tasks.",
        ),
        "L011" => Some(
            "L011 — task name too long (> 80 chars)\n\
             \n\
             Long task names are truncated in most terminal widths and make log \
             output hard to read. Shorten the name to 80 characters or fewer.",
        ),
        "L012" => Some(
            "L012 — play name too long (> 80 chars)\n\
             \n\
             Same rationale as L011 but for play names.",
        ),
        "L013" => Some(
            "L013 — [imports] block present but empty\n\
             \n\
             An empty `[imports]` section adds noise without value. \
             Either add aliases or remove the section.",
        ),
        "L014" => Some(
            "L014 — [imports] alias shadows a known builtin short name\n\
             \n\
             Declaring `debug = \"runsible_builtin.command\"` in `[imports]` \
             shadows the expected `runsible_builtin.debug` expansion. \
             Use a different alias name to avoid confusion.",
        ),
        "L015" => Some(
            "L015 — register without when guard\n\
             \n\
             A task registers a variable but no subsequent task uses it in a `when` \
             guard (or the registered var is not referenced at all in this file). \
             This is usually dead code. Either use the variable or remove `register`.",
        ),
        "L016" => Some(
            "L016 — no_log = false explicitly set\n\
             \n\
             `no_log` defaults to false; writing it explicitly is harmless but \
             suggests the author was thinking about sensitive output and then opted \
             out. Review whether the task output should actually be suppressed.",
        ),
        "L017" => Some(
            "L017 — ignore_errors = true\n\
             \n\
             Silently swallowing errors leads to incomplete runs that appear \
             successful. Use `failed_when` with a specific predicate instead, or \
             handle the error explicitly in a `rescue` block.",
        ),
        "L018" => Some(
            "L018 — shell module used\n\
             \n\
             `shell` spawns a full shell interpreter and makes idempotence harder \
             to reason about. Use `runsible_builtin.command` for simple invocations \
             that don't need shell features (pipes, redirects, glob expansion).",
        ),
        "L019" => Some(
            "L019 — command module with shell metacharacters\n\
             \n\
             The `command` module does not invoke a shell. A `cmd` string containing \
             `|`, `>`, `<`, `&&`, or `||` will be passed literally to execve and will \
             not behave as expected. Switch to `runsible_builtin.shell` (and note L018) \
             or restructure to avoid shell features.",
        ),
        "L020" => Some(
            "L020 — hosts = \"all\" without --limit guard annotation\n\
             \n\
             Running against all hosts in an inventory is risky in production. \
             Add a comment `# limit: <pattern>` near the play, or pass `--limit` \
             at the CLI, to document intentional scope.",
        ),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Output formatters
// ---------------------------------------------------------------------------

fn print_text(results: &[LintResult]) {
    for result in results {
        for finding in &result.findings {
            let line_str = finding
                .line
                .map(|l| format!("{}", l))
                .unwrap_or_else(|| "-".to_owned());
            let sev = finding.severity.to_string().to_uppercase();
            println!(
                "{}:{}: [{}] {} {}",
                result.path.display(),
                line_str,
                sev,
                finding.rule_id,
                finding.description
            );
        }
    }
}

fn print_json(results: &[LintResult]) {
    // Flatten into a single JSON array of finding objects each carrying the path.
    #[derive(serde::Serialize)]
    struct JsonFinding<'a> {
        path: &'a str,
        rule_id: &'a str,
        description: &'a str,
        severity: &'a Severity,
        line: Option<usize>,
        context: Option<&'a str>,
    }

    let flat: Vec<JsonFinding> = results
        .iter()
        .flat_map(|r| {
            r.findings.iter().map(move |f| JsonFinding {
                path: r.path.to_str().unwrap_or(""),
                rule_id: &f.rule_id,
                description: &f.description,
                severity: &f.severity,
                line: f.line,
                context: f.context.as_deref(),
            })
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&flat).unwrap_or_default());
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let cli = Cli::parse();

    // --list-rules
    if cli.list_rules {
        let rules = list_rules();
        println!("{:<6} {:<12} {:<12} {}", "ID", "SEVERITY", "PROFILE", "DESCRIPTION");
        println!("{}", "-".repeat(80));
        for r in rules {
            println!(
                "{:<6} {:<12} {:<12} {}",
                r.id,
                r.severity.to_string(),
                r.profile.to_string(),
                r.description
            );
        }
        process::exit(0);
    }

    // --explain <rule>
    if let Some(rule_id) = &cli.explain {
        let id_upper = rule_id.to_uppercase();
        match explain_text(&id_upper) {
            Some(text) => {
                println!("{text}");
                process::exit(0);
            }
            None => {
                // Try to find it in the catalog at least
                let rules = list_rules();
                if let Some(r) = rules.iter().find(|r| r.id.to_uppercase() == id_upper) {
                    println!("{} — {}", r.id, r.description);
                    println!("Profile: {}  Severity: {}", r.profile, r.severity);
                } else {
                    eprintln!("runsible-lint: unknown rule ID: {rule_id}");
                    process::exit(1);
                }
                process::exit(0);
            }
        }
    }

    if cli.files.is_empty() {
        eprintln!("runsible-lint: no files specified");
        process::exit(0);
    }

    // Build LintConfig from discovered config + CLI overrides.
    let mut cfg = if let Some(first_file) = cli.files.first() {
        discover_lint_config(first_file)
    } else {
        LintConfig::default()
    };

    // CLI --profile overrides config file.
    if let Some(p) = &cli.profile {
        match p.parse::<Profile>() {
            Ok(profile) => cfg.profile = profile,
            Err(e) => {
                eprintln!("runsible-lint: {e}");
                process::exit(5);
            }
        }
    }

    // CLI --skip-rules overrides config file.
    if let Some(skip) = &cli.skip_rules {
        let extra: Vec<String> = skip
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        cfg.skip_rules.extend(extra);
    }

    // Lint each file.
    let results: Vec<LintResult> = cli
        .files
        .iter()
        .map(|p| lint_file(p, &cfg))
        .collect();

    // Output
    match cli.format.as_str() {
        "json" => print_json(&results),
        _ => print_text(&results),
    }

    // Exit code: 1 if any findings at or above threshold.
    let threshold = if cli.strict { Severity::Warning } else { Severity::Error };
    let has_findings = results
        .iter()
        .any(|r| r.findings.iter().any(|f| f.severity >= threshold));

    if has_findings {
        process::exit(1);
    } else {
        process::exit(0);
    }
}
