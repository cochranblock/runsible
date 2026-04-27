//! `runsible_builtin.get_url` — download a URL to a destination path.
//!
//! Args:
//!   url      = "https://example.com/file.tgz"   (required)
//!   dest     = "/path/on/host"                  (required)
//!   mode     = "0644"                           (optional octal string)
//!   checksum = "sha256:<hex>"                   (optional; verified after download)
//!
//! Implementation: shells out to `curl` (or `wget` fallback) via the
//! connection's exec, so we don't have to pull reqwest/hyper into the engine.
//!
//! Idempotence:
//!   dest exists + checksum matches  → will_change=false
//!   dest exists + no checksum given → will_change=false (presence is the gate)
//!   otherwise → will_change=true

use std::path::Path;

use runsible_core::traits::{Cmd, ExecutionContext};
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};

pub struct GetUrlModule;

impl DynModule for GetUrlModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.get_url"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PlaybookError::TypeCheck("get_url: missing required arg `url`".into()))?
            .to_string();
        let dest = args
            .get("dest")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                PlaybookError::TypeCheck("get_url: missing required arg `dest`".into())
            })?
            .to_string();
        let mode = args.get("mode").and_then(|v| v.as_str()).map(String::from);
        let checksum = args
            .get("checksum")
            .and_then(|v| v.as_str())
            .map(String::from);

        // Validate checksum format if present.
        if let Some(c) = &checksum {
            parse_checksum(c).map_err(PlaybookError::TypeCheck)?;
        }

        let exists = ctx.connection.file_exists(Path::new(&dest)).unwrap_or(false);

        let will_change = if !exists {
            true
        } else if let Some(expected_full) = &checksum {
            let (algo, expected_hex) = parse_checksum(expected_full).unwrap();
            match compute_checksum(&algo, &dest, ctx) {
                Some(actual) => !actual.eq_ignore_ascii_case(&expected_hex),
                None => true,
            }
        } else {
            // dest exists, no checksum to compare → leave it alone.
            false
        };

        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({
                "url": url,
                "dest": dest,
                "mode": mode,
                "checksum": checksum,
                "currently_exists": exists,
            }),
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

        let url = plan.diff.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let dest = plan.diff.get("dest").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let checksum = plan.diff.get("checksum").and_then(|v| v.as_str()).map(String::from);
        let mode = plan
            .diff
            .get("mode")
            .and_then(|v| v.as_str())
            .and_then(|s| u32::from_str_radix(s.trim_start_matches('0'), 8).ok());

        let started = std::time::Instant::now();

        // Pick curl or wget.
        let downloader = pick_downloader(ctx).ok_or_else(|| PlaybookError::ExecFailed {
            host: ctx.host.name.clone(),
            message: "get_url: neither curl nor wget is available on the host".into(),
        })?;

        // mkdir -p the parent dir.
        if let Some(parent) = Path::new(&dest).parent() {
            if !parent.as_os_str().is_empty() {
                let mk = Cmd {
                    argv: vec![
                        "mkdir".into(),
                        "-p".into(),
                        parent.to_string_lossy().into_owned(),
                    ],
                    stdin: None,
                    env: vec![],
                    cwd: None,
                    become_: None,
                    timeout: None,
                    tty: false,
                };
                let mk_out = ctx.connection.exec(&mk).map_err(|e| PlaybookError::ExecFailed {
                    host: ctx.host.name.clone(),
                    message: e.to_string(),
                })?;
                if mk_out.rc != 0 {
                    return Ok(Outcome {
                        module: plan.module.clone(),
                        host: ctx.host.name.clone(),
                        status: OutcomeStatus::Failed,
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        returns: serde_json::json!({
                            "stage": "mkdir",
                            "rc": mk_out.rc,
                            "stderr": String::from_utf8_lossy(&mk_out.stderr).into_owned(),
                        }),
                    });
                }
            }
        }

        // Run downloader.
        let dl_argv = match downloader {
            Downloader::Curl => vec![
                "curl".into(),
                "-fsSL".into(),
                "-o".into(),
                dest.clone(),
                url.clone(),
            ],
            Downloader::Wget => vec![
                "wget".into(),
                "-q".into(),
                "-O".into(),
                dest.clone(),
                url.clone(),
            ],
        };
        let dl_cmd = Cmd {
            argv: dl_argv.clone(),
            stdin: None,
            env: vec![],
            cwd: None,
            become_: None,
            timeout: None,
            tty: false,
        };
        let dl_out = ctx.connection.exec(&dl_cmd).map_err(|e| PlaybookError::ExecFailed {
            host: ctx.host.name.clone(),
            message: e.to_string(),
        })?;
        if dl_out.rc != 0 {
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Failed,
                elapsed_ms: started.elapsed().as_millis() as u64,
                returns: serde_json::json!({
                    "stage": "download",
                    "rc": dl_out.rc,
                    "cmd": dl_argv,
                    "stderr": String::from_utf8_lossy(&dl_out.stderr).into_owned(),
                }),
            });
        }

        // Verify checksum if supplied.
        if let Some(expected_full) = &checksum {
            let (algo, expected_hex) = parse_checksum(expected_full).unwrap();
            match compute_checksum(&algo, &dest, ctx) {
                Some(actual) if actual.eq_ignore_ascii_case(&expected_hex) => {}
                Some(actual) => {
                    return Ok(Outcome {
                        module: plan.module.clone(),
                        host: ctx.host.name.clone(),
                        status: OutcomeStatus::Failed,
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        returns: serde_json::json!({
                            "stage": "checksum",
                            "expected": expected_hex,
                            "actual": actual,
                            "algo": algo,
                        }),
                    });
                }
                None => {
                    return Ok(Outcome {
                        module: plan.module.clone(),
                        host: ctx.host.name.clone(),
                        status: OutcomeStatus::Failed,
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        returns: serde_json::json!({
                            "stage": "checksum",
                            "error": "could not compute checksum after download",
                            "algo": algo,
                        }),
                    });
                }
            }
        }

        // chmod if requested.
        if let Some(m) = mode {
            let chmod = Cmd {
                argv: vec!["chmod".into(), format!("{:o}", m), dest.clone()],
                stdin: None,
                env: vec![],
                cwd: None,
                become_: None,
                timeout: None,
                tty: false,
            };
            let _ = ctx.connection.exec(&chmod);
        }

        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Changed,
            elapsed_ms: started.elapsed().as_millis() as u64,
            returns: serde_json::json!({
                "changed": true,
                "url": url,
                "dest": dest,
                "downloader": match downloader {
                    Downloader::Curl => "curl",
                    Downloader::Wget => "wget",
                },
            }),
        })
    }
}

#[derive(Clone, Copy)]
enum Downloader {
    Curl,
    Wget,
}

fn pick_downloader(ctx: &ExecutionContext) -> Option<Downloader> {
    if which_ok("curl", ctx) {
        Some(Downloader::Curl)
    } else if which_ok("wget", ctx) {
        Some(Downloader::Wget)
    } else {
        None
    }
}

fn which_ok(bin: &str, ctx: &ExecutionContext) -> bool {
    let cmd = Cmd {
        argv: vec!["which".into(), bin.into()],
        stdin: None,
        env: vec![],
        cwd: None,
        become_: None,
        timeout: None,
        tty: false,
    };
    ctx.connection.exec(&cmd).map(|o| o.rc == 0).unwrap_or(false)
}

/// Parse a checksum string like `"sha256:abc..."` into (algo, hex).
fn parse_checksum(s: &str) -> std::result::Result<(String, String), String> {
    let (algo, hex) = s
        .split_once(':')
        .ok_or_else(|| format!("invalid checksum format '{s}' (expected '<algo>:<hex>')"))?;
    if algo.is_empty() || hex.is_empty() {
        return Err(format!("invalid checksum format '{s}'"));
    }
    match algo {
        "sha256" | "sha1" | "md5" | "sha512" => Ok((algo.to_string(), hex.to_string())),
        other => Err(format!("unsupported checksum algorithm '{other}'")),
    }
}

fn compute_checksum(algo: &str, dest: &str, ctx: &ExecutionContext) -> Option<String> {
    let bin = match algo {
        "sha256" => "sha256sum",
        "sha1" => "sha1sum",
        "md5" => "md5sum",
        "sha512" => "sha512sum",
        _ => return None,
    };
    let cmd = Cmd {
        argv: vec![bin.into(), dest.into()],
        stdin: None,
        env: vec![],
        cwd: None,
        become_: None,
        timeout: None,
        tty: false,
    };
    let out = ctx.connection.exec(&cmd).ok()?;
    if out.rc != 0 {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Format: "<hex>  <path>"
    stdout.split_whitespace().next().map(|s| s.to_string())
}
