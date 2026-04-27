//! `runsible_builtin.lineinfile` — ensure a particular line is in a file.
//!
//! Args:
//!   path         = "/etc/somefile"   (required)
//!   line         = "the desired line"
//!   regexp       = "^pattern"        (optional)
//!   state        = "present" | "absent"   (default "present")
//!   insertbefore = "BOF" or regex    (optional)
//!   insertafter  = "EOF" or regex    (optional)
//!   create       = true | false      (default false)
//!   backup       = true | false      (M1: ignored)
//!
//! Idempotence: read the file, decide whether the desired state is already
//! satisfied; if so will_change=false.

use std::path::Path;

use runsible_core::traits::ExecutionContext;
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};

pub struct LineInFileModule;

impl DynModule for LineInFileModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.lineinfile"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                PlaybookError::TypeCheck("lineinfile: missing required arg `path`".into())
            })?
            .to_string();
        let state = args
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("present")
            .to_string();
        let line = args.get("line").and_then(|v| v.as_str()).map(String::from);
        let regexp = args.get("regexp").and_then(|v| v.as_str()).map(String::from);
        let insertbefore = args
            .get("insertbefore")
            .and_then(|v| v.as_str())
            .map(String::from);
        let insertafter = args
            .get("insertafter")
            .and_then(|v| v.as_str())
            .map(String::from);
        let create = args.get("create").and_then(|v| v.as_bool()).unwrap_or(false);
        let _backup = args.get("backup").and_then(|v| v.as_bool()).unwrap_or(false);

        if state == "present" && line.is_none() {
            return Err(PlaybookError::TypeCheck(
                "lineinfile: `line` is required when state=present".into(),
            ));
        }
        if state == "absent" && line.is_none() && regexp.is_none() {
            return Err(PlaybookError::TypeCheck(
                "lineinfile: state=absent requires `line` or `regexp`".into(),
            ));
        }
        if state != "present" && state != "absent" {
            return Err(PlaybookError::TypeCheck(format!(
                "lineinfile: unknown state '{state}'"
            )));
        }

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

        // Decide whether the desired state is already satisfied.
        let new_content = compute_new_content(
            &current,
            &state,
            line.as_deref(),
            regexp.as_deref(),
            insertbefore.as_deref(),
            insertafter.as_deref(),
        )?;

        let will_change = if !exists {
            // need to create
            create && new_content != current
        } else {
            new_content != current
        };

        let mut diff = serde_json::json!({
            "path": path,
            "state": state,
            "line": line,
            "regexp": regexp,
            "insertbefore": insertbefore,
            "insertafter": insertafter,
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
                    "path": path,
                    "msg": "file does not exist and create=false",
                }),
            });
        }

        // Stage to a tmp file, then put_file.
        let tmp = std::env::temp_dir().join(format!(
            "runsible-lineinfile-{}.tmp",
            std::process::id()
        ));
        if let Err(e) = std::fs::write(&tmp, &new_content) {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Failed,
                elapsed_ms: started.elapsed().as_millis() as u64,
                returns: serde_json::json!({
                    "stage": "stage_tmp",
                    "msg": e.to_string(),
                }),
            });
        }
        let put = ctx
            .connection
            .put_file(&tmp, Path::new(path), None);
        let _ = std::fs::remove_file(&tmp);
        if let Err(e) = put {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Failed,
                elapsed_ms: started.elapsed().as_millis() as u64,
                returns: serde_json::json!({
                    "stage": "put_file",
                    "msg": e.to_string(),
                }),
            });
        }

        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Changed,
            elapsed_ms: started.elapsed().as_millis() as u64,
            returns: serde_json::json!({
                "changed": true,
                "path": path,
            }),
        })
    }
}

/// Compute the desired full file content given the current content + args.
///
/// Returns the new content as a String. Comparing to current tells us
/// whether anything will change.
fn compute_new_content(
    current: &str,
    state: &str,
    line: Option<&str>,
    regexp: Option<&str>,
    insertbefore: Option<&str>,
    insertafter: Option<&str>,
) -> Result<String> {
    // Split into lines, preserving trailing newline behavior.
    let had_trailing_newline = current.ends_with('\n');
    let lines: Vec<String> = if current.is_empty() {
        Vec::new()
    } else {
        current
            .split('\n')
            // If the string ended in '\n' we get a trailing empty element
            // that we want to drop and re-add only at the end.
            .map(String::from)
            .collect::<Vec<_>>()
            .into_iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let last = i + 1 == current.split('\n').count();
                if last && had_trailing_newline && s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            })
            .collect()
    };

    let re = regexp.map(simple_regex_match);

    let mut new_lines: Vec<String> = Vec::with_capacity(lines.len() + 1);

    match state {
        "present" => {
            let line = line.unwrap_or("");
            let mut replaced = false;
            if let Some(matcher) = re.as_ref() {
                for l in &lines {
                    if !replaced && matcher(l) {
                        new_lines.push(line.to_string());
                        replaced = true;
                    } else if matcher(l) {
                        // duplicate match: drop it (we already replaced once)
                        continue;
                    } else {
                        new_lines.push(l.clone());
                    }
                }
                if !replaced {
                    insert_at(&mut new_lines, line, insertbefore, insertafter);
                }
            } else {
                // No regex: append if not already present (exact match).
                let already_present = lines.iter().any(|l| l == line);
                new_lines.extend(lines.iter().cloned());
                if !already_present {
                    insert_at(&mut new_lines, line, insertbefore, insertafter);
                }
            }
        }
        "absent" => {
            let line_arg = line.unwrap_or("");
            for l in &lines {
                let drop = if let Some(matcher) = re.as_ref() {
                    matcher(l)
                } else {
                    l == line_arg
                };
                if !drop {
                    new_lines.push(l.clone());
                }
            }
        }
        _ => unreachable!("validated by caller"),
    }

    let mut out = new_lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    } else if had_trailing_newline {
        out.push('\n');
    }
    Ok(out)
}

fn insert_at(
    lines: &mut Vec<String>,
    line: &str,
    insertbefore: Option<&str>,
    insertafter: Option<&str>,
) {
    if let Some(pat) = insertbefore {
        if pat == "BOF" {
            lines.insert(0, line.to_string());
            return;
        }
        let m = simple_regex_match(pat);
        for i in 0..lines.len() {
            if m(&lines[i]) {
                lines.insert(i, line.to_string());
                return;
            }
        }
    }
    if let Some(pat) = insertafter {
        if pat == "EOF" {
            lines.push(line.to_string());
            return;
        }
        let m = simple_regex_match(pat);
        for i in (0..lines.len()).rev() {
            if m(&lines[i]) {
                lines.insert(i + 1, line.to_string());
                return;
            }
        }
    }
    // Default: append.
    lines.push(line.to_string());
}

/// A tiny regex-ish matcher. We don't pull `regex` into this crate for M1;
/// supports anchors `^`, `$`, and literal substring matching for the common
/// lineinfile cases. TODO_M2: swap for `regex` crate.
fn simple_regex_match(pattern: &str) -> impl Fn(&str) -> bool {
    let pat = pattern.to_string();
    move |s: &str| {
        let p = pat.as_str();
        if let Some(rest) = p.strip_prefix('^') {
            if let Some(rest2) = rest.strip_suffix('$') {
                return s == rest2;
            }
            return s.starts_with(rest);
        }
        if let Some(rest) = p.strip_suffix('$') {
            return s.ends_with(rest);
        }
        s.contains(p)
    }
}
