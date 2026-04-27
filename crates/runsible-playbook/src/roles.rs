//! Role/package loading.
//!
//! At M1 a role is a directory under one of these search paths (first match wins):
//!   1. `./packages/<name>/`         (runsible-native)
//!   2. `./roles/<name>/`            (Ansible-compat)
//!   3. `~/.runsible/cache/<name>/`  (galaxy-installed)
//!
//! Each role can supply (per entry point, default `main`):
//!   - `tasks/<entry>.toml`        — tasks injected into the play
//!   - `handlers/<entry>.toml`     — handlers merged into the play
//!   - `defaults/<entry>.toml`     — vars (lowest precedence)
//!   - `vars/<entry>.toml`         — vars (above defaults)
//!
//! Role params (from `[plays.roles.vars]`) override role vars/defaults but
//! not play vars or host vars.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Deserialize;

use crate::errors::{PlaybookError, Result};

#[derive(Debug, Clone, Default)]
pub struct LoadedRole {
    pub name: String,
    pub entry_point: String,
    pub root: PathBuf,
    /// Resolved task TOML values (the contents of tasks/<entry>.toml).
    pub tasks: Vec<toml::Value>,
    /// Handler tables keyed by handler ID.
    pub handlers: IndexMap<String, toml::Value>,
    /// Defaults — lowest-precedence vars.
    pub defaults: IndexMap<String, toml::Value>,
    /// Role vars — override defaults.
    pub vars: IndexMap<String, toml::Value>,
}

/// File shape for `tasks/<entry>.toml` and `handlers/<entry>.toml`:
/// either a top-level `tasks = [...]`/`handlers = {...}` key, or the file is
/// directly the array/table.
#[derive(Debug, Deserialize, Default)]
struct TasksFile {
    #[serde(default)]
    tasks: Vec<toml::Value>,
}

#[derive(Debug, Deserialize, Default)]
struct HandlersFile {
    #[serde(default)]
    handlers: IndexMap<String, toml::Value>,
}

/// Default search paths relative to the project root (the cwd when the engine
/// runs is the project root).
pub fn default_search_paths() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = vec![PathBuf::from("packages"), PathBuf::from("roles")];
    if let Ok(home) = std::env::var("HOME") {
        v.push(PathBuf::from(home).join(".runsible/cache"));
    }
    v
}

pub fn find_role_root(name: &str, search_paths: &[PathBuf]) -> Option<PathBuf> {
    for sp in search_paths {
        let candidate = sp.join(name);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// Load a role by name. Missing files are tolerated (a role doesn't have to
/// have all of tasks/handlers/defaults/vars).
pub fn load_role(name: &str, entry_point: &str, search_paths: &[PathBuf]) -> Result<LoadedRole> {
    let root = find_role_root(name, search_paths).ok_or_else(|| {
        PlaybookError::Parse(format!(
            "role '{name}' not found in search paths: {:?}",
            search_paths
        ))
    })?;

    let tasks = load_tasks_file(&root.join(format!("tasks/{entry_point}.toml")))?;
    let handlers = load_handlers_file(&root.join(format!("handlers/{entry_point}.toml")))?;
    let defaults = load_vars_file(&root.join(format!("defaults/{entry_point}.toml")))?;
    let vars = load_vars_file(&root.join(format!("vars/{entry_point}.toml")))?;

    Ok(LoadedRole {
        name: name.to_string(),
        entry_point: entry_point.to_string(),
        root,
        tasks,
        handlers,
        defaults,
        vars,
    })
}

fn load_tasks_file(path: &Path) -> Result<Vec<toml::Value>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let body = std::fs::read_to_string(path).map_err(PlaybookError::Io)?;
    // Try to parse as `{ tasks = [...] }` first; fall back to bare array.
    if let Ok(parsed) = toml::from_str::<TasksFile>(&body) {
        if !parsed.tasks.is_empty() {
            return Ok(parsed.tasks);
        }
    }
    // Try parsing as a raw value to see if it's an array at the top level.
    let value: toml::Value = toml::from_str(&body)
        .map_err(|e| PlaybookError::Parse(format!("{}: {e}", path.display())))?;
    match value {
        toml::Value::Array(arr) => Ok(arr),
        toml::Value::Table(t) => {
            if let Some(toml::Value::Array(arr)) = t.get("tasks") {
                return Ok(arr.clone());
            }
            // Empty file is fine.
            Ok(Vec::new())
        }
        _ => Err(PlaybookError::Parse(format!(
            "{}: expected an array of tasks or a table with a `tasks` key",
            path.display()
        ))),
    }
}

fn load_handlers_file(path: &Path) -> Result<IndexMap<String, toml::Value>> {
    if !path.exists() {
        return Ok(IndexMap::new());
    }
    let body = std::fs::read_to_string(path).map_err(PlaybookError::Io)?;
    if let Ok(parsed) = toml::from_str::<HandlersFile>(&body) {
        if !parsed.handlers.is_empty() {
            return Ok(parsed.handlers);
        }
    }
    let value: toml::Value = toml::from_str(&body)
        .map_err(|e| PlaybookError::Parse(format!("{}: {e}", path.display())))?;
    if let toml::Value::Table(t) = value {
        // Treat the whole top-level table as handler_id → table.
        let mut out: IndexMap<String, toml::Value> = IndexMap::new();
        for (k, v) in t {
            if k == "handlers" {
                if let toml::Value::Table(inner) = v {
                    for (ik, iv) in inner {
                        out.insert(ik, iv);
                    }
                }
            } else if matches!(v, toml::Value::Table(_)) {
                out.insert(k, v);
            }
        }
        Ok(out)
    } else {
        Ok(IndexMap::new())
    }
}

fn load_vars_file(path: &Path) -> Result<IndexMap<String, toml::Value>> {
    if !path.exists() {
        return Ok(IndexMap::new());
    }
    let body = std::fs::read_to_string(path).map_err(PlaybookError::Io)?;
    let value: toml::Value = toml::from_str(&body)
        .map_err(|e| PlaybookError::Parse(format!("{}: {e}", path.display())))?;
    match value {
        toml::Value::Table(t) => Ok(t.into_iter().collect()),
        _ => Err(PlaybookError::Parse(format!(
            "{}: vars file must be a TOML table",
            path.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_role(dir: &Path, name: &str) {
        let root = dir.join(name);
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(root.join("handlers")).unwrap();
        std::fs::create_dir_all(root.join("defaults")).unwrap();
        std::fs::create_dir_all(root.join("vars")).unwrap();
        std::fs::write(
            root.join("tasks/main.toml"),
            r#"
[[tasks]]
name = "from role"
debug = { msg = "hi from {{ role_var }}" }
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("defaults/main.toml"),
            r#"role_var = "default""#,
        )
        .unwrap();
        std::fs::write(
            root.join("handlers/main.toml"),
            r#"
[restart_app]
debug = { msg = "restarting" }
"#,
        )
        .unwrap();
    }

    #[test]
    fn load_role_finds_files() {
        let tmp = std::env::temp_dir().join(format!("rsl-roles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let pkg_dir = tmp.join("packages");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        make_role(&pkg_dir, "myrole");

        let role = load_role("myrole", "main", &[pkg_dir.clone()]).unwrap();
        assert_eq!(role.name, "myrole");
        assert_eq!(role.tasks.len(), 1);
        assert!(role.handlers.contains_key("restart_app"));
        assert_eq!(role.defaults.get("role_var").and_then(|v| v.as_str()), Some("default"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn missing_role_errors() {
        let r = load_role("does_not_exist", "main", &[]);
        assert!(r.is_err());
    }

    #[test]
    fn missing_files_are_tolerated() {
        let tmp = std::env::temp_dir().join(format!("rsl-roles-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let role_root = tmp.join("packages/empty");
        std::fs::create_dir_all(&role_root).unwrap();
        let role = load_role("empty", "main", &[tmp.join("packages")]).unwrap();
        assert_eq!(role.tasks.len(), 0);
        assert_eq!(role.handlers.len(), 0);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
