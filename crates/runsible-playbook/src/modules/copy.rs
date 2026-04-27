//! `runsible_builtin.copy` — copy a file to the remote host.
//!
//! Args:
//!   src     = "/local/path"   (file on the controller)
//!   content = "literal string contents"   (alternative to src)
//!   dest    = "/remote/path"
//!   mode    = "0644"  (octal string, optional)
//!
//! Idempotence: if dest exists and its bytes match src/content, will_change=false.

use std::path::Path;

use runsible_core::traits::ExecutionContext;
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};

pub struct CopyModule;

impl DynModule for CopyModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.copy"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let dest = args
            .get("dest")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PlaybookError::TypeCheck("copy: missing required arg `dest`".into()))?
            .to_string();
        let src = args.get("src").and_then(|v| v.as_str()).map(String::from);
        let content = args.get("content").and_then(|v| v.as_str()).map(String::from);
        let mode = args.get("mode").and_then(|v| v.as_str()).map(String::from);

        if src.is_none() && content.is_none() {
            return Err(PlaybookError::TypeCheck(
                "copy: must provide either `src` or `content`".into(),
            ));
        }

        // Resolve desired payload bytes for idempotence check.
        let desired: Vec<u8> = if let Some(s) = &src {
            std::fs::read(s).map_err(|e| {
                PlaybookError::ExecFailed {
                    host: ctx.host.name.clone(),
                    message: format!("copy: cannot read src {s}: {e}"),
                }
            })?
        } else {
            content.as_deref().unwrap_or("").as_bytes().to_vec()
        };

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
            "content": content,
            "dest": dest,
            "mode": mode,
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
        let src = plan.diff.get("src").and_then(|v| v.as_str());
        let content = plan.diff.get("content").and_then(|v| v.as_str());
        let mode = plan
            .diff
            .get("mode")
            .and_then(|v| v.as_str())
            .and_then(|s| u32::from_str_radix(s.trim_start_matches('0'), 8).ok());

        let started = std::time::Instant::now();

        let result = if let Some(src_path) = src {
            ctx.connection.put_file(Path::new(src_path), Path::new(dest), mode)
        } else {
            // Write content to a temp file, then put_file it.
            let tmp = std::env::temp_dir().join(format!(
                "runsible-copy-{}.tmp",
                std::process::id()
            ));
            std::fs::write(&tmp, content.unwrap_or(""))
                .map_err(|e| PlaybookError::ExecFailed {
                    host: ctx.host.name.clone(),
                    message: e.to_string(),
                })?;
            let r = ctx.connection.put_file(&tmp, Path::new(dest), mode);
            let _ = std::fs::remove_file(&tmp);
            r
        };

        result.map_err(|e| PlaybookError::ExecFailed {
            host: ctx.host.name.clone(),
            message: e.to_string(),
        })?;

        let elapsed_ms = started.elapsed().as_millis() as u64;

        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Changed,
            elapsed_ms,
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
