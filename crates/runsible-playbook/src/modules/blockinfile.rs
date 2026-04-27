//! `runsible_builtin.blockinfile` — manage a marker-delimited block in a file.
//!
//! Args:
//!   path   = "/etc/somefile"  (required)
//!   block  = "multi\nline\nstring"
//!   marker = "# {mark} ANSIBLE MANAGED BLOCK"  (default; `{mark}` substituted)
//!   state  = "present" | "absent"   (default "present")
//!   create = true | false           (default false)

use std::path::Path;

use runsible_core::traits::ExecutionContext;
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};

pub struct BlockInFileModule;

const DEFAULT_MARKER: &str = "# {mark} ANSIBLE MANAGED BLOCK";

impl DynModule for BlockInFileModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.blockinfile"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                PlaybookError::TypeCheck("blockinfile: missing required arg `path`".into())
            })?
            .to_string();
        let state = args
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("present")
            .to_string();
        if state != "present" && state != "absent" {
            return Err(PlaybookError::TypeCheck(format!(
                "blockinfile: unknown state '{state}'"
            )));
        }
        let block = args
            .get("block")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let marker = args
            .get("marker")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_MARKER)
            .to_string();
        let create = args.get("create").and_then(|v| v.as_bool()).unwrap_or(false);

        let exists = ctx.connection.file_exists(Path::new(&path)).unwrap_or(false);
        let current = if exists {
            ctx.connection
                .slurp(Path::new(&path))
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .unwrap_or_default()
        } else {
            String::new()
        };

        let begin = marker.replace("{mark}", "BEGIN");
        let end = marker.replace("{mark}", "END");

        let new_content = compute_new(&current, &state, &block, &begin, &end);

        let will_change = if !exists {
            create && new_content != current
        } else {
            new_content != current
        };

        let mut diff = serde_json::json!({
            "path": path,
            "state": state,
            "block": block,
            "marker": marker,
            "begin": begin,
            "end": end,
            "create": create,
            "exists": exists,
            "new_content": new_content,
        });
        if ctx.diff_mode {
            if let Some(obj) = diff.as_object_mut() {
                obj.insert("before".into(), serde_json::Value::String(current.clone()));
                obj.insert("after".into(), serde_json::Value::String(new_content.clone()));
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
                returns: serde_json::json!({"changed": false, "path": plan.diff["path"]}),
            });
        }

        let started = std::time::Instant::now();
        let path = plan.diff.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let exists = plan.diff.get("exists").and_then(|v| v.as_bool()).unwrap_or(false);
        let create = plan.diff.get("create").and_then(|v| v.as_bool()).unwrap_or(false);
        let new_content = plan
            .diff
            .get("new_content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if !exists && !create {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Failed,
                elapsed_ms: started.elapsed().as_millis() as u64,
                returns: serde_json::json!({
                    "stage": "missing_file",
                    "msg": "file does not exist and create=false",
                }),
            });
        }

        let tmp = std::env::temp_dir().join(format!(
            "runsible-blockinfile-{}.tmp",
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

fn compute_new(current: &str, state: &str, block: &str, begin: &str, end: &str) -> String {
    // Strip any existing block first.
    let stripped = strip_block(current, begin, end);
    if state == "absent" {
        return stripped;
    }
    // state == "present"
    let mut out = stripped;
    let needs_leading_newline = !out.is_empty() && !out.ends_with('\n');
    if needs_leading_newline {
        out.push('\n');
    }
    out.push_str(begin);
    out.push('\n');
    out.push_str(block);
    if !block.ends_with('\n') && !block.is_empty() {
        out.push('\n');
    } else if block.is_empty() {
        // empty block — still want a newline before END
    }
    out.push_str(end);
    out.push('\n');
    out
}

fn strip_block(current: &str, begin: &str, end: &str) -> String {
    let begin_idx = match current.find(begin) {
        Some(i) => i,
        None => return current.to_string(),
    };
    let after_begin = begin_idx + begin.len();
    let end_idx = match current[after_begin..].find(end) {
        Some(i) => after_begin + i,
        None => return current.to_string(),
    };
    let after_end = end_idx + end.len();
    let mut start = begin_idx;
    // Drop the newline immediately preceding BEGIN if present.
    if start > 0 && current.as_bytes()[start - 1] == b'\n' {
        start -= 1;
    }
    // Skip the trailing newline after END marker if present.
    let mut tail = after_end;
    if tail < current.len() && current.as_bytes()[tail] == b'\n' {
        tail += 1;
    }
    let mut out = String::with_capacity(current.len());
    out.push_str(&current[..start]);
    out.push_str(&current[tail..]);
    out
}
