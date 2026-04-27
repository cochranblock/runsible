// SPDX-License-Identifier: Unlicense
// Contributors: Cochran Block
//! runsible-inventory CLI — Ansible-compatible dynamic-inventory emitter.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use runsible_inventory::{
    hosts_matching, merge_inventories, parse_inventory_from_ini, parse_inventory_from_yaml,
    parse_inventory_with_dirs, parse_pattern, to_ansible_host_json, to_ansible_list_json,
    Inventory,
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

/// Resolve a `--inventory <PATH>` argument to a (file, inventory_dir) pair.
///
/// * If `path` is a directory, look for `<path>/hosts.toml` or
///   `<path>/inventory.toml` (in that order) as the canonical inventory
///   document, and use `<path>` itself as the `inventory_dir` for
///   `group_vars/` and `host_vars/` scanning.
/// * If `path` is a file, treat it as the inventory document and use
///   its parent directory (if any) as `inventory_dir`.
fn resolve_inventory_path(path: &Path) -> Result<(PathBuf, Option<PathBuf>)> {
    if path.is_dir() {
        for candidate in ["hosts.toml", "inventory.toml"] {
            let cand = path.join(candidate);
            if cand.is_file() {
                return Ok((cand, Some(path.to_path_buf())));
            }
        }
        anyhow::bail!(
            "inventory directory {} does not contain hosts.toml or inventory.toml",
            path.display()
        );
    }
    let dir = path.parent().map(|p| p.to_path_buf()).filter(|p| !p.as_os_str().is_empty());
    Ok((path.to_path_buf(), dir))
}

fn load_inventories(paths: &[PathBuf]) -> Result<Inventory> {
    if paths.is_empty() {
        anyhow::bail!("No inventory file specified. Use -i/--inventory <path>.");
    }

    let mut merged: Option<Inventory> = None;

    for raw_path in paths {
        let (path, inv_dir) = resolve_inventory_path(raw_path)?;

        let src = std::fs::read_to_string(&path)
            .with_context(|| format!("reading inventory file {}", path.display()))?;

        // Dispatch by file extension. YAML and INI route through their
        // dedicated parsers; everything else (TOML or no extension) goes
        // to the canonical TOML parser via `parse_inventory_with_dirs`,
        // which also picks up `group_vars/` and `host_vars/` next to the
        // inventory file.
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase());
        let inv = match ext.as_deref() {
            Some("yaml") | Some("yml") => parse_inventory_from_yaml(&src)
                .with_context(|| format!("parsing YAML inventory file {}", path.display()))?,
            Some("ini") => parse_inventory_from_ini(&src)
                .with_context(|| format!("parsing INI inventory file {}", path.display()))?,
            _ => parse_inventory_with_dirs(&src, inv_dir.as_deref())
                .with_context(|| format!("parsing inventory file {}", path.display()))?,
        };

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
