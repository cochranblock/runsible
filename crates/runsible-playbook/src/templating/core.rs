//! Core MiniJinja-backed templating for variable interpolation in task arguments.
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

use super::filters::register_filters_and_tests;
use super::lookups::register_lookups;

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
        register_filters_and_tests(&mut env);
        register_lookups(&mut env);
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
pub(crate) fn vars_to_jvalue(vars: &Vars) -> JValue {
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
pub(crate) fn toml_to_jvalue(value: &toml::Value) -> JValue {
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
