use std::process;

use clap::Parser;

use runsible_playbook::ast::{BLOCK_SENTINEL, INCLUDE_SENTINEL};
use runsible_playbook::engine::{run_with, RunOptions};
use runsible_playbook::parse::{parse_playbook, resolve_task};

#[derive(Parser)]
#[command(
    name = "runsible-playbook",
    about = "Run a TOML playbook against an inventory",
    version
)]
struct Cli {
    /// Playbook file to run.
    playbook: String,

    /// Inventory: a file path, an inline host list (`host1,host2,`), or a
    /// single hostname. Repeatable; multiple inventories are merged.
    #[arg(short = 'i', long = "inventory", required = true)]
    inventory: Vec<String>,

    /// Only run plays and tasks tagged with these tags (comma-separated, repeatable).
    #[arg(long, value_delimiter = ',')]
    tags: Vec<String>,

    /// Skip plays and tasks tagged with these tags (comma-separated, repeatable).
    #[arg(long, value_delimiter = ',')]
    skip_tags: Vec<String>,

    /// Set extra variables: `-e key=value` or `-e '{"key":"value"}'`. Repeatable.
    #[arg(short = 'e', long = "extra-vars")]
    extra_vars: Vec<String>,

    /// Dry-run: run plan() but skip apply() for mutating modules. Safe modules
    /// (`debug`, `ping`, `set_fact`, `assert`) still execute so vars/asserts
    /// work as expected.
    #[arg(short = 'C', long = "check")]
    check_mode: bool,

    /// Show before/after diff for mutating modules. Often combined with `--check`.
    #[arg(short = 'D', long = "diff")]
    diff_mode: bool,

    /// Maximum number of hosts to run in parallel within each play. Defaults
    /// to 1 (sequential) so event ordering stays deterministic; bump higher
    /// to fan out across hosts.
    #[arg(short = 'f', long = "forks", default_value = "1")]
    forks: usize,

    /// Print the list of tasks that would run for each play (does not execute).
    #[arg(long = "list-tasks")]
    list_tasks: bool,

    /// Print the list of hosts each play would target (does not execute).
    #[arg(long = "list-hosts")]
    list_hosts: bool,

    /// Parse + type-check the playbook only; do not execute. Exits non-zero on
    /// any parse error.
    #[arg(long = "syntax-check")]
    syntax_check: bool,

    /// Skip tasks until the named task is encountered (matched by name). All
    /// previous tasks become Skipped.
    #[arg(long = "start-at-task")]
    start_at_task: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    let src = match std::fs::read_to_string(&cli.playbook) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading {:?}: {e}", cli.playbook);
            process::exit(1);
        }
    };

    // Merge all -i specs into a comma-joined inline list for now.
    let inv_spec = cli.inventory.join(",") + ",";

    // --syntax-check: parse + type-check every task; never invoke the engine.
    if cli.syntax_check {
        let pb = match parse_playbook(&src) {
            Ok(pb) => pb,
            Err(e) => {
                eprintln!("syntax error: {e}");
                process::exit(4);
            }
        };
        // Walk every task to surface "exactly one module key" / TypeCheck errs.
        for play in &pb.plays {
            let groups: [&Vec<toml::Value>; 4] = [
                &play.pre_tasks,
                &play.tasks,
                &play.post_tasks,
                &Vec::new(),
            ];
            for group in groups {
                for raw in group {
                    if let Err(e) = resolve_task(raw, &pb.imports) {
                        eprintln!("syntax error in play {:?}: {e}", play.name);
                        process::exit(4);
                    }
                }
            }
            for (id, raw) in &play.handlers {
                if let Err(e) = resolve_task(raw, &pb.imports) {
                    eprintln!(
                        "syntax error in handler '{}' of play {:?}: {e}",
                        id, play.name
                    );
                    process::exit(4);
                }
            }
        }
        println!("syntax check ok: {}", cli.playbook);
        process::exit(0);
    }

    // --list-tasks: parse, then print the task list per play. No execution.
    if cli.list_tasks {
        let pb = match parse_playbook(&src) {
            Ok(pb) => pb,
            Err(e) => {
                eprintln!("parse error: {e}");
                process::exit(4);
            }
        };
        for play in &pb.plays {
            println!("play: {}", play.name);
            let mut idx: usize = 0;
            // pre_tasks
            for raw in &play.pre_tasks {
                print_raw_task(raw, &pb.imports, idx, 1, None);
                idx += 1;
            }
            // role tasks (load each role to know its task list)
            let role_search: Vec<std::path::PathBuf> =
                runsible_playbook::roles::default_search_paths();
            for role_ref in &play.roles {
                match runsible_playbook::roles::load_role(
                    &role_ref.name,
                    &role_ref.entry_point,
                    &role_search,
                ) {
                    Ok(loaded) => {
                        for raw in &loaded.tasks {
                            print_raw_task(raw, &pb.imports, idx, 1, Some(&role_ref.name));
                            idx += 1;
                        }
                    }
                    Err(e) => {
                        eprintln!("warning: could not load role '{}': {e}", role_ref.name);
                    }
                }
            }
            // tasks
            for raw in &play.tasks {
                print_raw_task(raw, &pb.imports, idx, 1, None);
                idx += 1;
            }
            // post_tasks
            for raw in &play.post_tasks {
                print_raw_task(raw, &pb.imports, idx, 1, None);
                idx += 1;
            }
        }
        process::exit(0);
    }

    // --list-hosts: parse + resolve inventory, print each play's matched hosts.
    if cli.list_hosts {
        let pb = match parse_playbook(&src) {
            Ok(pb) => pb,
            Err(e) => {
                eprintln!("parse error: {e}");
                process::exit(4);
            }
        };
        let hosts = match runsible_playbook::engine::resolve_inventory(&inv_spec) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("inventory error: {e}");
                process::exit(1);
            }
        };
        for play in &pb.plays {
            let pattern = play.hosts.to_pattern();
            let matched: Vec<&str> = hosts
                .iter()
                .filter(|h| runsible_playbook::engine::pattern_matches(&pattern, &h.name))
                .map(|h| h.name.as_str())
                .collect();
            println!("play: {}", play.name);
            println!("  pattern: {}", pattern);
            println!(
                "  hosts ({}): {}",
                matched.len(),
                matched.join(" ")
            );
        }
        process::exit(0);
    }

    let mut extra_vars = runsible_core::types::Vars::new();
    for raw in &cli.extra_vars {
        if let Some(eq) = raw.find('=') {
            let (k, v) = raw.split_at(eq);
            extra_vars.insert(k.into(), toml::Value::String(v[1..].into()));
        } else if raw.starts_with('{') {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
                if let Some(obj) = v.as_object() {
                    for (k, jv) in obj {
                        let s = serde_json::to_string(jv).unwrap_or_default();
                        if let Ok(tv) = toml::from_str::<toml::Value>(&format!("v = {s}")) {
                            if let Some(val) = tv.get("v") {
                                extra_vars.insert(k.clone(), val.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    let opts = RunOptions {
        tags: cli.tags,
        skip_tags: cli.skip_tags,
        extra_vars,
        role_search_paths: None,
        check_mode: cli.check_mode,
        diff_mode: cli.diff_mode,
        forks: cli.forks.max(1),
        start_at_task: cli.start_at_task,
    };

    match run_with(&src, &inv_spec, &cli.playbook, opts) {
        Ok(result) => process::exit(result.exit_code()),
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

/// Print a single task entry for `--list-tasks`. For block tasks, recurse into
/// children with deeper indentation. For include/import_tasks, label the entry
/// with its include sentinel so the user sees exactly what's wired.
fn print_raw_task(
    raw: &toml::Value,
    imports: &indexmap::IndexMap<String, String>,
    idx: usize,
    depth: usize,
    role_prefix: Option<&str>,
) {
    let indent = "  ".repeat(depth);
    let prefix = match role_prefix {
        Some(name) => format!("(role: {}) ", name),
        None => String::new(),
    };
    match resolve_task(raw, imports) {
        Ok(task) => {
            let label = task
                .name
                .clone()
                .unwrap_or_else(|| task.module_name.clone());
            if task.module_name == BLOCK_SENTINEL {
                println!("{indent}{prefix}task[{idx}]: {label} (block)");
                for (i, child) in task.block.iter().enumerate() {
                    print_raw_task(child, imports, i, depth + 1, None);
                }
                if !task.rescue.is_empty() {
                    println!("{indent}  rescue:");
                    for (i, child) in task.rescue.iter().enumerate() {
                        print_raw_task(child, imports, i, depth + 2, None);
                    }
                }
                if !task.always.is_empty() {
                    println!("{indent}  always:");
                    for (i, child) in task.always.iter().enumerate() {
                        print_raw_task(child, imports, i, depth + 2, None);
                    }
                }
            } else if task.module_name == INCLUDE_SENTINEL {
                let path = task.args.as_str().unwrap_or("");
                println!("{indent}{prefix}task[{idx}]: {label} (include_tasks: {path})");
            } else {
                println!("{indent}{prefix}task[{idx}]: {label}");
            }
        }
        Err(e) => {
            println!("{indent}{prefix}task[{idx}]: <unresolvable: {e}>");
        }
    }
}
