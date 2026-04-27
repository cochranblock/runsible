//! `runsible_builtin.find` — list paths matching criteria.
//!
//! Args:
//!   paths     = "/dir" or ["/d1", "/d2"]   (required)
//!   patterns  = "*.log" or ["*.log", "*.txt"]   (default "*")
//!   recurse   = true | false                (default false)
//!   file_type = "file" | "directory" | "link" | "any"   (default "file")
//!   age       = "7d" / "1h" / "30m"   (optional)
//!   size      = "10k" / "1M"          (optional)
//!
//! Read-only — `will_change = false`.

use runsible_core::traits::{Cmd, ExecutionContext};
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};

pub struct FindModule;

impl DynModule for FindModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.find"
    }

    fn check_mode_safe(&self) -> bool {
        true
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let paths = extract_string_list(args.get("paths"))
            .ok_or_else(|| PlaybookError::TypeCheck("find: missing required arg `paths`".into()))?;
        if paths.is_empty() {
            return Err(PlaybookError::TypeCheck(
                "find: `paths` must contain at least one entry".into(),
            ));
        }
        let patterns = extract_string_list(args.get("patterns")).unwrap_or_else(|| vec!["*".into()]);
        let recurse = args.get("recurse").and_then(|v| v.as_bool()).unwrap_or(false);
        let file_type = args
            .get("file_type")
            .and_then(|v| v.as_str())
            .unwrap_or("file")
            .to_string();
        let age = args.get("age").and_then(|v| v.as_str()).map(String::from);
        let size = args.get("size").and_then(|v| v.as_str()).map(String::from);

        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({
                "paths": paths,
                "patterns": patterns,
                "recurse": recurse,
                "file_type": file_type,
                "age": age,
                "size": size,
            }),
            will_change: false,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        let started = std::time::Instant::now();
        let paths: Vec<String> = plan
            .diff
            .get("paths")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let patterns: Vec<String> = plan
            .diff
            .get("patterns")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let recurse = plan.diff.get("recurse").and_then(|v| v.as_bool()).unwrap_or(false);
        let file_type = plan
            .diff
            .get("file_type")
            .and_then(|v| v.as_str())
            .unwrap_or("file");
        let age = plan.diff.get("age").and_then(|v| v.as_str()).map(String::from);
        let size = plan.diff.get("size").and_then(|v| v.as_str()).map(String::from);

        // Build: find <paths> [-maxdepth 1] [-type t] \( -name p1 -o -name p2 \) [-mtime/size]
        let mut argv: Vec<String> = vec!["find".into()];
        argv.extend(paths.iter().cloned());
        if !recurse {
            argv.push("-maxdepth".into());
            argv.push("1".into());
        }
        match file_type {
            "file" => {
                argv.push("-type".into());
                argv.push("f".into());
            }
            "directory" => {
                argv.push("-type".into());
                argv.push("d".into());
            }
            "link" => {
                argv.push("-type".into());
                argv.push("l".into());
            }
            "any" => {}
            _ => {}
        }
        if !patterns.is_empty() {
            argv.push("(".into());
            for (i, p) in patterns.iter().enumerate() {
                if i > 0 {
                    argv.push("-o".into());
                }
                argv.push("-name".into());
                argv.push(p.clone());
            }
            argv.push(")".into());
        }
        if let Some(a) = age.as_deref() {
            if let Some(days) = age_to_days(a) {
                argv.push("-mtime".into());
                argv.push(format!("+{days}"));
            }
        }
        if let Some(s) = size.as_deref() {
            if let Some(arg) = size_to_find(s) {
                argv.push("-size".into());
                argv.push(arg);
            }
        }
        argv.push("-print".into());

        let cmd = Cmd {
            argv: argv.clone(),
            stdin: None,
            env: vec![],
            cwd: None,
            become_: None,
            timeout: None,
            tty: false,
        };
        let out = ctx.connection.exec(&cmd).map_err(|e| PlaybookError::ExecFailed {
            host: ctx.host.name.clone(),
            message: e.to_string(),
        })?;
        if out.rc != 0 && out.rc != 1 {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Failed,
                elapsed_ms: started.elapsed().as_millis() as u64,
                returns: serde_json::json!({
                    "stage": "find",
                    "rc": out.rc,
                    "stderr": String::from_utf8_lossy(&out.stderr).into_owned(),
                    "cmd": argv,
                }),
            });
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        let files: Vec<serde_json::Value> = stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(|p| serde_json::json!({"path": p}))
            .collect();
        let count = files.len();

        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Ok,
            elapsed_ms: started.elapsed().as_millis() as u64,
            returns: serde_json::json!({
                "files": files,
                "matched": count,
                "examined": count,
            }),
        })
    }
}

fn extract_string_list(v: Option<&toml::Value>) -> Option<Vec<String>> {
    let v = v?;
    if let Some(s) = v.as_str() {
        return Some(vec![s.to_string()]);
    }
    if let Some(arr) = v.as_array() {
        return Some(arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
    }
    None
}

fn age_to_days(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num_part, unit) = s.split_at(s.len() - 1);
    let n: i64 = num_part.parse().ok()?;
    match unit {
        "d" | "D" => Some(n),
        "w" | "W" => Some(n * 7),
        "h" | "H" => Some((n / 24).max(0)),
        "m" | "M" => Some((n / (24 * 60)).max(0)),
        _ => s.parse().ok(),
    }
}

fn size_to_find(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num, unit) = s.split_at(s.len() - 1);
    let n: i64 = num.parse().ok()?;
    let suffix = match unit {
        "k" | "K" => "k",
        "M" => "M",
        "G" => "G",
        "b" | "B" | "" => "c",
        _ => return Some(format!("+{n}c")),
    };
    Some(format!("+{n}{suffix}"))
}
