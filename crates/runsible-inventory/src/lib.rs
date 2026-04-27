// SPDX-License-Identifier: Unlicense
// Contributors: Cochran Block
//! runsible-inventory — TOML-native inventory parser, pattern matcher, and
//! Ansible-compatible dynamic-inventory emitter.

use std::collections::BTreeMap;
use std::path::Path;

use indexmap::IndexMap;
use runsible_core::types::{GroupName, HostName, Vars};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use yaml2toml::{convert as yaml2toml_convert, ConvertError as Yaml2TomlError, Profile};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum InventoryError {
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("invalid range pattern '{pattern}': {message}")]
    BadRange { pattern: String, message: String },

    #[error("invalid glob '{pattern}': {source}")]
    BadGlob {
        pattern: String,
        #[source]
        source: globset::Error,
    },

    #[error("invalid regex '{pattern}': {source}")]
    BadRegex {
        pattern: String,
        #[source]
        source: regex::Error,
    },

    #[error("inventory merge conflict: group '{group}' defined in multiple files")]
    MergeConflictGroup { group: String },

    #[error("unknown child group '{child}' referenced by '{parent}'")]
    UnknownChild { child: String, parent: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML inventory conversion error: {0}")]
    YamlConvert(#[from] Yaml2TomlError),

    #[error("INI parse error at line {line}: {message}")]
    IniParse { line: usize, message: String },
}

pub type Result<T> = std::result::Result<T, InventoryError>;

// ---------------------------------------------------------------------------
// Inventory types
// ---------------------------------------------------------------------------

/// Per-host entry inside the inventory (after range expansion).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostEntry {
    /// Inline vars declared in the inventory file for this host.
    pub vars: Vars,
    /// Groups this host belongs to (populated during post-processing).
    #[serde(skip)]
    pub groups: Vec<GroupName>,
}

/// Per-group entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GroupEntry {
    pub vars: Vars,
    pub hosts: Vec<HostName>,
    pub children: Vec<GroupName>,
}

/// The parsed, fully-expanded inventory.
#[derive(Debug, Clone, Default)]
pub struct Inventory {
    pub hosts: IndexMap<HostName, HostEntry>,
    pub groups: IndexMap<GroupName, GroupEntry>,
}

impl Inventory {
    /// Create an empty inventory pre-seeded with `all` and `ungrouped`.
    fn new_empty() -> Self {
        let mut inv = Inventory::default();
        inv.groups.insert("all".to_string(), GroupEntry::default());
        inv.groups
            .insert("ungrouped".to_string(), GroupEntry::default());
        inv
    }

    /// Return all host names that are *directly or transitively* members of a
    /// named group (walks the `children` tree, cycle-safe).
    pub fn hosts_in_group(&self, group: &str) -> Vec<HostName> {
        let mut result = Vec::new();
        let mut visited = std::collections::HashSet::new();
        self.collect_group_hosts(group, &mut result, &mut visited);
        result.sort();
        result.dedup();
        result
    }

    fn collect_group_hosts(
        &self,
        group: &str,
        out: &mut Vec<HostName>,
        visited: &mut std::collections::HashSet<String>,
    ) {
        if !visited.insert(group.to_string()) {
            return;
        }
        if let Some(g) = self.groups.get(group) {
            out.extend(g.hosts.clone());
            for child in &g.children.clone() {
                self.collect_group_hosts(child, out, visited);
            }
        }
    }

    /// Merge vars: group vars (all first, then specific groups), then host
    /// inline vars. Returns a flat `Vars` map for the given host.
    pub fn merged_vars_for(&self, host: &str) -> Vars {
        let mut merged: Vars = BTreeMap::new();

        // Start with `all` group vars.
        if let Some(all) = self.groups.get("all") {
            merged.extend(all.vars.clone());
        }

        // Then per-group vars for every group the host belongs to.
        if let Some(entry) = self.hosts.get(host) {
            for grp in &entry.groups {
                if grp == "all" {
                    continue;
                }
                if let Some(g) = self.groups.get(grp) {
                    merged.extend(g.vars.clone());
                }
            }
            // Finally host inline vars override everything.
            merged.extend(entry.vars.clone());
        }

        merged
    }
}

// ---------------------------------------------------------------------------
// Range expansion
// ---------------------------------------------------------------------------

/// Expand a host key that may contain a `[start:end]` range suffix.
///
/// Numeric ranges (zero-padded to the width of `start`):  `web[01:20]`
/// Alpha ranges (single char):                             `redis-[a:c]`
///
/// If the key has no `[…]` notation, returns `vec![key.to_string()]`.
pub fn expand_range(key: &str) -> Result<Vec<String>> {
    if let Some(open) = key.find('[') {
        let close = key.find(']').ok_or_else(|| InventoryError::BadRange {
            pattern: key.to_string(),
            message: "opening '[' without closing ']'".to_string(),
        })?;

        let prefix = &key[..open];
        let suffix = &key[close + 1..];
        let inner = &key[open + 1..close];

        let colon = inner.find(':').ok_or_else(|| InventoryError::BadRange {
            pattern: key.to_string(),
            message: "range must contain ':'".to_string(),
        })?;

        let start_str = &inner[..colon];
        let end_str = &inner[colon + 1..];

        // Alpha range: both sides are single ASCII letters.
        if start_str.len() == 1
            && end_str.len() == 1
            && start_str.chars().all(|c| c.is_ascii_alphabetic())
            && end_str.chars().all(|c| c.is_ascii_alphabetic())
        {
            let s = start_str.chars().next().unwrap();
            let e = end_str.chars().next().unwrap();
            if s > e {
                return Err(InventoryError::BadRange {
                    pattern: key.to_string(),
                    message: format!("start '{s}' > end '{e}'"),
                });
            }
            return Ok((s..=e)
                .map(|c| format!("{prefix}{c}{suffix}"))
                .collect());
        }

        // Numeric range (possibly zero-padded).
        let pad = start_str.len();
        let start: u64 = start_str.parse().map_err(|_| InventoryError::BadRange {
            pattern: key.to_string(),
            message: format!("'{start_str}' is not a valid integer"),
        })?;
        let end: u64 = end_str.parse().map_err(|_| InventoryError::BadRange {
            pattern: key.to_string(),
            message: format!("'{end_str}' is not a valid integer"),
        })?;

        if start > end {
            return Err(InventoryError::BadRange {
                pattern: key.to_string(),
                message: format!("start {start} > end {end}"),
            });
        }

        return Ok((start..=end)
            .map(|n| format!("{prefix}{n:0>pad$}{suffix}"))
            .collect());
    }

    Ok(vec![key.to_string()])
}

// ---------------------------------------------------------------------------
// TOML inventory parser
// ---------------------------------------------------------------------------

/// Parse a TOML inventory document from a string.
pub fn parse_inventory(src: &str) -> Result<Inventory> {
    let value: toml::Value = src.parse()?;

    let table = match value.as_table() {
        Some(t) => t,
        None => return Ok(Inventory::new_empty()),
    };

    let mut inv = Inventory::new_empty();

    for (group_name, group_val) in table {
        let group_table = match group_val.as_table() {
            Some(t) => t,
            None => continue,
        };

        let entry = inv
            .groups
            .entry(group_name.clone())
            .or_insert_with(GroupEntry::default);

        // [group.vars]
        if let Some(vars_val) = group_table.get("vars") {
            if let Some(vars_table) = vars_val.as_table() {
                for (k, v) in vars_table {
                    entry.vars.insert(k.clone(), v.clone());
                }
            }
        }

        // [group.hosts]
        if let Some(hosts_val) = group_table.get("hosts") {
            if let Some(hosts_table) = hosts_val.as_table() {
                for (host_key, host_vars_val) in hosts_table {
                    let expanded = expand_range(host_key)?;
                    for host_name in expanded {
                        let host_entry =
                            inv.hosts.entry(host_name.clone()).or_insert_with(HostEntry::default);

                        // Merge inline host vars (last-write-wins across groups).
                        if let Some(hv_table) = host_vars_val.as_table() {
                            for (k, v) in hv_table {
                                host_entry.vars.insert(k.clone(), v.clone());
                            }
                        }

                        entry.hosts.push(host_name);
                    }
                }
            }
        }

        // children = [...]
        if let Some(children_val) = group_table.get("children") {
            if let Some(arr) = children_val.as_array() {
                let children: Vec<String> = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                entry.children = children;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Post-processing: validate child references, populate host.groups, and
    // ensure `all` contains every host while `ungrouped` collects hosts that
    // are not in any named group.
    // -----------------------------------------------------------------------

    // Validate that every `children = [...]` entry references a known group.
    let known_groups: std::collections::HashSet<String> = inv.groups.keys().cloned().collect();
    for (parent, gentry) in &inv.groups {
        for child in &gentry.children {
            if !known_groups.contains(child) {
                return Err(InventoryError::UnknownChild {
                    child: child.clone(),
                    parent: parent.clone(),
                });
            }
        }
    }

    // Collect each host's group memberships (direct, not transitive).
    let group_names: Vec<String> = inv.groups.keys().cloned().collect();
    for g_name in &group_names {
        let host_list: Vec<String> = inv
            .groups
            .get(g_name)
            .map(|g| g.hosts.clone())
            .unwrap_or_default();
        for h in host_list {
            if let Some(he) = inv.hosts.get_mut(&h) {
                if !he.groups.contains(g_name) {
                    he.groups.push(g_name.clone());
                }
            }
        }
    }

    // Any host not in any explicit group (other than `all`/`ungrouped`)
    // goes into `ungrouped`.
    let ungrouped_hosts: Vec<String> = inv
        .hosts
        .iter()
        .filter(|(_, he)| {
            he.groups
                .iter()
                .all(|g| g == "all" || g == "ungrouped")
        })
        .map(|(name, _)| name.clone())
        .collect();

    for h in &ungrouped_hosts {
        let ug = inv.groups.entry("ungrouped".to_string()).or_default();
        if !ug.hosts.contains(h) {
            ug.hosts.push(h.clone());
        }
        if let Some(he) = inv.hosts.get_mut(h) {
            if !he.groups.contains(&"ungrouped".to_string()) {
                he.groups.push("ungrouped".to_string());
            }
        }
    }

    // `all` group accumulates every known host.
    let all_host_names: Vec<String> = inv.hosts.keys().cloned().collect();
    {
        let all_entry = inv.groups.entry("all".to_string()).or_default();
        for h in &all_host_names {
            if !all_entry.hosts.contains(h) {
                all_entry.hosts.push(h.clone());
            }
        }
    }
    // Add `all` to every host's group list.
    for h in &all_host_names {
        if let Some(he) = inv.hosts.get_mut(h) {
            if !he.groups.contains(&"all".to_string()) {
                he.groups.push("all".to_string());
            }
        }
    }

    Ok(inv)
}

// ---------------------------------------------------------------------------
// YAML inventory parser (routes through yaml2toml per plan §9.2)
// ---------------------------------------------------------------------------

/// Parse a YAML inventory document by transpiling through `yaml2toml`
/// (`Profile::Inventory`) and then handing the resulting TOML to
/// `parse_inventory`.
///
/// Per the runsible-inventory plan §9.2, YAML inventories are read-only and
/// imported through this conversion path. The `warnings` from yaml2toml are
/// currently discarded; M2 will surface them as engine events.
pub fn parse_inventory_from_yaml(src: &str) -> Result<Inventory> {
    let result = yaml2toml_convert(src, Profile::Inventory)?;
    parse_inventory(&result.toml)
}

// ---------------------------------------------------------------------------
// INI inventory parser (read-only per plan §9.1)
// ---------------------------------------------------------------------------

/// Parse an Ansible-style INI inventory document.
///
/// Per the runsible-inventory plan §9.1, INI inventories are read-only and
/// hand-parsed (no Python-style `literal_eval`). Inline `key=value` host vars
/// and `[group:vars]` entries are typed as TOML scalars: `port=8080` becomes
/// an integer, `proto=http` becomes a string, `enabled=true` becomes a bool.
///
/// Section forms recognised:
/// - `[group]`            — host list; each line is a host name plus optional
///                          inline `key=value` host vars.
/// - `[group:vars]`       — group var block.
/// - `[group:children]`   — child group names, one per line.
///
/// `#` and `;` start comments. Empty lines are ignored.
pub fn parse_inventory_from_ini(src: &str) -> Result<Inventory> {
    // Section kinds we track while scanning the document.
    enum SectionKind {
        Hosts(String),     // [group]
        Vars(String),      // [group:vars]
        Children(String),  // [group:children]
    }

    let mut current: Option<SectionKind> = None;

    let mut inv = Inventory::new_empty();

    for (idx, raw_line) in src.lines().enumerate() {
        let line_no = idx + 1;
        let line = strip_inline_comment(raw_line).trim();

        if line.is_empty() {
            continue;
        }

        // Section header.
        if let Some(stripped) = line.strip_prefix('[') {
            let header = stripped
                .strip_suffix(']')
                .ok_or_else(|| InventoryError::IniParse {
                    line: line_no,
                    message: format!("section header missing ']': '{raw_line}'"),
                })?
                .trim();

            if let Some((group, kind)) = header.split_once(':') {
                let group = group.trim();
                let kind = kind.trim();
                match kind {
                    "vars" => {
                        ensure_group(&mut inv, group);
                        current = Some(SectionKind::Vars(group.to_string()));
                    }
                    "children" => {
                        ensure_group(&mut inv, group);
                        current = Some(SectionKind::Children(group.to_string()));
                    }
                    other => {
                        return Err(InventoryError::IniParse {
                            line: line_no,
                            message: format!(
                                "unknown section qualifier ':{other}' (expected ':vars' or ':children')"
                            ),
                        });
                    }
                }
            } else {
                ensure_group(&mut inv, header);
                current = Some(SectionKind::Hosts(header.to_string()));
            }
            continue;
        }

        // Body lines depend on the current section kind. With no current
        // section, treat as ungrouped host list (Ansible compatibility).
        let section = match &current {
            Some(s) => s,
            None => {
                let host = line.to_string();
                let expanded = expand_range(&host)?;
                for h in expanded {
                    inv.hosts
                        .entry(h.clone())
                        .or_insert_with(HostEntry::default);
                    let g = inv.groups.entry("ungrouped".to_string()).or_default();
                    if !g.hosts.contains(&h) {
                        g.hosts.push(h);
                    }
                }
                continue;
            }
        };

        match section {
            SectionKind::Hosts(group_name) => {
                // "host01 key=val key2=val2"
                let mut parts = line.split_whitespace();
                let host_token = match parts.next() {
                    Some(t) => t,
                    None => continue,
                };
                let inline_vars: std::result::Result<Vars, InventoryError> = parts
                    .map(|tok| parse_ini_kv(tok, line_no))
                    .collect();
                let inline_vars = inline_vars?;

                let expanded = expand_range(host_token)?;
                for host_name in expanded {
                    let host_entry = inv
                        .hosts
                        .entry(host_name.clone())
                        .or_insert_with(HostEntry::default);
                    for (k, v) in &inline_vars {
                        host_entry.vars.insert(k.clone(), v.clone());
                    }
                    let g = inv.groups.entry(group_name.clone()).or_default();
                    if !g.hosts.contains(&host_name) {
                        g.hosts.push(host_name);
                    }
                }
            }
            SectionKind::Vars(group_name) => {
                let (k, v) = parse_ini_kv(line, line_no)?;
                let g = inv.groups.entry(group_name.clone()).or_default();
                g.vars.insert(k, v);
            }
            SectionKind::Children(group_name) => {
                let child = line.split_whitespace().next().unwrap_or("").to_string();
                if child.is_empty() {
                    continue;
                }
                ensure_group(&mut inv, &child);
                let g = inv.groups.entry(group_name.clone()).or_default();
                if !g.children.contains(&child) {
                    g.children.push(child);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Post-processing — mirror parse_inventory's bookkeeping.
    // -----------------------------------------------------------------------

    // Validate child references.
    let known_groups: std::collections::HashSet<String> = inv.groups.keys().cloned().collect();
    for (parent, gentry) in &inv.groups {
        for child in &gentry.children {
            if !known_groups.contains(child) {
                return Err(InventoryError::UnknownChild {
                    child: child.clone(),
                    parent: parent.clone(),
                });
            }
        }
    }

    // Direct group memberships.
    let group_names: Vec<String> = inv.groups.keys().cloned().collect();
    for g_name in &group_names {
        let host_list: Vec<String> = inv
            .groups
            .get(g_name)
            .map(|g| g.hosts.clone())
            .unwrap_or_default();
        for h in host_list {
            if let Some(he) = inv.hosts.get_mut(&h) {
                if !he.groups.contains(g_name) {
                    he.groups.push(g_name.clone());
                }
            }
        }
    }

    // Ungrouped hosts.
    let ungrouped_hosts: Vec<String> = inv
        .hosts
        .iter()
        .filter(|(_, he)| {
            he.groups
                .iter()
                .all(|g| g == "all" || g == "ungrouped")
        })
        .map(|(name, _)| name.clone())
        .collect();
    for h in &ungrouped_hosts {
        let ug = inv.groups.entry("ungrouped".to_string()).or_default();
        if !ug.hosts.contains(h) {
            ug.hosts.push(h.clone());
        }
        if let Some(he) = inv.hosts.get_mut(h) {
            if !he.groups.contains(&"ungrouped".to_string()) {
                he.groups.push("ungrouped".to_string());
            }
        }
    }

    // `all` accumulates every host.
    let all_host_names: Vec<String> = inv.hosts.keys().cloned().collect();
    {
        let all_entry = inv.groups.entry("all".to_string()).or_default();
        for h in &all_host_names {
            if !all_entry.hosts.contains(h) {
                all_entry.hosts.push(h.clone());
            }
        }
    }
    for h in &all_host_names {
        if let Some(he) = inv.hosts.get_mut(h) {
            if !he.groups.contains(&"all".to_string()) {
                he.groups.push("all".to_string());
            }
        }
    }

    Ok(inv)
}

/// Strip an inline `;` or `#` comment (ignoring chars inside quotes).
/// Quotes: a `"` toggles quoted state. A leading-of-token `#`/`;` ends parsing.
fn strip_inline_comment(line: &str) -> &str {
    let mut in_quotes = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            '#' | ';' if !in_quotes => return &line[..i],
            _ => {}
        }
    }
    line
}

/// Ensure a group with this name exists in `inv.groups` (no-op if present).
fn ensure_group(inv: &mut Inventory, name: &str) {
    inv.groups
        .entry(name.to_string())
        .or_insert_with(GroupEntry::default);
}

/// Parse a single `key=value` token (or `key = value` when split off a vars
/// line) into a `(String, toml::Value)` pair, typing the value as a TOML
/// scalar via `infer_ini_scalar`.
fn parse_ini_kv(input: &str, line_no: usize) -> Result<(String, toml::Value)> {
    let (key, val) = input
        .split_once('=')
        .ok_or_else(|| InventoryError::IniParse {
            line: line_no,
            message: format!("expected 'key=value', got '{input}'"),
        })?;
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err(InventoryError::IniParse {
            line: line_no,
            message: format!("empty key in '{input}'"),
        });
    }
    let val = val.trim();
    Ok((key, infer_ini_scalar(val)))
}

/// Infer a TOML scalar from an INI value string. Quoted strings are unquoted
/// verbatim; bare `true`/`false` become bools; integer-shaped tokens become
/// integers; float-shaped tokens become floats; everything else stays a
/// string. Per plan §3.1: TOML scalars only — no Python literal_eval.
fn infer_ini_scalar(raw: &str) -> toml::Value {
    // Quoted strings: take inner content verbatim.
    if raw.len() >= 2 {
        let bytes = raw.as_bytes();
        if (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
        {
            let inner = &raw[1..raw.len() - 1];
            return toml::Value::String(inner.to_string());
        }
    }

    match raw {
        "true" => return toml::Value::Boolean(true),
        "false" => return toml::Value::Boolean(false),
        _ => {}
    }

    if let Ok(i) = raw.parse::<i64>() {
        return toml::Value::Integer(i);
    }
    if let Ok(f) = raw.parse::<f64>() {
        if f.is_finite() {
            return toml::Value::Float(f);
        }
    }

    toml::Value::String(raw.to_string())
}

// ---------------------------------------------------------------------------
// Vars merge + group_vars/host_vars directory loader
// ---------------------------------------------------------------------------

/// Merge `from` into `into`. Keys in `from` overwrite keys in `into`.
///
/// Used as the explicit, testable building block for layering vars from
/// `group_vars/<group>.toml` and `host_vars/<host>.toml` on top of inline
/// inventory vars (Inventory precedence, level 2 of 5 per the
/// runsible-inventory plan §7.1).
pub fn merge_vars(into: &mut Vars, from: &Vars) {
    for (k, v) in from {
        into.insert(k.clone(), v.clone());
    }
}

/// Read a TOML file as a flat `Vars` table. Non-table top-level documents
/// produce an empty `Vars` (silently skipped — `group_vars`/`host_vars`
/// files are conventionally key-value tables).
fn read_vars_file(path: &Path) -> Result<Vars> {
    let src = std::fs::read_to_string(path)?;
    let value: toml::Value = src.parse()?;
    let mut vars: Vars = BTreeMap::new();
    if let Some(table) = value.as_table() {
        for (k, v) in table {
            vars.insert(k.clone(), v.clone());
        }
    }
    Ok(vars)
}

/// Collect `.toml` files under a directory, sorted lexicographically by
/// file name. Non-`.toml` entries and subdirectories are skipped.
fn collect_toml_files_sorted(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut entries: Vec<std::path::PathBuf> = Vec::new();
    for ent in std::fs::read_dir(dir)? {
        let ent = ent?;
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        entries.push(path);
    }
    entries.sort();
    Ok(entries)
}

/// Load all vars contributions for a single key under `group_vars/` or
/// `host_vars/`. The convention:
///
/// * `<base>/<key>.toml` — flat table, merged in.
/// * `<base>/<key>/*.toml` — every `.toml` file in lexicographic order,
///   each merged in (later files override earlier ones).
///
/// Both forms may coexist; the `<key>.toml` file is processed first, then
/// the directory's files. Returns an empty map if neither exists.
fn load_vars_for_key(base: &Path, key: &str) -> Result<Vars> {
    let mut acc: Vars = BTreeMap::new();
    let file_form = base.join(format!("{key}.toml"));
    if file_form.is_file() {
        let v = read_vars_file(&file_form)?;
        merge_vars(&mut acc, &v);
    }
    let dir_form = base.join(key);
    if dir_form.is_dir() {
        for f in collect_toml_files_sorted(&dir_form)? {
            let v = read_vars_file(&f)?;
            merge_vars(&mut acc, &v);
        }
    }
    Ok(acc)
}

/// Like `parse_inventory`, but also scans `inventory_dir/group_vars/` and
/// `inventory_dir/host_vars/` for per-group and per-host var files.
///
/// Layout:
/// * `group_vars/<group>.toml` or `group_vars/<group>/*.toml` (lex order,
///   later wins) → merged into `inventory.groups[<group>].vars`.
/// * `host_vars/<host>.toml` or `host_vars/<host>/*.toml` → merged into
///   `inventory.hosts[<host>].vars`.
///
/// Per the runsible-inventory plan §7.1:
/// * Files **win over inline** vars from the inventory document (more
///   specific source — the file path explicitly names the group/host).
/// * Host vars **win over group vars** unconditionally during the
///   per-host merge performed by `merged_vars_for`.
///
/// `inventory_dir = None` makes this function equivalent to
/// `parse_inventory`. A nonexistent `inventory_dir` or missing
/// `group_vars` / `host_vars` subdirectory is not an error — the
/// function silently proceeds. Files without a `.toml` extension are
/// silently skipped.
pub fn parse_inventory_with_dirs(
    src: &str,
    inventory_dir: Option<&Path>,
) -> Result<Inventory> {
    let mut inv = parse_inventory(src)?;

    let dir = match inventory_dir {
        Some(d) => d,
        None => return Ok(inv),
    };
    if !dir.exists() {
        return Ok(inv);
    }

    // group_vars/
    let gv_root = dir.join("group_vars");
    if gv_root.is_dir() {
        // Discover all group keys: anything that has a `<key>.toml` file
        // or a `<key>/` subdirectory in group_vars/. A `group_vars/foo.toml`
        // file effectively declares `foo` as a group (with no hosts/children
        // of its own) if it isn't already in the inline inventory.
        let mut keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for ent in std::fs::read_dir(&gv_root)? {
            let ent = ent?;
            let path = ent.path();
            if path.is_file() {
                if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        keys.insert(stem.to_string());
                    }
                }
            } else if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                    keys.insert(name.to_string());
                }
            }
        }

        for key in keys {
            let loaded = load_vars_for_key(&gv_root, &key)?;
            if loaded.is_empty() {
                continue;
            }
            let entry = inv.groups.entry(key.clone()).or_default();
            // File wins over inline: merge file vars on top.
            merge_vars(&mut entry.vars, &loaded);
        }
    }

    // host_vars/
    let hv_root = dir.join("host_vars");
    if hv_root.is_dir() {
        let mut keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for ent in std::fs::read_dir(&hv_root)? {
            let ent = ent?;
            let path = ent.path();
            if path.is_file() {
                if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        keys.insert(stem.to_string());
                    }
                }
            } else if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                    keys.insert(name.to_string());
                }
            }
        }

        for key in keys {
            let loaded = load_vars_for_key(&hv_root, &key)?;
            if loaded.is_empty() {
                continue;
            }
            // host_vars files declare host vars even when the host wasn't
            // mentioned in the inline inventory; create-on-demand so the
            // host shows up in the merged inventory.
            let entry = inv.hosts.entry(key.clone()).or_default();
            merge_vars(&mut entry.vars, &loaded);
        }
    }

    Ok(inv)
}

/// Merge two inventories. Hosts are unioned (host vars merged, later wins).
/// Groups must not conflict on name (returns error if the same named group
/// appears in both with different definitions — except `all`/`ungrouped`
/// which are always merged).
pub fn merge_inventories(a: Inventory, b: Inventory) -> Result<Inventory> {
    let mut result = a;

    // Merge hosts.
    for (name, entry) in b.hosts {
        let existing = result.hosts.entry(name).or_default();
        existing.vars.extend(entry.vars);
    }

    // Merge groups.
    for (name, entry) in b.groups {
        if name == "all" || name == "ungrouped" {
            let target = result.groups.entry(name).or_default();
            for h in &entry.hosts {
                if !target.hosts.contains(h) {
                    target.hosts.push(h.clone());
                }
            }
            target.vars.extend(entry.vars);
            continue;
        }
        if result.groups.contains_key(&name) {
            return Err(InventoryError::MergeConflictGroup { group: name });
        }
        result.groups.insert(name, entry);
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Pattern
// ---------------------------------------------------------------------------

/// A parsed host-selection pattern.
#[derive(Debug, Clone)]
pub enum Pattern {
    /// Every host.
    All,
    /// Exact host or group name.
    Exact(String),
    /// Shell glob against host/group names.
    Glob(String),
    /// Regex (prefixed with `~` in the source).
    Regex(String),
    /// `a:b` or `a,b` — union.
    Union(Vec<Pattern>),
    /// `a:&b` — intersection.
    Intersection(Box<Pattern>, Box<Pattern>),
    /// `a:!b` — exclusion.
    Exclusion(Box<Pattern>, Box<Pattern>),
}

/// Parse a pattern string like `web*:&prod:!staging`.
///
/// Grammar (simplified):
/// - Tokenize on `:` and `,`.
/// - Tokens prefixed with `&` = intersection operand.
/// - Tokens prefixed with `!` = exclusion operand.
/// - Otherwise union operands.
/// - `all` or `*` = Pattern::All.
/// - `~<regex>` = Pattern::Regex.
/// - Tokens containing `*` or `?` = Pattern::Glob.
/// - All others = Pattern::Exact.
pub fn parse_pattern(s: &str) -> Result<Pattern> {
    // Tokenize on `:` and `,`.
    let tokens: Vec<&str> = s.split(|c| c == ':' || c == ',').collect();

    let mut union_parts: Vec<Pattern> = Vec::new();
    let mut base: Option<Pattern> = None;

    for token in tokens {
        if token.is_empty() {
            continue;
        }

        if let Some(rest) = token.strip_prefix('&') {
            // Intersection: base & rest
            let rhs = single_token_pattern(rest)?;
            let lhs = base.take().unwrap_or(Pattern::All);
            base = Some(Pattern::Intersection(Box::new(lhs), Box::new(rhs)));
        } else if let Some(rest) = token.strip_prefix('!') {
            // Exclusion: base ! rest
            let rhs = single_token_pattern(rest)?;
            let lhs = base.take().unwrap_or(Pattern::All);
            base = Some(Pattern::Exclusion(Box::new(lhs), Box::new(rhs)));
        } else {
            // Plain union operand.
            if let Some(b) = base.take() {
                union_parts.push(b);
            }
            base = Some(single_token_pattern(token)?);
        }
    }

    if let Some(b) = base {
        union_parts.push(b);
    }

    if union_parts.is_empty() {
        return Ok(Pattern::All);
    }
    if union_parts.len() == 1 {
        return Ok(union_parts.remove(0));
    }
    Ok(Pattern::Union(union_parts))
}

fn single_token_pattern(token: &str) -> Result<Pattern> {
    if token == "all" || token == "*" {
        return Ok(Pattern::All);
    }
    if let Some(regex_src) = token.strip_prefix('~') {
        return Ok(Pattern::Regex(regex_src.to_string()));
    }
    if token.contains('*') || token.contains('?') || token.contains('[') {
        return Ok(Pattern::Glob(token.to_string()));
    }
    Ok(Pattern::Exact(token.to_string()))
}

// ---------------------------------------------------------------------------
// Pattern evaluation
// ---------------------------------------------------------------------------

/// Evaluate a pattern against an inventory and return a sorted, deduplicated
/// list of host names.
pub fn hosts_matching(inv: &Inventory, pattern: &Pattern) -> Vec<String> {
    let result = eval_pattern(inv, pattern);
    let mut v: Vec<String> = result.into_iter().collect();
    v.sort();
    v
}

fn eval_pattern(inv: &Inventory, pattern: &Pattern) -> std::collections::HashSet<String> {
    match pattern {
        Pattern::All => inv.hosts.keys().cloned().collect(),

        Pattern::Exact(name) => {
            // Could be a host name or a group name.
            if inv.hosts.contains_key(name) {
                let mut s = std::collections::HashSet::new();
                s.insert(name.clone());
                s
            } else {
                // Treat as group.
                inv.hosts_in_group(name)
                    .into_iter()
                    .collect()
            }
        }

        Pattern::Glob(pat) => {
            let mut matches = std::collections::HashSet::new();
            // Match against host names.
            if let Ok(glob) = globset::GlobBuilder::new(pat)
                .case_insensitive(false)
                .build()
                .map(|g| {
                    let mut b = globset::GlobSetBuilder::new();
                    b.add(g);
                    b.build()
                })
                .and_then(|r| r)
            {
                for host in inv.hosts.keys() {
                    if glob.is_match(host) {
                        matches.insert(host.clone());
                    }
                }
                // Also expand any groups whose names match the glob.
                for group in inv.groups.keys() {
                    if glob.is_match(group) {
                        for h in inv.hosts_in_group(group) {
                            matches.insert(h);
                        }
                    }
                }
            }
            matches
        }

        Pattern::Regex(src) => {
            let mut matches = std::collections::HashSet::new();
            if let Ok(re) = regex::Regex::new(src) {
                for host in inv.hosts.keys() {
                    if re.is_match(host) {
                        matches.insert(host.clone());
                    }
                }
                for group in inv.groups.keys() {
                    if re.is_match(group) {
                        for h in inv.hosts_in_group(group) {
                            matches.insert(h);
                        }
                    }
                }
            }
            matches
        }

        Pattern::Union(parts) => {
            let mut result = std::collections::HashSet::new();
            for p in parts {
                result.extend(eval_pattern(inv, p));
            }
            result
        }

        Pattern::Intersection(a, b) => {
            let sa = eval_pattern(inv, a);
            let sb = eval_pattern(inv, b);
            sa.intersection(&sb).cloned().collect()
        }

        Pattern::Exclusion(a, b) => {
            let sa = eval_pattern(inv, a);
            let sb = eval_pattern(inv, b);
            sa.difference(&sb).cloned().collect()
        }
    }
}

// ---------------------------------------------------------------------------
// Ansible dynamic-inventory JSON output
// ---------------------------------------------------------------------------

/// Build the Ansible-compatible `--list` JSON structure.
///
/// Schema:
/// ```json
/// {
///   "_meta": { "hostvars": { "host1": {...}, ... } },
///   "all": { "hosts": [...], "vars": {...} },
///   "groupname": { "hosts": [...], "children": [...], "vars": {...} }
/// }
/// ```
pub fn to_ansible_list_json(inv: &Inventory) -> serde_json::Value {
    let mut obj = serde_json::Map::new();

    // _meta.hostvars: one entry per host with merged vars.
    let mut hostvars = serde_json::Map::new();
    for host in inv.hosts.keys() {
        let merged = inv.merged_vars_for(host);
        let hv_json: serde_json::Map<String, serde_json::Value> = merged
            .into_iter()
            .map(|(k, v)| (k, toml_value_to_json(v)))
            .collect();
        hostvars.insert(host.clone(), serde_json::Value::Object(hv_json));
    }
    obj.insert(
        "_meta".to_string(),
        serde_json::json!({ "hostvars": hostvars }),
    );

    // One entry per group.
    for (gname, gentry) in &inv.groups {
        let mut gobj = serde_json::Map::new();
        if !gentry.hosts.is_empty() {
            gobj.insert(
                "hosts".to_string(),
                serde_json::Value::Array(
                    gentry
                        .hosts
                        .iter()
                        .map(|h| serde_json::Value::String(h.clone()))
                        .collect(),
                ),
            );
        }
        if !gentry.children.is_empty() {
            gobj.insert(
                "children".to_string(),
                serde_json::Value::Array(
                    gentry
                        .children
                        .iter()
                        .map(|c| serde_json::Value::String(c.clone()))
                        .collect(),
                ),
            );
        }
        if !gentry.vars.is_empty() {
            let vars_json: serde_json::Map<String, serde_json::Value> = gentry
                .vars
                .iter()
                .map(|(k, v)| (k.clone(), toml_value_to_json(v.clone())))
                .collect();
            gobj.insert("vars".to_string(), serde_json::Value::Object(vars_json));
        }
        obj.insert(gname.clone(), serde_json::Value::Object(gobj));
    }

    serde_json::Value::Object(obj)
}

/// Build the Ansible `--host <name>` JSON: merged vars for one host.
pub fn to_ansible_host_json(inv: &Inventory, host: &str) -> serde_json::Value {
    let merged = inv.merged_vars_for(host);
    let map: serde_json::Map<String, serde_json::Value> = merged
        .into_iter()
        .map(|(k, v)| (k, toml_value_to_json(v)))
        .collect();
    serde_json::Value::Object(map)
}

// ---------------------------------------------------------------------------
// TOML → JSON value coercion (best-effort, no vault awareness at M0)
// ---------------------------------------------------------------------------

fn toml_value_to_json(v: toml::Value) -> serde_json::Value {
    match v {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(i) => serde_json::Value::Number(i.into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(toml_value_to_json).collect())
        }
        toml::Value::Table(t) => {
            let m: serde_json::Map<String, serde_json::Value> = t
                .into_iter()
                .map(|(k, v)| (k, toml_value_to_json(v)))
                .collect();
            serde_json::Value::Object(m)
        }
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
    }
}

// ---------------------------------------------------------------------------
// f30 — TRIPLE SIMS smoke gate
// ---------------------------------------------------------------------------

/// Smoke gate: exercise the public API end-to-end. Parse a small inventory
/// (3 hosts in 2 groups + a `prod` group with `children = [...]`), run the
/// canonical pattern operators (intersection, exclusion), and verify
/// numeric range expansion. Returns 0 on success or a non-zero stage code
/// on failure. Used by the runsible-inventory-test binary's TRIPLE SIMS.
pub fn f30() -> i32 {
    const SRC: &str = r#"
[webservers.hosts]
web01 = {}
web02 = {}

[databases.hosts]
db01 = {}

[prod]
children = ["webservers", "databases"]
"#;

    // Stage 1: parse the inventory.
    let inv = match parse_inventory(SRC) {
        Ok(i) => i,
        Err(_) => return 1,
    };

    // Stage 2: parse `prod:&webservers` — must yield exactly the webservers.
    let p1 = match parse_pattern("prod:&webservers") {
        Ok(p) => p,
        Err(_) => return 2,
    };
    let r1 = hosts_matching(&inv, &p1);
    if r1 != vec!["web01".to_string(), "web02".to_string()] {
        return 3;
    }

    // Stage 3: parse `all:!databases` — must yield exactly the webservers.
    let p2 = match parse_pattern("all:!databases") {
        Ok(p) => p,
        Err(_) => return 4,
    };
    let r2 = hosts_matching(&inv, &p2);
    if r2 != vec!["web01".to_string(), "web02".to_string()] {
        return 5;
    }

    // Stage 4: numeric range expansion must yield ["web01","web02","web03"].
    let r3 = match expand_range("web[01:03]") {
        Ok(v) => v,
        Err(_) => return 6,
    };
    if r3 != vec!["web01".to_string(), "web02".to_string(), "web03".to_string()] {
        return 7;
    }

    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Range expansion
    // -----------------------------------------------------------------------

    #[test]
    fn range_expansion_numeric() {
        let result = expand_range("web[01:03]").unwrap();
        assert_eq!(result, vec!["web01", "web02", "web03"]);
    }

    #[test]
    fn range_expansion_alpha() {
        let result = expand_range("redis-[a:c]").unwrap();
        assert_eq!(result, vec!["redis-a", "redis-b", "redis-c"]);
    }

    // -----------------------------------------------------------------------
    // Parse and list
    // -----------------------------------------------------------------------

    const MINI_INV: &str = r#"
[all.vars]
ntp_pool = "pool.ntp.org"

[webservers.vars]
http_port = 8080

[webservers.hosts]
"web01" = {}
"web02" = {}
"web03" = {}

[dbservers.hosts]
"db01" = {}
"db02" = {}
"#;

    #[test]
    fn parse_and_list() {
        let inv = parse_inventory(MINI_INV).unwrap();
        let mut names: Vec<&str> = inv.hosts.keys().map(|s| s.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["db01", "db02", "web01", "web02", "web03"]);
        // All group should contain all hosts.
        let all_hosts = inv.hosts_in_group("all");
        assert_eq!(all_hosts.len(), 5);
    }

    // -----------------------------------------------------------------------
    // Pattern matching
    // -----------------------------------------------------------------------

    fn make_test_inv() -> Inventory {
        parse_inventory(MINI_INV).unwrap()
    }

    #[test]
    fn pattern_union() {
        let inv = make_test_inv();
        let p = parse_pattern("web*:db*").unwrap();
        let result = hosts_matching(&inv, &p);
        assert_eq!(
            result,
            vec!["db01", "db02", "web01", "web02", "web03"]
        );
    }

    #[test]
    fn pattern_intersection() {
        let inv = make_test_inv();
        // all:&webservers → only webservers hosts
        let p = parse_pattern("all:&webservers").unwrap();
        let result = hosts_matching(&inv, &p);
        assert_eq!(result, vec!["web01", "web02", "web03"]);
    }

    #[test]
    fn pattern_exclusion() {
        let inv = make_test_inv();
        // all:!webservers → every host that is NOT in webservers
        let p = parse_pattern("all:!webservers").unwrap();
        let result = hosts_matching(&inv, &p);
        assert_eq!(result, vec!["db01", "db02"]);
    }

    // -----------------------------------------------------------------------
    // Range expansion — extended coverage
    // -----------------------------------------------------------------------

    #[test]
    fn range_no_brackets_passthrough() {
        let result = expand_range("web").unwrap();
        assert_eq!(result, vec!["web"]);
    }

    #[test]
    fn range_single_element_padded() {
        let result = expand_range("web[01:01]").unwrap();
        assert_eq!(result, vec!["web01"]);
    }

    #[test]
    fn range_no_padding_when_unpadded() {
        let result = expand_range("web[1:3]").unwrap();
        assert_eq!(result, vec!["web1", "web2", "web3"]);
    }

    #[test]
    fn range_three_digit_padding_preserved() {
        let result = expand_range("web[001:003]").unwrap();
        assert_eq!(result, vec!["web001", "web002", "web003"]);
    }

    #[test]
    fn range_uppercase_alpha() {
        let result = expand_range("redis-[A:C]").unwrap();
        assert_eq!(result, vec!["redis-A", "redis-B", "redis-C"]);
    }

    #[test]
    fn range_mixed_case_alpha_invalid() {
        // 'a' (97) > 'Z' (90) so this fails the descending-range guard.
        let err = expand_range("bad[a:Z]").unwrap_err();
        assert!(
            matches!(err, InventoryError::BadRange { .. }),
            "expected BadRange, got {err:?}"
        );
    }

    #[test]
    fn range_descending_numeric_invalid() {
        let err = expand_range("nope[3:1]").unwrap_err();
        assert!(
            matches!(err, InventoryError::BadRange { .. }),
            "expected BadRange, got {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Inventory parse — extended coverage
    // -----------------------------------------------------------------------

    #[test]
    fn parse_empty_string() {
        let inv = parse_inventory("").unwrap();
        assert_eq!(inv.hosts.len(), 0);
        assert!(inv.groups.contains_key("all"));
        assert!(inv.groups.contains_key("ungrouped"));
    }

    #[test]
    fn parse_only_all_vars() {
        let src = r#"
[all.vars]
foo = "bar"
ntp_pool = "pool.ntp.org"
"#;
        let inv = parse_inventory(src).unwrap();
        assert_eq!(inv.hosts.len(), 0);
        let all = inv.groups.get("all").expect("all group present");
        assert_eq!(all.vars.len(), 2);
        assert_eq!(
            all.vars.get("foo").and_then(|v| v.as_str()),
            Some("bar")
        );
    }

    #[test]
    fn parse_unknown_child_errors() {
        let src = r#"
[webservers.hosts]
web01 = {}

[prod]
children = ["nonexistent"]
"#;
        let err = parse_inventory(src).unwrap_err();
        match err {
            InventoryError::UnknownChild { child, parent } => {
                assert_eq!(child, "nonexistent");
                assert_eq!(parent, "prod");
            }
            other => panic!("expected UnknownChild, got {other:?}"),
        }
    }

    #[test]
    fn parse_host_in_two_groups_merges_vars() {
        let src = r#"
[group1.hosts]
shared01 = { port = 80 }

[group2.hosts]
shared01 = { proto = "http" }
"#;
        let inv = parse_inventory(src).unwrap();
        let host = inv.hosts.get("shared01").expect("shared01 present");
        assert_eq!(
            host.vars.get("port").and_then(|v| v.as_integer()),
            Some(80)
        );
        assert_eq!(
            host.vars.get("proto").and_then(|v| v.as_str()),
            Some("http")
        );
        // Should be a member of both named groups.
        assert!(host.groups.contains(&"group1".to_string()));
        assert!(host.groups.contains(&"group2".to_string()));
    }

    // -----------------------------------------------------------------------
    // Pattern matching — extended coverage with shared fixture
    // -----------------------------------------------------------------------

    fn fixture() -> Inventory {
        parse_inventory(
            r#"
[all.hosts]
web01 = {}
web02 = {}
db01 = {}
db02 = {}

[webservers.hosts]
web01 = {}
web02 = {}

[databases.hosts]
db01 = {}
db02 = {}

[prod]
children = ["webservers", "databases"]
"#,
        )
        .unwrap()
    }

    #[test]
    fn pattern_all_keyword() {
        let inv = fixture();
        let p = parse_pattern("all").unwrap();
        let result = hosts_matching(&inv, &p);
        assert_eq!(result, vec!["db01", "db02", "web01", "web02"]);
    }

    #[test]
    fn pattern_group_name_exact() {
        let inv = fixture();
        let p = parse_pattern("webservers").unwrap();
        let result = hosts_matching(&inv, &p);
        assert_eq!(result, vec!["web01", "web02"]);
    }

    #[test]
    fn pattern_glob_web_star() {
        let inv = fixture();
        let p = parse_pattern("web*").unwrap();
        let result = hosts_matching(&inv, &p);
        assert_eq!(result, vec!["web01", "web02"]);
    }

    #[test]
    fn pattern_regex_anchored_class() {
        let inv = fixture();
        let p = parse_pattern("~web0[12]").unwrap();
        let result = hosts_matching(&inv, &p);
        assert_eq!(result, vec!["web01", "web02"]);
    }

    #[test]
    fn pattern_intersection_prod_and_webservers() {
        let inv = fixture();
        let p = parse_pattern("prod:&webservers").unwrap();
        let result = hosts_matching(&inv, &p);
        assert_eq!(result, vec!["web01", "web02"]);
    }

    #[test]
    fn pattern_exclusion_all_minus_databases() {
        let inv = fixture();
        let p = parse_pattern("all:!databases").unwrap();
        let result = hosts_matching(&inv, &p);
        assert_eq!(result, vec!["web01", "web02"]);
    }

    #[test]
    fn pattern_comma_separated_union() {
        let inv = fixture();
        let p = parse_pattern("web01,db01").unwrap();
        let result = hosts_matching(&inv, &p);
        assert_eq!(result, vec!["db01", "web01"]);
    }

    // -----------------------------------------------------------------------
    // JSON emitters
    // -----------------------------------------------------------------------

    #[test]
    fn json_list_meta_hostvars_per_host() {
        let inv = make_test_inv();
        let json = to_ansible_list_json(&inv);
        let hostvars = json
            .get("_meta")
            .and_then(|m| m.get("hostvars"))
            .and_then(|h| h.as_object())
            .expect("_meta.hostvars present");

        // Every host in the inventory should appear in hostvars.
        for host in inv.hosts.keys() {
            assert!(
                hostvars.contains_key(host),
                "hostvars missing entry for {host}"
            );
        }
        assert_eq!(hostvars.len(), inv.hosts.len());
    }

    #[test]
    fn json_list_group_has_hosts_and_vars() {
        let inv = make_test_inv();
        let json = to_ansible_list_json(&inv);
        let webservers = json
            .get("webservers")
            .and_then(|g| g.as_object())
            .expect("webservers group in output");

        let hosts = webservers
            .get("hosts")
            .and_then(|h| h.as_array())
            .expect("webservers.hosts array");
        let host_strs: Vec<&str> = hosts.iter().filter_map(|v| v.as_str()).collect();
        assert!(host_strs.contains(&"web01"));
        assert!(host_strs.contains(&"web02"));
        assert!(host_strs.contains(&"web03"));

        let vars = webservers
            .get("vars")
            .and_then(|v| v.as_object())
            .expect("webservers.vars object");
        assert_eq!(vars.get("http_port").and_then(|v| v.as_i64()), Some(8080));
    }

    #[test]
    fn json_host_returns_merged_vars() {
        let inv = make_test_inv();
        let json = to_ansible_host_json(&inv, "web01");
        let obj = json.as_object().expect("host json is object");
        // `all.vars` flows through merge.
        assert_eq!(
            obj.get("ntp_pool").and_then(|v| v.as_str()),
            Some("pool.ntp.org")
        );
        // `webservers.vars` flows through merge for a webservers member.
        assert_eq!(obj.get("http_port").and_then(|v| v.as_i64()), Some(8080));
    }

    // -----------------------------------------------------------------------
    // merge_inventories
    // -----------------------------------------------------------------------

    #[test]
    fn merge_non_overlapping_inventories() {
        let a = parse_inventory(
            r#"
[webservers.hosts]
web01 = {}
"#,
        )
        .unwrap();
        let b = parse_inventory(
            r#"
[databases.hosts]
db01 = {}
"#,
        )
        .unwrap();
        let merged = merge_inventories(a, b).unwrap();
        assert!(merged.hosts.contains_key("web01"));
        assert!(merged.hosts.contains_key("db01"));
        assert!(merged.groups.contains_key("webservers"));
        assert!(merged.groups.contains_key("databases"));
    }

    #[test]
    fn merge_conflicting_groups_errors() {
        let a = parse_inventory(
            r#"
[webservers.hosts]
web01 = {}
"#,
        )
        .unwrap();
        let b = parse_inventory(
            r#"
[webservers.hosts]
web02 = {}
"#,
        )
        .unwrap();
        let err = merge_inventories(a, b).unwrap_err();
        match err {
            InventoryError::MergeConflictGroup { group } => {
                assert_eq!(group, "webservers");
            }
            other => panic!("expected MergeConflictGroup, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // group_vars / host_vars directory loader (parse_inventory_with_dirs)
    // -----------------------------------------------------------------------

    #[test]
    fn group_vars_file_overrides_inline() {
        let dir = tempfile::tempdir().unwrap();
        let gv = dir.path().join("group_vars");
        std::fs::create_dir(&gv).unwrap();
        std::fs::write(gv.join("webservers.toml"), "port = 8080\n").unwrap();

        let src = r#"
[webservers.vars]
port = 80

[webservers.hosts]
web01 = {}
"#;
        let inv = parse_inventory_with_dirs(src, Some(dir.path())).unwrap();
        let webs = inv.groups.get("webservers").expect("webservers present");
        assert_eq!(
            webs.vars.get("port").and_then(|v| v.as_integer()),
            Some(8080),
            "group_vars/webservers.toml should win over inline [webservers.vars]"
        );
    }

    #[test]
    fn host_vars_file_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let hv = dir.path().join("host_vars");
        std::fs::create_dir(&hv).unwrap();
        std::fs::write(hv.join("web01.toml"), "role = \"frontend\"\n").unwrap();

        let src = r#"
[all.hosts]
web01 = {}
"#;
        let inv = parse_inventory_with_dirs(src, Some(dir.path())).unwrap();
        let host = inv.hosts.get("web01").expect("web01 present");
        assert_eq!(
            host.vars.get("role").and_then(|v| v.as_str()),
            Some("frontend"),
            "host_vars/web01.toml should populate host vars"
        );
    }

    #[test]
    fn host_vars_overrides_group_vars() {
        let dir = tempfile::tempdir().unwrap();
        let gv = dir.path().join("group_vars");
        let hv = dir.path().join("host_vars");
        std::fs::create_dir(&gv).unwrap();
        std::fs::create_dir(&hv).unwrap();
        std::fs::write(gv.join("webservers.toml"), "port = 80\n").unwrap();
        std::fs::write(hv.join("web01.toml"), "port = 8443\n").unwrap();

        let src = r#"
[webservers.hosts]
web01 = {}
"#;
        let inv = parse_inventory_with_dirs(src, Some(dir.path())).unwrap();

        // Group has 80, host has 8443 — both stored as raw vars.
        let webs = inv.groups.get("webservers").expect("webservers present");
        assert_eq!(
            webs.vars.get("port").and_then(|v| v.as_integer()),
            Some(80)
        );
        let host = inv.hosts.get("web01").expect("web01 present");
        assert_eq!(
            host.vars.get("port").and_then(|v| v.as_integer()),
            Some(8443)
        );

        // The merged view (what callers actually consume) puts host on top.
        let merged = inv.merged_vars_for("web01");
        assert_eq!(
            merged.get("port").and_then(|v| v.as_integer()),
            Some(8443),
            "host vars must win over group vars unconditionally"
        );
    }

    #[test]
    fn group_vars_directory_lexicographic() {
        let dir = tempfile::tempdir().unwrap();
        let gv_dir = dir.path().join("group_vars").join("webservers");
        std::fs::create_dir_all(&gv_dir).unwrap();
        // Earlier file sets port=80, later file (lex-sorted) overrides to 443.
        std::fs::write(gv_dir.join("01-base.toml"), "port = 80\n").unwrap();
        std::fs::write(gv_dir.join("02-prod.toml"), "port = 443\n").unwrap();

        let src = r#"
[webservers.hosts]
web01 = {}
"#;
        let inv = parse_inventory_with_dirs(src, Some(dir.path())).unwrap();
        let webs = inv.groups.get("webservers").expect("webservers present");
        assert_eq!(
            webs.vars.get("port").and_then(|v| v.as_integer()),
            Some(443),
            "later files in group_vars/<group>/ must override earlier ones (lex order)"
        );
    }

    #[test]
    fn nonexistent_inventory_dir_is_noop() {
        let bogus = std::path::PathBuf::from("/this/path/should/not/exist/runsible-inv-test");
        let src = r#"
[webservers.hosts]
web01 = {}
"#;
        // Must not error — missing inventory_dir is silently ignored.
        let inv = parse_inventory_with_dirs(src, Some(&bogus)).unwrap();
        assert!(inv.hosts.contains_key("web01"));
        assert!(inv.groups.contains_key("webservers"));
    }

    #[test]
    fn non_toml_files_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let hv = dir.path().join("host_vars");
        std::fs::create_dir(&hv).unwrap();
        // README and txt files MUST be ignored — only .toml is read.
        std::fs::write(hv.join("web01.txt"), "this should be ignored\n").unwrap();
        std::fs::write(hv.join("README"), "no extension\n").unwrap();
        std::fs::write(hv.join("web01.toml"), "role = \"frontend\"\n").unwrap();

        let src = r#"
[all.hosts]
web01 = {}
"#;
        let inv = parse_inventory_with_dirs(src, Some(dir.path())).unwrap();
        let host = inv.hosts.get("web01").expect("web01 present");
        // Only the .toml contribution should land in vars.
        assert_eq!(
            host.vars.get("role").and_then(|v| v.as_str()),
            Some("frontend")
        );
        assert_eq!(
            host.vars.len(),
            1,
            "non-.toml host_vars files must be silently skipped"
        );
    }

    // -----------------------------------------------------------------------
    // YAML inventory import (routes through yaml2toml)
    // -----------------------------------------------------------------------

    #[test]
    fn yaml_basic_inventory_parses() {
        let yaml = "all:\n  children:\n    web:\n      hosts:\n        web01: {}\n";
        let inv = parse_inventory_from_yaml(yaml).expect("parse yaml");
        assert!(
            inv.groups.contains_key("web"),
            "expected 'web' group, got {:?}",
            inv.groups.keys().collect::<Vec<_>>()
        );
        let web = inv.groups.get("web").unwrap();
        assert!(
            web.hosts.iter().any(|h| h == "web01"),
            "expected web01 in 'web' group, got {:?}",
            web.hosts
        );
        assert!(inv.hosts.contains_key("web01"));
    }

    #[test]
    fn yaml_with_inline_vars() {
        let yaml = r#"
web:
  hosts:
    web01:
      ansible_host: 10.0.0.5
"#;
        let inv = parse_inventory_from_yaml(yaml).expect("parse yaml");
        let host = inv.hosts.get("web01").expect("web01 present");
        assert_eq!(
            host.vars.get("ansible_host").and_then(|v| v.as_str()),
            Some("10.0.0.5"),
            "ansible_host not propagated through yaml→toml→inventory"
        );
    }

    #[test]
    fn yaml_invalid_yaml_errors() {
        // YAML containing unbalanced flow-mapping braces.
        let bad = "this: { is: not: valid yaml }";
        assert!(parse_inventory_from_yaml(bad).is_err());
    }

    #[test]
    fn yaml_with_group_vars_section() {
        let yaml = r#"
webservers:
  hosts:
    web01: {}
  vars:
    http_port: 80
"#;
        let inv = parse_inventory_from_yaml(yaml).expect("parse yaml");
        let g = inv
            .groups
            .get("webservers")
            .expect("webservers group present");
        assert_eq!(
            g.vars.get("http_port").and_then(|v| v.as_integer()),
            Some(80)
        );
    }

    // -----------------------------------------------------------------------
    // INI inventory import (hand-rolled parser, plan §9.1)
    // -----------------------------------------------------------------------

    #[test]
    fn ini_basic_groups_and_hosts() {
        let ini = "[web]\nweb01\nweb02\n";
        let inv = parse_inventory_from_ini(ini).expect("parse ini");
        let g = inv.groups.get("web").expect("web group present");
        assert!(g.hosts.iter().any(|h| h == "web01"), "missing web01");
        assert!(g.hosts.iter().any(|h| h == "web02"), "missing web02");
        assert!(inv.hosts.contains_key("web01"));
        assert!(inv.hosts.contains_key("web02"));
    }

    #[test]
    fn ini_inline_host_vars() {
        let ini = "[web]\nweb01 port=8080 proto=http\n";
        let inv = parse_inventory_from_ini(ini).expect("parse ini");
        let host = inv.hosts.get("web01").expect("web01 present");

        // `port=8080` is bare-numeric — typed as TOML integer (locked-in choice).
        assert_eq!(
            host.vars.get("port").and_then(|v| v.as_integer()),
            Some(8080),
            "expected port to type as integer; got {:?}",
            host.vars.get("port")
        );

        // `proto=http` is a non-numeric bare token — typed as TOML string.
        assert_eq!(
            host.vars.get("proto").and_then(|v| v.as_str()),
            Some("http")
        );
    }

    #[test]
    fn ini_group_vars_section() {
        let ini = "[web]\nweb01\n\n[web:vars]\nport=80\nname=hello\n";
        let inv = parse_inventory_from_ini(ini).expect("parse ini");
        let g = inv.groups.get("web").expect("web group present");
        assert_eq!(
            g.vars.get("port").and_then(|v| v.as_integer()),
            Some(80),
            "expected port=80 typed as integer; got {:?}",
            g.vars.get("port")
        );
        assert_eq!(
            g.vars.get("name").and_then(|v| v.as_str()),
            Some("hello")
        );
    }

    #[test]
    fn ini_group_children_section() {
        let ini = "[web]\nweb01\n\n[db]\ndb01\n\n[prod:children]\nweb\ndb\n";
        let inv = parse_inventory_from_ini(ini).expect("parse ini");
        let prod = inv.groups.get("prod").expect("prod group present");
        assert_eq!(prod.children, vec!["web".to_string(), "db".to_string()]);
    }

    #[test]
    fn ini_comments_skipped() {
        let ini = r#"
# this is a comment
; this also a comment
[web]
# inline comment
web01
; another comment
web02
"#;
        let inv = parse_inventory_from_ini(ini).expect("parse ini");
        let g = inv.groups.get("web").expect("web group present");
        assert!(g.hosts.iter().any(|h| h == "web01"));
        assert!(g.hosts.iter().any(|h| h == "web02"));
        // No spurious hosts created from the comment lines.
        assert_eq!(g.hosts.len(), 2, "got hosts: {:?}", g.hosts);
    }

    #[test]
    fn ini_empty_string_yields_empty_inventory() {
        let inv = parse_inventory_from_ini("").expect("parse empty ini");
        assert_eq!(inv.hosts.len(), 0);
        // `all` and `ungrouped` are always present.
        assert!(inv.groups.contains_key("all"));
        assert!(inv.groups.contains_key("ungrouped"));
        // No other groups beyond those two implicit ones.
        let extra: Vec<&String> = inv
            .groups
            .keys()
            .filter(|k| k.as_str() != "all" && k.as_str() != "ungrouped")
            .collect();
        assert!(extra.is_empty(), "unexpected extra groups: {extra:?}");
    }
}
