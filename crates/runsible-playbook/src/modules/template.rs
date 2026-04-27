//! `runsible_builtin.template` — render a MiniJinja template and write to dest.
//!
//! Args:
//!   src  = "/local/path/to/template.j2"   (file on the controller)
//!   dest = "/remote/path"
//!   mode = "0644"  (octal string, optional)
//!
//! Idempotence: render the template, compare bytes to existing dest file,
//! skip put if identical.

use std::path::Path;

use runsible_core::traits::ExecutionContext;
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};
use crate::templating::Templater;

pub struct TemplateModule;

impl DynModule for TemplateModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.template"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let src = args
            .get("src")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PlaybookError::TypeCheck("template: missing required arg `src`".into()))?
            .to_string();
        let dest = args
            .get("dest")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                PlaybookError::TypeCheck("template: missing required arg `dest`".into())
            })?
            .to_string();
        let mode = args.get("mode").and_then(|v| v.as_str()).map(String::from);

        // Read the template source from disk (controller side).
        let raw = std::fs::read_to_string(&src).map_err(|e| PlaybookError::ExecFailed {
            host: ctx.host.name.clone(),
            message: format!("template: cannot read src {src}: {e}"),
        })?;

        let templater = Templater::new();
        let rendered = templater.render_str(&raw, ctx.vars).map_err(|e| {
            PlaybookError::ExecFailed {
                host: ctx.host.name.clone(),
                message: format!("template render error: {e}"),
            }
        })?;
        let desired = rendered.as_bytes().to_vec();

        let mut will_change = true;
        let mut current_bytes: Option<Vec<u8>> = None;
        if ctx.connection.file_exists(Path::new(&dest)).unwrap_or(false) {
            if let Ok(current) = ctx.connection.slurp(Path::new(&dest)) {
                if current == desired {
                    will_change = false;
                }
                current_bytes = Some(current);
            }
        }

        let mut diff = serde_json::json!({
            "src": src,
            "dest": dest,
            "mode": mode,
            "rendered": rendered,
            "size_bytes": desired.len(),
        });
        if ctx.diff_mode {
            let before = current_bytes
                .as_deref()
                .map(bytes_to_diff_string)
                .unwrap_or_default();
            let after = bytes_to_diff_string(&desired);
            if let Some(obj) = diff.as_object_mut() {
                obj.insert("before".into(), serde_json::Value::String(before));
                obj.insert("after".into(), serde_json::Value::String(after));
            }
        }

        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff,
            will_change,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        if !plan.will_change {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Ok,
                elapsed_ms: 0,
                returns: serde_json::json!({"changed": false, "dest": plan.diff["dest"]}),
            });
        }

        let dest = plan.diff.get("dest").and_then(|v| v.as_str()).unwrap_or("");
        let rendered = plan
            .diff
            .get("rendered")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let mode = plan
            .diff
            .get("mode")
            .and_then(|v| v.as_str())
            .and_then(|s| u32::from_str_radix(s.trim_start_matches('0'), 8).ok());

        let started = std::time::Instant::now();

        // Stage to temp file then put_file via the connection.
        let tmp = std::env::temp_dir().join(format!(
            "runsible-template-{}.tmp",
            std::process::id()
        ));
        std::fs::write(&tmp, rendered).map_err(|e| PlaybookError::ExecFailed {
            host: ctx.host.name.clone(),
            message: e.to_string(),
        })?;
        let r = ctx.connection.put_file(&tmp, Path::new(dest), mode);
        let _ = std::fs::remove_file(&tmp);

        r.map_err(|e| PlaybookError::ExecFailed {
            host: ctx.host.name.clone(),
            message: e.to_string(),
        })?;

        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Changed,
            elapsed_ms: started.elapsed().as_millis() as u64,
            returns: serde_json::json!({
                "changed": true,
                "dest": dest,
                "mode": plan.diff["mode"],
            }),
        })
    }
}

/// Decode bytes for diff display. UTF-8 strings pass through; other bytes
/// produce the placeholder `"<binary>"`.
fn bytes_to_diff_string(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => "<binary>".to_string(),
    }
}
