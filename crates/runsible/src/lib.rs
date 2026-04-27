//! runsible — ad-hoc module runner.
//!
//! Core library: args parsing, synthetic playbook construction, and exit-code
//! mapping.  The `main` binary is a thin CLI wrapper around this.

pub use runsible_playbook::{RunResult, run};

// ── Args parsing ─────────────────────────────────────────────────────────────

/// Parse a `-a` / `--args` string into a `toml::Value::Table`.
///
/// Two forms are accepted:
/// - JSON object: `{"key":"val"}` — parsed via serde_json, then converted.
/// - Space-separated k=v pairs: `msg=hello debug=true`.
/// - Empty / missing: returns an empty table.
pub fn parse_args(raw: &str) -> anyhow::Result<toml::Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(toml::Value::Table(toml::map::Map::new()));
    }

    // Try JSON first (starts with '{').
    if trimmed.starts_with('{') {
        let json: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|e| anyhow::anyhow!("invalid JSON args: {e}"))?;
        return json_to_toml(json);
    }

    // k=v pairs.
    let mut table = toml::map::Map::new();
    for token in trimmed.split_whitespace() {
        let (k, v) = token
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("invalid arg token (expected key=val): {token}"))?;
        table.insert(k.to_string(), toml::Value::String(v.to_string()));
    }
    Ok(toml::Value::Table(table))
}

/// Convert a JSON value to an equivalent toml::Value.
fn json_to_toml(v: serde_json::Value) -> anyhow::Result<toml::Value> {
    match v {
        serde_json::Value::Null => Ok(toml::Value::String(String::new())),
        serde_json::Value::Bool(b) => Ok(toml::Value::Boolean(b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(toml::Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(toml::Value::Float(f))
            } else {
                Err(anyhow::anyhow!("unrepresentable number: {n}"))
            }
        }
        serde_json::Value::String(s) => Ok(toml::Value::String(s)),
        serde_json::Value::Array(arr) => {
            let items: anyhow::Result<Vec<_>> = arr.into_iter().map(json_to_toml).collect();
            Ok(toml::Value::Array(items?))
        }
        serde_json::Value::Object(obj) => {
            let mut table = toml::map::Map::new();
            for (k, val) in obj {
                table.insert(k, json_to_toml(val)?);
            }
            Ok(toml::Value::Table(table))
        }
    }
}

// ── Synthetic playbook builder ────────────────────────────────────────────────

/// Build a single-task synthetic TOML playbook string.
///
/// `module_alias` is the last dot-separated segment of `module_name`
/// (e.g. `ping` from `runsible_builtin.ping`).
///
/// `args_value` is a `toml::Value::Table` that will be serialised inline.
pub fn build_synthetic_playbook(
    pattern: &str,
    module_name: &str,
    args_value: &toml::Value,
) -> anyhow::Result<String> {
    let alias = module_name
        .rsplit('.')
        .next()
        .unwrap_or(module_name)
        .to_string();

    // Serialise args as an inline TOML table: `{ key = "val", ... }`.
    // Empty table → `{}`.
    let args_inline = match args_value {
        toml::Value::Table(t) if t.is_empty() => "{}".to_string(),
        _ => {
            // toml::to_string gives us a multi-line document.  We need a single
            // inline value, so we serialize each key manually for the simple
            // string-only case, or fall back to `{}` for empty.
            // For M0 all values are strings from k=v parsing, so this suffices.
            let pairs: Vec<String> = if let toml::Value::Table(t) = args_value {
                t.iter()
                    .map(|(k, v)| {
                        let vs = match v {
                            toml::Value::String(s) => format!("\"{s}\""),
                            toml::Value::Boolean(b) => b.to_string(),
                            toml::Value::Integer(i) => i.to_string(),
                            toml::Value::Float(f) => f.to_string(),
                            other => format!("\"{other}\""),
                        };
                        format!("{k} = {vs}")
                    })
                    .collect()
            } else {
                vec![]
            };
            if pairs.is_empty() {
                "{}".to_string()
            } else {
                format!("{{ {} }}", pairs.join(", "))
            }
        }
    };

    Ok(format!(
        r#"schema = "runsible.playbook.v1"

[imports]
{alias} = "{module_name}"

[[plays]]
name = "ad-hoc"
hosts = "{pattern}"

[[plays.tasks]]
name = "ad-hoc task"
{alias} = {args_inline}
"#
    ))
}

// ── Exit-code helpers ─────────────────────────────────────────────────────────

/// Map a `RunResult` to an Ansible-style exit code.
///
/// 0 = all ok, 2 = host failures.
pub fn exit_code(result: &RunResult) -> i32 {
    result.exit_code()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use runsible_core::types::{Host, Vars};
    use runsible_playbook::catalog::DynModule;
    use runsible_playbook::modules::ping::PingModule;

    // 1. ping module — plan and apply
    #[test]
    fn ping_module_plan_apply() {
        use runsible_core::traits::ExecutionContext;
        let host = Host { name: "localhost".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = ExecutionContext { host: &host, vars: &vars, connection: &conn, check_mode: false };
        let module = PingModule;
        let args = toml::Value::Table(toml::map::Map::new());

        let plan = DynModule::plan(&module, &args, &ctx).unwrap();
        assert!(!plan.will_change);
        assert_eq!(plan.diff["ping"], "pong");

        let outcome = DynModule::apply(&module, &plan, &ctx).unwrap();
        assert_eq!(outcome.returns["ping"], "pong");
        assert_eq!(outcome.status, runsible_core::types::OutcomeStatus::Ok);
    }

    // 2. args parsing — k=v pairs
    #[test]
    fn args_parse_kv() {
        let table = parse_args("msg=hello debug=true").unwrap();
        assert_eq!(
            table.get("msg").and_then(|v| v.as_str()),
            Some("hello")
        );
        assert_eq!(
            table.get("debug").and_then(|v| v.as_str()),
            Some("true")
        );
    }

    // 3. args parsing — JSON object
    #[test]
    fn args_parse_json() {
        let table = parse_args(r#"{"msg":"hello"}"#).unwrap();
        assert_eq!(
            table.get("msg").and_then(|v| v.as_str()),
            Some("hello")
        );
    }

    // 4. build_synthetic_playbook round-trip
    #[test]
    fn build_synthetic_playbook_test() {
        let args = toml::Value::Table(toml::map::Map::new());
        let src = build_synthetic_playbook("all", "runsible_builtin.ping", &args).unwrap();
        // Must parse cleanly as TOML.
        let doc: toml::Value = toml::from_str(&src).expect("synthetic playbook must be valid TOML");
        // Spot-check the schema key.
        assert_eq!(
            doc.get("schema").and_then(|v| v.as_str()),
            Some("runsible.playbook.v1")
        );
    }

    // 5. full integration — run ping against localhost
    #[test]
    fn run_ping_localhost() {
        let args = toml::Value::Table(toml::map::Map::new());
        let src = build_synthetic_playbook("all", "runsible_builtin.ping", &args).unwrap();
        let result = run(&src, "localhost,", "test").unwrap();
        assert_eq!(result.ok, 1);
        assert_eq!(result.failed, 0);
        assert_eq!(result.exit_code(), 0);
    }
}
