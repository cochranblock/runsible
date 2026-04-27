//! `runsible_builtin.unarchive` — extract a tar/zip archive on the host.
//!
//! Args:
//!   src        = "/path/to/archive"   (required)
//!   dest       = "/extract/dir"       (required)
//!   remote_src = true                 (M1: always treated as remote_src)
//!   creates    = "/marker"            (skip if this exists)

use std::path::Path;

use runsible_core::traits::{Cmd, ExecutionContext};
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};

pub struct UnarchiveModule;

impl DynModule for UnarchiveModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.unarchive"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let src = args
            .get("src")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PlaybookError::TypeCheck("unarchive: missing required arg `src`".into()))?
            .to_string();
        let dest = args
            .get("dest")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PlaybookError::TypeCheck("unarchive: missing required arg `dest`".into()))?
            .to_string();
        let _remote_src = args
            .get("remote_src")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let creates = args.get("creates").and_then(|v| v.as_str()).map(String::from);

        let mut will_change = true;
        if let Some(p) = &creates {
            if ctx.connection.file_exists(Path::new(p)).unwrap_or(false) {
                will_change = false;
            }
        }

        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({
                "src": src,
                "dest": dest,
                "creates": creates,
            }),
            will_change,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        if !plan.will_change {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Skipped,
                elapsed_ms: 0,
                returns: serde_json::json!({"skipped_reason": "creates guard satisfied"}),
            });
        }
        let started = std::time::Instant::now();
        let src = plan.diff.get("src").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let dest = plan.diff.get("dest").and_then(|v| v.as_str()).unwrap_or("").to_string();

        // Make sure dest exists.
        let mk = Cmd {
            argv: vec!["mkdir".into(), "-p".into(), dest.clone()],
            stdin: None,
            env: vec![],
            cwd: None,
            become_: None,
            timeout: None,
            tty: false,
        };
        let _ = ctx.connection.exec(&mk);

        let argv = pick_extractor(&src, &dest);
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
        if out.rc != 0 {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Failed,
                elapsed_ms: started.elapsed().as_millis() as u64,
                returns: serde_json::json!({
                    "stage": "extract",
                    "rc": out.rc,
                    "stderr": String::from_utf8_lossy(&out.stderr).into_owned(),
                    "cmd": argv,
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
                "src": src,
                "dest": dest,
            }),
        })
    }
}

fn pick_extractor(src: &str, dest: &str) -> Vec<String> {
    let lower = src.to_ascii_lowercase();
    if lower.ends_with(".zip") {
        return vec!["unzip".into(), "-o".into(), src.into(), "-d".into(), dest.into()];
    }
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        return vec!["tar".into(), "xzf".into(), src.into(), "-C".into(), dest.into()];
    }
    if lower.ends_with(".tar.bz2") || lower.ends_with(".tbz2") {
        return vec!["tar".into(), "xjf".into(), src.into(), "-C".into(), dest.into()];
    }
    if lower.ends_with(".tar.xz") || lower.ends_with(".txz") {
        return vec!["tar".into(), "xJf".into(), src.into(), "-C".into(), dest.into()];
    }
    if lower.ends_with(".tar") {
        return vec!["tar".into(), "xf".into(), src.into(), "-C".into(), dest.into()];
    }
    // Default: try tar with auto-detect (-a) if available.
    vec!["tar".into(), "xf".into(), src.into(), "-C".into(), dest.into()]
}
