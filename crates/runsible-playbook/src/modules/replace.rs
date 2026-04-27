//! `runsible_builtin.replace` — substring/regex replace within a file.
//!
//! Args:
//!   path    = "/etc/somefile"  (required)
//!   regexp  = "pattern"        (required)
//!   replace = "replacement"    (required)
//!   before  = "anchor"         (optional — only replace text before this)
//!   after   = "anchor"         (optional — only replace text after this)
//!
//! Idempotence: if running the substitution again produces no change → not
//! changed. We compute new content in plan().

use std::path::Path;

use runsible_core::traits::ExecutionContext;
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};

pub struct ReplaceModule;

impl DynModule for ReplaceModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.replace"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                PlaybookError::TypeCheck("replace: missing required arg `path`".into())
            })?
            .to_string();
        let regexp = args
            .get("regexp")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                PlaybookError::TypeCheck("replace: missing required arg `regexp`".into())
            })?
            .to_string();
        let replacement = args
            .get("replace")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let before = args.get("before").and_then(|v| v.as_str()).map(String::from);
        let after = args.get("after").and_then(|v| v.as_str()).map(String::from);

        let exists = ctx.connection.file_exists(Path::new(&path)).unwrap_or(false);
        if !exists {
            return Ok(Plan {
                module: self.module_name().into(),
                host: ctx.host.name.clone(),
                diff: serde_json::json!({
                    "path": path,
                    "exists": false,
                    "new_content": "",
                }),
                will_change: false,
            });
        }
        let current = ctx
            .connection
            .slurp(Path::new(&path))
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();

        let new = apply_replace(&current, &regexp, &replacement, after.as_deref(), before.as_deref());

        let mut diff = serde_json::json!({
            "path": path,
            "regexp": regexp,
            "replace": replacement,
            "before": before,
            "after": after,
            "exists": true,
            "new_content": new.clone(),
        });
        if ctx.diff_mode {
            // diff_mode overloads `before`/`after` to mean file content. Keep
            // the anchor patterns under explicit names so they're still usable.
            if let Some(obj) = diff.as_object_mut() {
                let prev_before = obj
                    .insert("before".into(), serde_json::Value::String(current.clone()))
                    .unwrap_or(serde_json::Value::Null);
                let prev_after = obj
                    .insert("after".into(), serde_json::Value::String(new.clone()))
                    .unwrap_or(serde_json::Value::Null);
                obj.insert("before_anchor".into(), prev_before);
                obj.insert("after_anchor".into(), prev_after);
            }
        }

        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff,
            will_change: new != current,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        if !plan.will_change {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Ok,
                elapsed_ms: 0,
                returns: serde_json::json!({"changed": false, "path": plan.diff["path"]}),
            });
        }
        let started = std::time::Instant::now();
        let path = plan.diff.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let new_content = plan
            .diff
            .get("new_content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let tmp = std::env::temp_dir().join(format!(
            "runsible-replace-{}.tmp",
            std::process::id()
        ));
        if let Err(e) = std::fs::write(&tmp, &new_content) {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Failed,
                elapsed_ms: started.elapsed().as_millis() as u64,
                returns: serde_json::json!({"stage": "stage_tmp", "msg": e.to_string()}),
            });
        }
        let put = ctx.connection.put_file(&tmp, Path::new(path), None);
        let _ = std::fs::remove_file(&tmp);
        if let Err(e) = put {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Failed,
                elapsed_ms: started.elapsed().as_millis() as u64,
                returns: serde_json::json!({"stage": "put_file", "msg": e.to_string()}),
            });
        }

        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Changed,
            elapsed_ms: started.elapsed().as_millis() as u64,
            returns: serde_json::json!({"changed": true, "path": path}),
        })
    }
}

/// Apply a substring substitution across the entire file (or only the
/// region after `after` and before `before`, when supplied). M1 simplification:
/// we treat `regexp` as a literal substring. TODO_M2: swap to regex crate.
fn apply_replace(
    src: &str,
    regexp: &str,
    replacement: &str,
    after: Option<&str>,
    before: Option<&str>,
) -> String {
    if regexp.is_empty() {
        return src.to_string();
    }
    let start = match after {
        Some(a) => match src.find(a) {
            Some(i) => i + a.len(),
            None => return src.to_string(),
        },
        None => 0,
    };
    let end = match before {
        Some(b) => match src[start..].find(b) {
            Some(i) => start + i,
            None => return src.to_string(),
        },
        None => src.len(),
    };
    let head = &src[..start];
    let middle = &src[start..end];
    let tail = &src[end..];
    let new_middle = middle.replace(regexp, replacement);
    let mut out = String::with_capacity(head.len() + new_middle.len() + tail.len());
    out.push_str(head);
    out.push_str(&new_middle);
    out.push_str(tail);
    out
}
