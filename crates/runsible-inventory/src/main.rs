// SPDX-License-Identifier: Unlicense
// Contributors: Cochran Block
//! runsible-inventory CLI — Ansible-compatible dynamic-inventory emitter.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use runsible_inventory::{
    hosts_matching, merge_inventories, parse_inventory, parse_pattern, to_ansible_host_json,
    to_ansible_list_json, Inventory,
};

#[derive(Parser, Debug)]
#[command(
    name = "runsible-inventory",
    about = "TOML-native dynamic inventory for runsible / Ansible",
    version
)]
struct Cli {
    /// Inventory file(s) to load (repeatable).
    #[arg(short = 'i', long = "inventory", value_name = "PATH")]
    inventory: Vec<PathBuf>,

    /// Limit output to hosts matching this pattern.
    #[arg(short = 'l', long = "limit", value_name = "PATTERN")]
    limit: Option<String>,

    /// Output full inventory in Ansible dynamic-inventory JSON format.
    #[arg(long = "list", conflicts_with = "host")]
    list: bool,

    /// Output merged vars for a single host.
    #[arg(long = "host", value_name = "HOSTNAME", conflicts_with = "list")]
    host: Option<String>,
}

fn load_inventories(paths: &[PathBuf]) -> Result<Inventory> {
    if paths.is_empty() {
        anyhow::bail!("No inventory file specified. Use -i/--inventory <path>.");
    }

    let mut merged: Option<Inventory> = None;

    for path in paths {
        let src = std::fs::read_to_string(path)
            .with_context(|| format!("reading inventory file {}", path.display()))?;
        let inv = parse_inventory(&src)
            .with_context(|| format!("parsing inventory file {}", path.display()))?;
        merged = Some(match merged {
            None => inv,
            Some(base) => merge_inventories(base, inv)
                .with_context(|| format!("merging inventory file {}", path.display()))?,
        });
    }

    Ok(merged.expect("loop ran at least once"))
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut inv = load_inventories(&cli.inventory)?;

    // Apply --limit: filter inventory to only the matching hosts.
    if let Some(limit_pat) = &cli.limit {
        let pattern =
            parse_pattern(limit_pat).with_context(|| format!("parsing limit '{limit_pat}'"))?;
        let allowed: std::collections::HashSet<String> =
            hosts_matching(&inv, &pattern).into_iter().collect();

        // Remove hosts not in the limit set.
        inv.hosts.retain(|name, _| allowed.contains(name));

        // Prune groups.
        for grp in inv.groups.values_mut() {
            grp.hosts.retain(|h| allowed.contains(h));
        }
    }

    if cli.list {
        let json = to_ansible_list_json(&inv);
        println!("{}", serde_json::to_string_pretty(&json)?);
        return Ok(());
    }

    if let Some(host_name) = &cli.host {
        let json = to_ansible_host_json(&inv, host_name);
        println!("{}", serde_json::to_string_pretty(&json)?);
        return Ok(());
    }

    // Neither --list nor --host given: print usage hint.
    eprintln!("Specify --list or --host <name>. Run with --help for usage.");
    std::process::exit(1);
}
