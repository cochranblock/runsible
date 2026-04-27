use std::process;

use clap::Parser;

use runsible_playbook::engine::{run_with, RunOptions};

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

    /// Dry-run: plan only, do not apply. (M1)
    #[arg(long = "check")]
    check_mode: bool,
}

fn main() {
    let cli = Cli::parse();

    if cli.check_mode {
        eprintln!("note: --check mode is not yet implemented (M1)");
        process::exit(2);
    }

    let src = match std::fs::read_to_string(&cli.playbook) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading {:?}: {e}", cli.playbook);
            process::exit(1);
        }
    };

    // Merge all -i specs into a comma-joined inline list for now.
    let inv_spec = cli.inventory.join(",") + ",";

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
    };

    match run_with(&src, &inv_spec, &cli.playbook, opts) {
        Ok(result) => process::exit(result.exit_code()),
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}
