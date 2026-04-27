//! TOML → AST parsing and task resolution.

use indexmap::IndexMap;

use crate::ast::{Playbook, Task, BLOCK_SENTINEL, TASK_META_KEYS};
use crate::errors::{PlaybookError, Result};

pub fn parse_playbook(src: &str) -> Result<Playbook> {
    toml::from_str(src).map_err(|e| PlaybookError::Parse(e.to_string()))
}

/// Extract the module call from a raw task TOML value.
///
/// A task is a TOML table where exactly one key is NOT in `TASK_META_KEYS` —
/// that key is the module alias/name and its value is the args table.
pub fn resolve_task(raw: &toml::Value, imports: &IndexMap<String, String>) -> Result<Task> {
    let table = raw
        .as_table()
        .ok_or_else(|| PlaybookError::Parse("task must be a TOML table".into()))?;

    let name = table
        .get("name")
        .and_then(|v| v.as_str())
        .map(String::from);

    let register = table
        .get("register")
        .and_then(|v| v.as_str())
        .map(String::from);

    let tags: Vec<String> = table
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // when = { expr = "..." } | when = "..." (string shorthand)
    let when = match table.get("when") {
        Some(toml::Value::Table(t)) => t.get("expr").and_then(|v| v.as_str()).map(String::from),
        Some(toml::Value::String(s)) => Some(s.clone()),
        _ => None,
    };

    let notify: Vec<String> = table
        .get("notify")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Block / rescue / always — extract first; if present, this is a block task.
    let block: Vec<toml::Value> = table
        .get("block")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let rescue: Vec<toml::Value> = table
        .get("rescue")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let always: Vec<toml::Value> = table
        .get("always")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let is_block = !block.is_empty() || !rescue.is_empty() || !always.is_empty();

    let module_keys: Vec<&str> = table
        .keys()
        .filter(|k| !TASK_META_KEYS.contains(&k.as_str()))
        .map(String::as_str)
        .collect();

    let (module_name, args) = if is_block {
        if !module_keys.is_empty() {
            return Err(PlaybookError::TypeCheck(format!(
                "task {:?}: block tasks cannot also call a module (got module keys: {:?})",
                name, module_keys
            )));
        }
        (BLOCK_SENTINEL.to_string(), toml::Value::Table(toml::map::Map::new()))
    } else {
        if module_keys.len() != 1 {
            return Err(PlaybookError::TypeCheck(format!(
                "task {:?}: expected exactly one module key, found: {:?}",
                name, module_keys
            )));
        }
        let alias = module_keys[0];
        let module_name = imports.get(alias).cloned().unwrap_or_else(|| alias.to_string());
        let args = table
            .get(alias)
            .cloned()
            .unwrap_or(toml::Value::Table(toml::map::Map::new()));
        (module_name, args)
    };

    let loop_items: Option<Vec<toml::Value>> = table
        .get("loop")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().cloned().collect());

    let (loop_var, loop_label) = match table.get("loop_control") {
        Some(toml::Value::Table(t)) => {
            let lv = t
                .get("loop_var")
                .and_then(|v| v.as_str())
                .unwrap_or("item")
                .to_string();
            let lab = t.get("label").and_then(|v| v.as_str()).map(String::from);
            (lv, lab)
        }
        _ => ("item".to_string(), None),
    };

    let until = match table.get("until") {
        Some(toml::Value::Table(t)) => t.get("expr").and_then(|v| v.as_str()).map(String::from),
        Some(toml::Value::String(s)) => Some(s.clone()),
        _ => None,
    };
    let retries = table
        .get("retries")
        .and_then(|v| v.as_integer())
        .map(|i| i.max(1) as u32)
        .unwrap_or(3);
    let delay_seconds = table
        .get("delay_seconds")
        .and_then(|v| v.as_integer())
        .map(|i| i.max(0) as u64)
        .unwrap_or(5);

    Ok(Task {
        name,
        module_name,
        args,
        register,
        tags,
        when,
        notify,
        loop_items,
        loop_var,
        loop_label,
        until,
        retries,
        delay_seconds,
        block,
        rescue,
        always,
    })
}

/// Resolve a handler entry: an `[plays.handlers.<id>]` table.
/// Same shape as a task but with no `notify`/`register` etc.
pub fn resolve_handler(
    id: &str,
    raw: &toml::Value,
    imports: &IndexMap<String, String>,
) -> Result<Task> {
    let mut t = resolve_task(raw, imports)?;
    if t.name.is_none() {
        t.name = Some(id.to_string());
    }
    Ok(t)
}
