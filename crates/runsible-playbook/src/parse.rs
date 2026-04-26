//! TOML → AST parsing and task resolution.

use indexmap::IndexMap;

use crate::ast::{Playbook, Task, TASK_META_KEYS};
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

    let module_keys: Vec<&str> = table
        .keys()
        .filter(|k| !TASK_META_KEYS.contains(&k.as_str()))
        .map(String::as_str)
        .collect();

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

    Ok(Task {
        name,
        module_name,
        args,
        register,
        tags,
    })
}
