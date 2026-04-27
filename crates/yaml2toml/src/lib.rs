// SPDX-License-Identifier: Unlicense
// Contributors: Cochran Block

//! yaml2toml — best-effort YAML → TOML conversion for runsible-shaped files.
//!
//! Targets Ansible playbooks, inventories, and vars files. Not a general YAML→TOML
//! converter; shapes outside those profiles are handled as `Vars` (flat mapping).

use serde_yaml::Value as YamlValue;
use thiserror::Error;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Key, Table, Value as TomlValue, value};

// ─── Public types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Playbook,
    Inventory,
    Vars,
    Auto,
}

#[derive(Debug)]
pub struct ConvertResult {
    pub toml: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ConvertError {
    #[error("yaml parse: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("toml serialize: {0}")]
    Toml(String),
    #[error("unexpected structure: {0}")]
    Structure(String),
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn convert(yaml: &str, profile: Profile) -> Result<ConvertResult, ConvertError> {
    let root: YamlValue = serde_yaml::from_str(yaml)?;

    let effective = match profile {
        Profile::Auto => detect_profile(&root),
        p => p,
    };

    let mut warnings: Vec<String> = Vec::new();

    let toml = match effective {
        Profile::Playbook => convert_playbook(&root, &mut warnings)?,
        Profile::Inventory => convert_inventory(&root, &mut warnings)?,
        Profile::Vars | Profile::Auto => convert_vars(&root, &mut warnings)?,
    };

    Ok(ConvertResult { toml, warnings })
}

// ─── Profile detection ───────────────────────────────────────────────────────

fn detect_profile(root: &YamlValue) -> Profile {
    match root {
        YamlValue::Sequence(_) => Profile::Playbook,
        YamlValue::Mapping(m) => {
            let all_values_are_mappings = m.values().all(|v| matches!(v, YamlValue::Mapping(_)));
            if all_values_are_mappings && !m.is_empty() {
                Profile::Inventory
            } else {
                Profile::Vars
            }
        }
        _ => Profile::Vars,
    }
}

// ─── Key helpers ─────────────────────────────────────────────────────────────

fn yaml_type_name(v: &YamlValue) -> &'static str {
    match v {
        YamlValue::Null => "null",
        YamlValue::Bool(_) => "bool",
        YamlValue::Number(_) => "number",
        YamlValue::String(_) => "string",
        YamlValue::Sequence(_) => "sequence",
        YamlValue::Mapping(_) => "mapping",
        YamlValue::Tagged(_) => "tagged",
    }
}

fn yaml_key_to_string(k: &YamlValue) -> Result<String, ConvertError> {
    match k {
        YamlValue::String(s) => Ok(s.clone()),
        YamlValue::Number(n) => Ok(n.to_string()),
        YamlValue::Bool(b) => Ok(b.to_string()),
        other => Err(ConvertError::Structure(format!(
            "unsupported YAML key type: {}",
            yaml_type_name(other)
        ))),
    }
}

/// Build a `toml_edit::Key` that is properly bare or quoted.
/// A bare TOML key matches [A-Za-z0-9_-]+. Everything else is double-quoted.
fn make_key(k: &str) -> Key {
    let is_bare = !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if is_bare {
        Key::new(k)
    } else {
        // toml_edit::Key::new always produces a bare key internally; to force a
        // quoted representation we must parse a pre-quoted string.
        let quoted = format!("\"{}\"", k.replace('\\', "\\\\").replace('"', "\\\""));
        toml_edit::Key::parse(&quoted)
            .ok()
            .and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) })
            .unwrap_or_else(|| Key::new(k))
    }
}

/// Returns the raw string form of the key as it should appear in TOML output.
/// Used only for the `key_quoting` test assertion.
pub fn toml_key_repr(k: &str) -> String {
    let is_bare = !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if is_bare {
        k.to_string()
    } else {
        format!("\"{}\"", k.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

// ─── Vars conversion ─────────────────────────────────────────────────────────

fn convert_vars(root: &YamlValue, warnings: &mut Vec<String>) -> Result<String, ConvertError> {
    let mut doc = DocumentMut::new();

    match root {
        YamlValue::Mapping(m) => {
            let table = doc.as_table_mut();
            mapping_into_table(m, table, warnings)?;
        }
        other => {
            return Err(ConvertError::Structure(format!(
                "vars profile expects a top-level mapping, got {}",
                yaml_type_name(other)
            )));
        }
    }

    Ok(doc.to_string())
}

/// Insert all entries of a YAML mapping into an existing `toml_edit::Table`.
fn mapping_into_table(
    m: &serde_yaml::Mapping,
    table: &mut Table,
    warnings: &mut Vec<String>,
) -> Result<(), ConvertError> {
    for (k, v) in m {
        let key_str = yaml_key_to_string(k)?;
        let key = make_key(&key_str);
        let item = yaml_to_toml_item(v, warnings)?;
        table.insert_formatted(&key, item);
    }
    Ok(())
}

// ─── Playbook conversion ─────────────────────────────────────────────────────

fn convert_playbook(root: &YamlValue, warnings: &mut Vec<String>) -> Result<String, ConvertError> {
    let plays = match root {
        YamlValue::Sequence(s) => s,
        other => {
            return Err(ConvertError::Structure(format!(
                "playbook profile expects a top-level sequence, got {}",
                yaml_type_name(other)
            )));
        }
    };

    let mut doc = DocumentMut::new();

    let mut aot = toml_edit::ArrayOfTables::new();

    for play_yaml in plays {
        let play_map = match play_yaml {
            YamlValue::Mapping(m) => m,
            other => {
                return Err(ConvertError::Structure(format!(
                    "each play must be a mapping, got {}",
                    yaml_type_name(other)
                )));
            }
        };

        let mut play_table = Table::new();

        for (k, v) in play_map {
            let key_str = yaml_key_to_string(k)?;

            // Task-list keys become arrays of inline tables inside the play
            if matches!(
                key_str.as_str(),
                "tasks" | "handlers" | "pre_tasks" | "post_tasks"
            ) {
                let tasks_item = task_list_to_toml_array(v, warnings)?;
                play_table.insert_formatted(&make_key(&key_str), tasks_item);
            } else {
                let item = yaml_to_toml_item(v, warnings)?;
                play_table.insert_formatted(&make_key(&key_str), item);
            }
        }

        aot.push(play_table);
    }

    doc.insert_formatted(
        &Key::new("plays"),
        Item::ArrayOfTables(aot),
    );

    Ok(doc.to_string())
}

/// Convert a YAML sequence of task maps to a TOML array of inline tables.
fn task_list_to_toml_array(
    v: &YamlValue,
    warnings: &mut Vec<String>,
) -> Result<Item, ConvertError> {
    let seq = match v {
        YamlValue::Sequence(s) => s,
        other => {
            return Err(ConvertError::Structure(format!(
                "task list must be a sequence, got {}",
                yaml_type_name(other)
            )));
        }
    };

    let mut arr = Array::new();
    for task_yaml in seq {
        let inline = yaml_mapping_to_inline_table(task_yaml, warnings)?;
        arr.push_formatted(TomlValue::InlineTable(inline));
    }

    Ok(Item::Value(TomlValue::Array(arr)))
}

/// Recursively convert a YAML mapping into a TOML inline table.
fn yaml_mapping_to_inline_table(
    v: &YamlValue,
    warnings: &mut Vec<String>,
) -> Result<InlineTable, ConvertError> {
    let m = match v {
        YamlValue::Mapping(m) => m,
        other => {
            return Err(ConvertError::Structure(format!(
                "expected mapping for inline table, got {}",
                yaml_type_name(other)
            )));
        }
    };

    let mut table = InlineTable::new();
    for (k, val) in m {
        let key_str = yaml_key_to_string(k)?;
        let key = make_key(&key_str);
        let tv = yaml_to_toml_value(val, warnings)?;
        table.insert_formatted(&key, tv);
    }
    Ok(table)
}

// ─── Inventory conversion ────────────────────────────────────────────────────

fn convert_inventory(
    root: &YamlValue,
    warnings: &mut Vec<String>,
) -> Result<String, ConvertError> {
    let top = match root {
        YamlValue::Mapping(m) => m,
        other => {
            return Err(ConvertError::Structure(format!(
                "inventory profile expects a top-level mapping, got {}",
                yaml_type_name(other)
            )));
        }
    };

    let mut doc = DocumentMut::new();

    for (top_k, top_v) in top {
        let top_key = yaml_key_to_string(top_k)?;

        match top_v {
            YamlValue::Mapping(group_map) => {
                emit_inventory_group(&top_key, group_map, &mut doc, warnings)?;
            }
            YamlValue::Null => {
                get_or_create_table(&mut doc, &top_key);
            }
            other => {
                return Err(ConvertError::Structure(format!(
                    "inventory group '{}' value must be a mapping, got {}",
                    top_key,
                    yaml_type_name(other)
                )));
            }
        }
    }

    Ok(doc.to_string())
}

fn emit_inventory_group(
    group_name: &str,
    group_map: &serde_yaml::Mapping,
    doc: &mut DocumentMut,
    warnings: &mut Vec<String>,
) -> Result<(), ConvertError> {
    // hosts: mapping of hostname -> (null | mapping of vars)
    if let Some(hosts_val) = group_map.get("hosts") {
        match hosts_val {
            YamlValue::Mapping(hosts_map) => {
                // Ensure [group.hosts] table exists
                let hosts_table = get_or_create_subtable(doc, group_name, "hosts");
                for (h, hv) in hosts_map {
                    let host = yaml_key_to_string(h)?;
                    let host_key = make_key(&host);
                    match hv {
                        YamlValue::Null => {
                            hosts_table.insert_formatted(
                                &host_key,
                                Item::Value(TomlValue::InlineTable(InlineTable::new())),
                            );
                        }
                        YamlValue::Mapping(hm) if hm.is_empty() => {
                            hosts_table.insert_formatted(
                                &host_key,
                                Item::Value(TomlValue::InlineTable(InlineTable::new())),
                            );
                        }
                        YamlValue::Mapping(hm) => {
                            let mut it = InlineTable::new();
                            for (vk, vv) in hm {
                                let vkey = yaml_key_to_string(vk)?;
                                let tv = yaml_to_toml_value(vv, warnings)?;
                                it.insert_formatted(&make_key(&vkey), tv);
                            }
                            hosts_table.insert_formatted(
                                &host_key,
                                Item::Value(TomlValue::InlineTable(it)),
                            );
                        }
                        _ => {
                            let item = yaml_to_toml_item(hv, warnings)?;
                            hosts_table.insert_formatted(&host_key, item);
                        }
                    }
                }
            }
            YamlValue::Null => {} // no hosts
            other => {
                return Err(ConvertError::Structure(format!(
                    "group '{}' hosts must be a mapping, got {}",
                    group_name,
                    yaml_type_name(other)
                )));
            }
        }
    }

    // vars: mapping of var_name -> value
    if let Some(vars_val) = group_map.get("vars") {
        if let YamlValue::Mapping(vars_map) = vars_val {
            let vars_table = get_or_create_subtable(doc, group_name, "vars");
            for (vk, vv) in vars_map {
                let vkey = yaml_key_to_string(vk)?;
                let item = yaml_to_toml_item(vv, warnings)?;
                vars_table.insert_formatted(&make_key(&vkey), item);
            }
        }
    }

    // children: mapping of child_group_name -> (null | mapping)
    if let Some(children_val) = group_map.get("children") {
        if let YamlValue::Mapping(children_map) = children_val {
            let mut child_names: Vec<String> = Vec::new();
            for (ck, cv) in children_map {
                let child_name = yaml_key_to_string(ck)?;
                child_names.push(child_name.clone());

                // Recurse into child group definition
                match cv {
                    YamlValue::Mapping(cm) => {
                        emit_inventory_group(&child_name, cm, doc, warnings)?;
                    }
                    YamlValue::Null => {
                        get_or_create_table(doc, &child_name);
                    }
                    _ => {}
                }
            }

            // Store children = [...] on the parent group table
            let mut arr = Array::new();
            for n in &child_names {
                arr.push_formatted(TomlValue::from(n.as_str()));
            }
            let parent_table = get_or_create_table(doc, group_name);
            parent_table.insert_formatted(&Key::new("children"), Item::Value(TomlValue::Array(arr)));
        }
    }

    Ok(())
}

/// Get or create a top-level table named `name` in the document.
/// Returns a mutable reference to the inner `Table`.
fn get_or_create_table<'a>(doc: &'a mut DocumentMut, name: &str) -> &'a mut Table {
    if !doc.as_table().contains_key(name) {
        doc.as_table_mut().insert(name, Item::Table(Table::new()));
    }
    doc.as_table_mut()
        .get_mut(name)
        .expect("table just inserted")
        .as_table_mut()
        .expect("item is Table")
}

/// Get or create `[parent.child]` as a table inside the document.
/// Returns a mutable reference to the child `Table`.
fn get_or_create_subtable<'a>(
    doc: &'a mut DocumentMut,
    parent: &str,
    child: &str,
) -> &'a mut Table {
    // Ensure parent exists
    if !doc.as_table().contains_key(parent) {
        doc.as_table_mut().insert(parent, Item::Table(Table::new()));
    }
    let parent_table = doc
        .as_table_mut()
        .get_mut(parent)
        .expect("parent just inserted")
        .as_table_mut()
        .expect("parent is Table");

    if !parent_table.contains_key(child) {
        parent_table.insert(child, Item::Table(Table::new()));
    }
    parent_table
        .get_mut(child)
        .expect("child just inserted")
        .as_table_mut()
        .expect("child is Table")
}

// ─── Value conversion helpers ─────────────────────────────────────────────────

fn yaml_to_toml_item(v: &YamlValue, warnings: &mut Vec<String>) -> Result<Item, ConvertError> {
    let tv = yaml_to_toml_value(v, warnings)?;
    Ok(value(tv))
}

fn yaml_to_toml_value(
    v: &YamlValue,
    warnings: &mut Vec<String>,
) -> Result<TomlValue, ConvertError> {
    match v {
        YamlValue::Null => {
            warnings.push("yaml2toml: null coerced to \"\"".to_string());
            Ok(TomlValue::from(""))
        }
        YamlValue::Bool(b) => Ok(TomlValue::from(*b)),
        YamlValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(TomlValue::from(i))
            } else if let Some(f) = n.as_f64() {
                Ok(TomlValue::from(f))
            } else {
                Ok(TomlValue::from(n.to_string()))
            }
        }
        YamlValue::String(s) => Ok(TomlValue::from(s.as_str())),
        YamlValue::Sequence(seq) => {
            let mut arr = Array::new();
            for item in seq {
                arr.push_formatted(yaml_to_toml_value(item, warnings)?);
            }
            Ok(TomlValue::Array(arr))
        }
        YamlValue::Mapping(m) => {
            let mut table = InlineTable::new();
            for (k, val) in m {
                let key_str = yaml_key_to_string(k)?;
                let tv = yaml_to_toml_value(val, warnings)?;
                table.insert_formatted(&make_key(&key_str), tv);
            }
            Ok(TomlValue::InlineTable(table))
        }
        YamlValue::Tagged(tagged) => {
            // Unwrap tagged values (e.g. !!str, !!int)
            yaml_to_toml_value(&tagged.value, warnings)
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // 1. vars round-trip
    #[test]
    fn vars_round_trip() {
        let yaml = r#"
http_port: 8080
enable_feature: true
app_name: "myapp"
"#;
        let result = convert(yaml, Profile::Vars).expect("convert failed");
        assert!(result.warnings.is_empty(), "unexpected warnings: {:?}", result.warnings);

        let parsed: toml::Value = toml::from_str(&result.toml).expect("invalid TOML output");
        assert_eq!(parsed["http_port"].as_integer(), Some(8080));
        assert_eq!(parsed["enable_feature"].as_bool(), Some(true));
        assert_eq!(parsed["app_name"].as_str(), Some("myapp"));
    }

    // 2. null coercion
    #[test]
    fn null_coercion() {
        let yaml = "foo: ~\n";
        let result = convert(yaml, Profile::Vars).expect("convert failed");
        assert!(!result.warnings.is_empty(), "expected a warning for null coercion");

        let parsed: toml::Value = toml::from_str(&result.toml).expect("invalid TOML output");
        assert_eq!(parsed["foo"].as_str(), Some(""));
    }

    // 3. key quoting — IP-address-like key
    #[test]
    fn key_quoting() {
        let yaml = "192.168.1.1: some_host\n";
        let result = convert(yaml, Profile::Vars).expect("convert failed");
        // The output TOML must contain the key quoted
        assert!(
            result.toml.contains("\"192.168.1.1\""),
            "expected quoted key in: {}",
            result.toml
        );
        // Must parse cleanly
        let parsed: toml::Value = toml::from_str(&result.toml).expect("invalid TOML output");
        assert_eq!(parsed["192.168.1.1"].as_str(), Some("some_host"));
    }

    // 4. inventory profile
    #[test]
    fn inventory_profile() {
        let yaml = r#"
all:
  children:
    webservers:
      hosts:
        web01: {}
        web02: {}
      vars:
        http_port: 80
"#;
        let result = convert(yaml, Profile::Inventory).expect("convert failed");
        let toml_str = &result.toml;

        let parsed: toml::Value = toml::from_str(toml_str)
            .unwrap_or_else(|e| panic!("invalid TOML output:\n{}\n\nerror: {}", toml_str, e));

        // [webservers.vars] http_port = 80
        let http_port = parsed
            .get("webservers")
            .and_then(|g| g.get("vars"))
            .and_then(|v| v.get("http_port"))
            .and_then(|p| p.as_integer());
        assert_eq!(http_port, Some(80), "http_port not found; TOML:\n{}", toml_str);
    }

    // 5. playbook profile
    #[test]
    fn playbook_profile() {
        let yaml = r#"
- name: install nginx
  hosts: webservers
  tasks:
    - name: install
      ansible.builtin.package:
        name: nginx
        state: present
"#;
        let result = convert(yaml, Profile::Playbook).expect("convert failed");
        let toml_str = &result.toml;

        let parsed: toml::Value = toml::from_str(toml_str)
            .unwrap_or_else(|e| panic!("invalid TOML output:\n{}\n\nerror: {}", toml_str, e));

        let plays = parsed["plays"].as_array().expect("[[plays]] missing");
        assert!(!plays.is_empty(), "plays array is empty");
        let first = &plays[0];
        assert_eq!(first["name"].as_str(), Some("install nginx"));
        assert_eq!(first["hosts"].as_str(), Some("webservers"));
    }

    // 6. auto-detect playbook
    #[test]
    fn auto_detect_playbook() {
        let yaml = "- name: do something\n  hosts: all\n";
        let result = convert(yaml, Profile::Auto).expect("convert failed");
        assert!(
            result.toml.contains("[[plays]]"),
            "expected [[plays]] in: {}",
            result.toml
        );
    }

    // 7. auto-detect inventory
    #[test]
    fn auto_detect_inventory() {
        let yaml = r#"
webservers:
  hosts:
    web01: {}
databases:
  hosts:
    db01: {}
"#;
        let result = convert(yaml, Profile::Auto).expect("convert failed");
        // Should have produced inventory-shaped TOML (no [[plays]])
        assert!(
            !result.toml.contains("[[plays]]"),
            "unexpected [[plays]] in inventory output: {}",
            result.toml
        );
        let parsed: toml::Value = toml::from_str(&result.toml)
            .unwrap_or_else(|e| panic!("invalid TOML:\n{}\n\nerror: {}", result.toml, e));
        assert!(parsed.get("webservers").is_some(), "webservers group missing");
    }

    // ── Added coverage ─────────────────────────────────────────────────

    // ─── Value mappings ───

    /// YAML int beyond i32 range still survives as a TOML integer.
    #[test]
    fn value_large_int() {
        let yaml = "big: 9999999999\n";
        let result = convert(yaml, Profile::Vars).expect("convert");
        let parsed: toml::Value = toml::from_str(&result.toml).expect("toml");
        assert_eq!(parsed["big"].as_integer(), Some(9_999_999_999));
    }

    /// Negative integers preserved.
    #[test]
    fn value_negative_int() {
        let yaml = "n: -42\n";
        let result = convert(yaml, Profile::Vars).expect("convert");
        let parsed: toml::Value = toml::from_str(&result.toml).expect("toml");
        assert_eq!(parsed["n"].as_integer(), Some(-42));
    }

    /// Float survives as TOML float.
    #[test]
    fn value_float() {
        let yaml = "pi: 3.14\n";
        let result = convert(yaml, Profile::Vars).expect("convert");
        let parsed: toml::Value = toml::from_str(&result.toml).expect("toml");
        let pi = parsed["pi"].as_float().expect("pi as float");
        assert!((pi - 3.14).abs() < 1e-9);
    }

    /// Bool true and false both survive.
    #[test]
    fn value_bool_true_false() {
        let yaml = "t: true\nf: false\n";
        let result = convert(yaml, Profile::Vars).expect("convert");
        let parsed: toml::Value = toml::from_str(&result.toml).expect("toml");
        assert_eq!(parsed["t"].as_bool(), Some(true));
        assert_eq!(parsed["f"].as_bool(), Some(false));
    }

    /// Quoted YAML string with an embedded newline survives as TOML string.
    #[test]
    fn value_string_with_special_chars() {
        let yaml = "s: \"hello\\nworld\"\n";
        let result = convert(yaml, Profile::Vars).expect("convert");
        let parsed: toml::Value = toml::from_str(&result.toml).expect("toml");
        assert_eq!(parsed["s"].as_str(), Some("hello\nworld"));
    }

    /// Block-scalar (literal) string: lines preserved as-is.
    #[test]
    fn value_block_scalar_literal() {
        let yaml = "doc: |\n  line one\n  line two\n";
        let result = convert(yaml, Profile::Vars).expect("convert");
        let parsed: toml::Value = toml::from_str(&result.toml).expect("toml");
        let s = parsed["doc"].as_str().expect("doc string");
        assert!(s.contains("line one"), "got: {:?}", s);
        assert!(s.contains("line two"), "got: {:?}", s);
        // The literal block keeps a newline between the two lines
        assert!(s.contains("line one\nline two"), "got: {:?}", s);
    }

    // ─── Sequences / mappings ───

    /// Empty top-level YAML mapping yields parseable TOML.
    #[test]
    fn empty_mapping_parses() {
        let yaml = "{}\n";
        let result = convert(yaml, Profile::Vars).expect("convert");
        let parsed: toml::Value =
            toml::from_str(&result.toml).expect("invalid TOML output for empty mapping");
        if let toml::Value::Table(t) = parsed {
            assert!(t.is_empty(), "expected empty table, got {:?}", t);
        } else {
            panic!("expected table at top level");
        }
    }

    /// Three-level nested mapping survives as TOML and round-trips.
    #[test]
    fn nested_mapping_three_levels() {
        let yaml = r#"
a:
  b:
    c: 1
"#;
        let result = convert(yaml, Profile::Vars).expect("convert");
        let parsed: toml::Value = toml::from_str(&result.toml)
            .unwrap_or_else(|e| panic!("invalid TOML:\n{}\n\nerror: {}", result.toml, e));
        let c = parsed
            .get("a")
            .and_then(|v| v.get("b"))
            .and_then(|v| v.get("c"))
            .and_then(|v| v.as_integer());
        assert_eq!(c, Some(1), "TOML was:\n{}", result.toml);
    }

    /// Array of mappings survives as TOML and elements are accessible.
    #[test]
    fn array_of_mappings() {
        let yaml = r#"
items:
  - name: alice
    age: 30
  - name: bob
    age: 25
"#;
        let result = convert(yaml, Profile::Vars).expect("convert");
        let parsed: toml::Value = toml::from_str(&result.toml)
            .unwrap_or_else(|e| panic!("invalid TOML:\n{}\n\nerror: {}", result.toml, e));
        let items = parsed["items"].as_array().expect("items array");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].get("name").and_then(|v| v.as_str()), Some("alice"));
        assert_eq!(items[1].get("age").and_then(|v| v.as_integer()), Some(25));
    }

    // ─── Profile detection ───

    /// `Profile::Auto` on an empty top-level list — current behavior is to take
    /// the Playbook branch (Sequence -> Playbook). Lock that in.
    #[test]
    fn profile_auto_empty_list_locks_behavior() {
        let yaml = "[]\n";
        let result = convert(yaml, Profile::Auto).expect("convert");
        // Playbook handler emits an empty `[[plays]]` array of tables. The TOML
        // serializer for an empty AOT may emit either nothing or an empty section,
        // so we just make sure the output parses and does NOT contain [vars]-shaped
        // top-level keys (i.e. it is not Vars-profile output).
        let parsed: toml::Value = toml::from_str(&result.toml)
            .unwrap_or_else(|e| panic!("invalid TOML:\n{}\n\nerror: {}", result.toml, e));
        // Either no `plays` key (because empty AOT was elided) or `plays` is an
        // empty array. Either is acceptable; we just want parse success.
        match parsed.get("plays") {
            None => {}
            Some(toml::Value::Array(a)) => assert!(a.is_empty(), "expected empty plays"),
            Some(other) => panic!("unexpected plays shape: {:?}", other),
        }
    }

    /// `Profile::Auto` on a flat scalar mapping detects Vars (no [[plays]]).
    #[test]
    fn profile_auto_flat_scalar_is_vars() {
        let yaml = "alpha: 1\nbeta: two\n";
        let result = convert(yaml, Profile::Auto).expect("convert");
        assert!(
            !result.toml.contains("[[plays]]"),
            "did not expect plays-shape output: {}",
            result.toml
        );
        let parsed: toml::Value = toml::from_str(&result.toml).expect("toml");
        assert_eq!(parsed["alpha"].as_integer(), Some(1));
        assert_eq!(parsed["beta"].as_str(), Some("two"));
    }

    // ─── Key quoting ───

    /// Hyphenated kebab-case keys are bare in TOML (hyphens allowed).
    #[test]
    fn key_quoting_kebab_case_is_bare() {
        let yaml = "kebab-case: 1\n";
        let result = convert(yaml, Profile::Vars).expect("convert");
        // The repr should be bare, with no surrounding quotes.
        assert!(
            result.toml.contains("kebab-case = 1"),
            "expected bare hyphenated key in: {}",
            result.toml
        );
        assert!(
            !result.toml.contains("\"kebab-case\""),
            "did not expect quoted kebab-case key in: {}",
            result.toml
        );
        let parsed: toml::Value = toml::from_str(&result.toml).expect("toml");
        assert_eq!(parsed["kebab-case"].as_integer(), Some(1));
    }

    /// Keys starting with a digit: TOML allows leading digits in bare keys, and
    /// the converter emits them bare. Lock in that behavior — what really matters
    /// is that the output parses round-trip.
    #[test]
    fn key_quoting_leading_digit_round_trips() {
        let yaml = "1key: 1\n";
        let result = convert(yaml, Profile::Vars).expect("convert");
        let parsed: toml::Value = toml::from_str(&result.toml).unwrap_or_else(|e| {
            panic!("invalid TOML for leading-digit key:\n{}\nerr: {}", result.toml, e)
        });
        assert_eq!(parsed["1key"].as_integer(), Some(1));
    }

    /// Keys with characters outside [A-Za-z0-9_-] (here: a dot) MUST be quoted.
    #[test]
    fn key_quoting_dotted_key_is_quoted() {
        let yaml = "weird.key: 1\n";
        let result = convert(yaml, Profile::Vars).expect("convert");
        assert!(
            result.toml.contains("\"weird.key\""),
            "expected quoted dotted key in: {}",
            result.toml
        );
        let parsed: toml::Value = toml::from_str(&result.toml)
            .unwrap_or_else(|e| panic!("invalid TOML:\n{}\n\nerror: {}", result.toml, e));
        assert_eq!(parsed["weird.key"].as_integer(), Some(1));
    }
}
