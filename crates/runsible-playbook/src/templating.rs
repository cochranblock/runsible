//! MiniJinja-backed templating for variable interpolation in task arguments.
//!
//! The [`Templater`] wraps a `minijinja::Environment` and exposes three
//! convenience methods used by the engine:
//!
//!   * [`Templater::render_str`] — render a single string template.
//!   * [`Templater::render_value`] — recursively render every string scalar
//!     inside a `toml::Value`. Tables and arrays traverse; non-string scalars
//!     pass through unchanged.
//!   * [`Templater::eval_bool`] — evaluate a Jinja expression as a boolean.
//!     Used for `when` conditions like `dl.rc == 0` or
//!     `'OK' in result.stdout`.
//!
//! The `Vars` type from `runsible_core` is `BTreeMap<String, toml::Value>`;
//! values are converted to `minijinja::Value` via [`toml_to_jvalue`] which
//! walks the TOML tree.

use std::collections::BTreeMap;

use minijinja::{Environment, UndefinedBehavior, Value as JValue};
use runsible_core::types::Vars;

use crate::errors::{PlaybookError, Result};

/// Wraps a configured `minijinja::Environment` plus helpers for rendering
/// task arguments and evaluating `when` expressions.
pub struct Templater {
    env: Environment<'static>,
}

impl Default for Templater {
    fn default() -> Self {
        Self::new()
    }
}

impl Templater {
    /// Construct a new templater with a default `minijinja::Environment`.
    ///
    /// Configures strict undefined behavior so referencing an unknown
    /// variable raises an error rather than silently rendering as empty.
    pub fn new() -> Self {
        let mut env = Environment::new();
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        // Preserve a trailing newline in template sources. Jinja's default is
        // to swallow one final newline; for the `template` module that means a
        // file ending in `\n` would round-trip without it, breaking byte-equal
        // idempotence and producing files without terminators. Keep it.
        env.set_keep_trailing_newline(true);
        Self { env }
    }

    /// Render a string template against `vars`.
    ///
    /// Fast path: if `src` contains no `{{` and no `{%`, return it untouched
    /// without invoking the template engine.
    pub fn render_str(&self, src: &str, vars: &Vars) -> Result<String> {
        if !src.contains("{{") && !src.contains("{%") {
            return Ok(src.to_string());
        }
        let ctx = vars_to_jvalue(vars);
        self.env
            .render_str(src, ctx)
            .map_err(|e| PlaybookError::TemplateError(format!("{e:#}")))
    }

    /// Recursively render every string scalar inside a `toml::Value`.
    ///
    /// Tables and arrays traverse; other scalars (integers, floats, bools,
    /// datetimes) pass through unchanged.
    pub fn render_value(&self, value: &toml::Value, vars: &Vars) -> Result<toml::Value> {
        match value {
            toml::Value::String(s) => Ok(toml::Value::String(self.render_str(s, vars)?)),
            toml::Value::Table(tbl) => {
                let mut out = toml::map::Map::with_capacity(tbl.len());
                for (k, v) in tbl {
                    out.insert(k.clone(), self.render_value(v, vars)?);
                }
                Ok(toml::Value::Table(out))
            }
            toml::Value::Array(arr) => {
                let mut out = Vec::with_capacity(arr.len());
                for v in arr {
                    out.push(self.render_value(v, vars)?);
                }
                Ok(toml::Value::Array(out))
            }
            other => Ok(other.clone()),
        }
    }

    /// Evaluate a Jinja expression as a boolean.
    ///
    /// Used for `when` conditions: `dl.rc == 0`, `'OK' in result.stdout`,
    /// `x == 1 and y == 2`, etc.
    pub fn eval_bool(&self, expr: &str, vars: &Vars) -> Result<bool> {
        let compiled = self
            .env
            .compile_expression(expr)
            .map_err(|e| PlaybookError::TemplateError(format!("{e:#}")))?;
        let ctx = vars_to_jvalue(vars);
        let result = compiled
            .eval(ctx)
            .map_err(|e| PlaybookError::TemplateError(format!("{e:#}")))?;
        Ok(result.is_true())
    }
}

/// Convert the engine `Vars` map into a `minijinja::Value::from_object`-style
/// map by walking each entry.
fn vars_to_jvalue(vars: &Vars) -> JValue {
    let mut map: BTreeMap<String, JValue> = BTreeMap::new();
    for (k, v) in vars {
        map.insert(k.clone(), toml_to_jvalue(v));
    }
    JValue::from_serialize(&map)
}

/// Convert a `toml::Value` into the corresponding `minijinja::Value`.
///
/// Tables become maps, arrays become sequences, datetimes degrade to their
/// RFC-3339 string form (Jinja has no native datetime concept).
fn toml_to_jvalue(value: &toml::Value) -> JValue {
    match value {
        toml::Value::String(s) => JValue::from(s.as_str()),
        toml::Value::Integer(i) => JValue::from(*i),
        toml::Value::Float(f) => JValue::from(*f),
        toml::Value::Boolean(b) => JValue::from(*b),
        toml::Value::Datetime(dt) => JValue::from(dt.to_string()),
        toml::Value::Array(arr) => {
            let items: Vec<JValue> = arr.iter().map(toml_to_jvalue).collect();
            JValue::from(items)
        }
        toml::Value::Table(tbl) => {
            let mut map: BTreeMap<String, JValue> = BTreeMap::new();
            for (k, v) in tbl {
                map.insert(k.clone(), toml_to_jvalue(v));
            }
            JValue::from_serialize(&map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars_from_toml(src: &str) -> Vars {
        let v: toml::Value = toml::from_str(src).expect("toml parse");
        let tbl = v.as_table().expect("top-level table");
        tbl.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    #[test]
    fn render_simple_var() {
        let t = Templater::new();
        let vars = vars_from_toml(r#"name = "alice""#);
        let out = t.render_str("{{ name }}", &vars).unwrap();
        assert_eq!(out, "alice");
    }

    #[test]
    fn render_no_template_passthrough() {
        let t = Templater::new();
        let vars = Vars::new();
        let out = t.render_str("no template here", &vars).unwrap();
        assert_eq!(out, "no template here");
    }

    #[test]
    fn render_nested_table() {
        let t = Templater::new();
        let vars = vars_from_toml(r#"name = "alice""#);
        let value: toml::Value = toml::from_str(
            r#"
msg = "hi {{ name }}"
port = 80
"#,
        )
        .unwrap();
        let rendered = t.render_value(&value, &vars).unwrap();
        let tbl = rendered.as_table().unwrap();
        assert_eq!(tbl["msg"].as_str().unwrap(), "hi alice");
        assert_eq!(tbl["port"].as_integer().unwrap(), 80);
    }

    #[test]
    fn render_array_of_strings() {
        let t = Templater::new();
        let vars = vars_from_toml(
            r#"
a = "1"
b = "2"
"#,
        );
        let value: toml::Value = toml::from_str(r#"items = ["{{ a }}", "{{ b }}"]"#).unwrap();
        let rendered = t.render_value(&value, &vars).unwrap();
        let arr = rendered.as_table().unwrap()["items"].as_array().unwrap();
        assert_eq!(arr[0].as_str().unwrap(), "1");
        assert_eq!(arr[1].as_str().unwrap(), "2");
    }

    #[test]
    fn eval_bool_true() {
        let t = Templater::new();
        let vars = vars_from_toml("x = 1");
        assert!(t.eval_bool("x == 1", &vars).unwrap());
    }

    #[test]
    fn eval_bool_false() {
        let t = Templater::new();
        let vars = vars_from_toml("x = 2");
        assert!(!t.eval_bool("x == 1", &vars).unwrap());
    }

    #[test]
    fn eval_bool_and() {
        let t = Templater::new();
        let vars = vars_from_toml(
            r#"
x = 1
y = 2
"#,
        );
        assert!(t.eval_bool("x == 1 and y == 2", &vars).unwrap());
    }

    #[test]
    fn eval_bool_in_string() {
        let t = Templater::new();
        let vars = vars_from_toml(r#"s = "foobar""#);
        assert!(t.eval_bool("'foo' in s", &vars).unwrap());
    }

    #[test]
    fn missing_var_errors() {
        let t = Templater::new();
        let vars = Vars::new();
        let err = t.render_str("{{ undefined }}", &vars).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("undefined"),
            "expected error to mention 'undefined', got: {msg}"
        );
    }

    #[test]
    fn render_int_var() {
        let t = Templater::new();
        let vars = vars_from_toml("port = 8080");
        let out = t.render_str("{{ port }}", &vars).unwrap();
        assert_eq!(out, "8080");
    }

    #[test]
    fn render_bool_var() {
        let t = Templater::new();
        let vars = vars_from_toml("enabled = true");
        let out = t.render_str("{{ enabled }}", &vars).unwrap();
        assert_eq!(out, "true");
    }

    #[test]
    fn render_float_var() {
        let t = Templater::new();
        let vars = vars_from_toml("ratio = 1.5");
        let out = t.render_str("{{ ratio }}", &vars).unwrap();
        assert!(out.starts_with("1.5"));
    }

    #[test]
    fn render_nested_table_access() {
        let t = Templater::new();
        let vars = vars_from_toml(
            r#"
[user]
name = "alice"
age = 30
"#,
        );
        let out = t.render_str("{{ user.name }}", &vars).unwrap();
        assert_eq!(out, "alice");
    }

    #[test]
    fn render_array_index() {
        let t = Templater::new();
        let vars = vars_from_toml(r#"items = ["a", "b", "c"]"#);
        let out = t.render_str("{{ items[0] }}", &vars).unwrap();
        assert_eq!(out, "a");
        let out2 = t.render_str("{{ items[2] }}", &vars).unwrap();
        assert_eq!(out2, "c");
    }

    #[test]
    fn render_default_filter() {
        let t = Templater::new();
        let vars = Vars::new();
        let out = t
            .render_str("{{ missing | default('x') }}", &vars)
            .unwrap();
        assert_eq!(out, "x");
    }

    #[test]
    fn render_length_filter_on_array() {
        let t = Templater::new();
        let vars = vars_from_toml(r#"items = ["a", "b", "c"]"#);
        let out = t.render_str("{{ items | length }}", &vars).unwrap();
        assert_eq!(out, "3");
    }

    #[test]
    fn render_upper_filter() {
        let t = Templater::new();
        let vars = vars_from_toml(r#"who = "alice""#);
        let out = t.render_str("{{ who | upper }}", &vars).unwrap();
        assert_eq!(out, "ALICE");
    }

    #[test]
    fn render_lower_filter() {
        let t = Templater::new();
        let vars = vars_from_toml(r#"who = "ALICE""#);
        let out = t.render_str("{{ who | lower }}", &vars).unwrap();
        assert_eq!(out, "alice");
    }

    #[test]
    fn eval_bool_parenthesized() {
        let t = Templater::new();
        let vars = vars_from_toml(
            r#"
a = 1
b = 2
c = 3
"#,
        );
        assert!(t.eval_bool("(a == 1 and b == 2) or c == 99", &vars).unwrap());
        assert!(t.eval_bool("a == 1 and (b == 2 or c == 99)", &vars).unwrap());
    }

    #[test]
    fn eval_bool_not_operator() {
        let t = Templater::new();
        let vars = vars_from_toml("x = 5");
        assert!(t.eval_bool("not (x == 1)", &vars).unwrap());
        assert!(!t.eval_bool("not (x == 5)", &vars).unwrap());
    }

    #[test]
    fn render_value_array_of_templates() {
        let t = Templater::new();
        let vars = vars_from_toml(
            r#"
a = "first"
b = "second"
"#,
        );
        let raw: toml::Value =
            toml::Value::Array(vec![
                toml::Value::String("{{ a }}".into()),
                toml::Value::String("{{ b }}".into()),
            ]);
        let rendered = t.render_value(&raw, &vars).unwrap();
        let arr = rendered.as_array().unwrap();
        assert_eq!(arr[0].as_str().unwrap(), "first");
        assert_eq!(arr[1].as_str().unwrap(), "second");
    }
}
