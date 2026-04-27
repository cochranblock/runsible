//! TOML → AST parsing and task resolution.

use indexmap::IndexMap;

use crate::ast::{Playbook, Task, BLOCK_SENTINEL, INCLUDE_SENTINEL, TASK_META_KEYS};
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

    // delegate_to / run_once are task-level options that orthogonally apply to
    // the dispatch (or the include short-circuit below). Read them up front so
    // every code path below can attach them to the resolved Task uniformly.
    let delegate_to = table
        .get("delegate_to")
        .and_then(|v| v.as_str())
        .map(String::from);
    let run_once = table
        .get("run_once")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // include_tasks / import_tasks short-circuit. Both behave identically at M1
    // (truly-static import semantics are M2). The path is stashed into `args`
    // as a TOML string so the engine can read it back without a special field.
    if let Some(path_v) = table
        .get("include_tasks")
        .or_else(|| table.get("import_tasks"))
    {
        let path = path_v
            .as_str()
            .ok_or_else(|| {
                PlaybookError::TypeCheck(
                    "include_tasks/import_tasks must be a string path".into(),
                )
            })?
            .to_string();
        return Ok(Task {
            name,
            module_name: INCLUDE_SENTINEL.to_string(),
            args: toml::Value::String(path),
            register,
            tags,
            when,
            notify,
            loop_items: None,
            loop_var: "item".to_string(),
            loop_label: None,
            until: None,
            retries: 3,
            delay_seconds: 5,
            block: Vec::new(),
            rescue: Vec::new(),
            always: Vec::new(),
            delegate_to,
            run_once,
        });
    }

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
        delegate_to,
        run_once,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_playbook_has_zero_plays() {
        let pb = parse_playbook("").unwrap();
        assert_eq!(pb.plays.len(), 0);
    }

    #[test]
    fn when_string_shorthand_resolves_to_expr() {
        let raw: toml::Value = toml::from_str(
            r#"
name = "shorthand"
when = "x == 1"
debug = { msg = "hi" }
"#,
        )
        .unwrap();
        let imports = IndexMap::new();
        let task = resolve_task(&raw, &imports).unwrap();
        assert_eq!(task.when.as_deref(), Some("x == 1"));
    }

    #[test]
    fn task_with_register_and_when() {
        let raw: toml::Value = toml::from_str(
            r#"
name = "both"
register = "out"
when = { expr = "ready" }
debug = { msg = "hi" }
"#,
        )
        .unwrap();
        let imports = IndexMap::new();
        let task = resolve_task(&raw, &imports).unwrap();
        assert_eq!(task.register.as_deref(), Some("out"));
        assert_eq!(task.when.as_deref(), Some("ready"));
    }

    #[test]
    fn missing_module_with_no_block_errors() {
        let raw: toml::Value = toml::from_str(
            r#"
name = "no module"
"#,
        )
        .unwrap();
        let imports = IndexMap::new();
        let err = resolve_task(&raw, &imports).unwrap_err();
        assert!(matches!(err, PlaybookError::TypeCheck(_)));
    }

    #[test]
    fn notify_array_parses_two_handlers() {
        let raw: toml::Value = toml::from_str(
            r#"
name = "notify two"
notify = ["a", "b"]
debug = { msg = "x" }
"#,
        )
        .unwrap();
        let imports = IndexMap::new();
        let task = resolve_task(&raw, &imports).unwrap();
        assert_eq!(task.notify.len(), 2);
        assert_eq!(task.notify[0], "a");
        assert_eq!(task.notify[1], "b");
    }

    #[test]
    fn loop_control_renames_loop_var() {
        let raw: toml::Value = toml::from_str(
            r#"
name = "loop"
loop = ["x", "y"]
loop_control = { loop_var = "i" }
debug = { msg = "{{ i }}" }
"#,
        )
        .unwrap();
        let imports = IndexMap::new();
        let task = resolve_task(&raw, &imports).unwrap();
        assert_eq!(task.loop_var, "i");
    }

    #[test]
    fn resolve_handler_uses_id_as_name_when_unnamed() {
        let raw: toml::Value = toml::from_str(r#"debug = { msg = "fire" }"#).unwrap();
        let imports = IndexMap::new();
        let h = resolve_handler("restart_app", &raw, &imports).unwrap();
        assert_eq!(h.name.as_deref(), Some("restart_app"));
    }

    #[test]
    fn imports_alias_resolved_to_fq_name() {
        let raw: toml::Value = toml::from_str(
            r#"
name = "aliased"
dbg = { msg = "hi" }
"#,
        )
        .unwrap();
        let mut imports = IndexMap::new();
        imports.insert("dbg".to_string(), "runsible_builtin.debug".to_string());
        let task = resolve_task(&raw, &imports).unwrap();
        assert_eq!(task.module_name, "runsible_builtin.debug");
    }
}
