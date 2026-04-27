//! `runsible_builtin.uri` — talk HTTP via curl.
//!
//! Args:
//!   url           = "https://..."  (required)
//!   method        = "GET"          (default)
//!   body          = "string"       (optional)
//!   body_format   = "json" | "form" | "raw"   (default "raw")
//!   status_code   = [200]          (allowed list)
//!   headers       = { "X-Foo" = "bar" }
//!   return_content= false
//!   dest          = "/path"        (write response body here)
//!
//! M1: shells out to `curl`. If curl is missing, returns Failed with a
//! TODO_M2 message (we don't bring in reqwest for the engine).

use runsible_core::traits::{Cmd, ExecutionContext};
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::{PlaybookError, Result};

pub struct UriModule;

impl DynModule for UriModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.uri"
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PlaybookError::TypeCheck("uri: missing required arg `url`".into()))?
            .to_string();
        let method = args
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET")
            .to_string();
        let body = args.get("body").and_then(|v| v.as_str()).map(String::from);
        let body_format = args
            .get("body_format")
            .and_then(|v| v.as_str())
            .unwrap_or("raw")
            .to_string();
        let status_code: Vec<i64> = match args.get("status_code") {
            Some(v) => {
                if let Some(arr) = v.as_array() {
                    arr.iter().filter_map(|x| x.as_integer()).collect()
                } else if let Some(n) = v.as_integer() {
                    vec![n]
                } else {
                    vec![200]
                }
            }
            None => vec![200],
        };
        let headers: Vec<(String, String)> = match args.get("headers") {
            Some(toml::Value::Table(t)) => t
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect(),
            _ => Vec::new(),
        };
        let return_content = args
            .get("return_content")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let dest = args.get("dest").and_then(|v| v.as_str()).map(String::from);

        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({
                "url": url,
                "method": method,
                "body": body,
                "body_format": body_format,
                "status_code": status_code,
                "headers": headers,
                "return_content": return_content,
                "dest": dest,
            }),
            will_change: true,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        let started = std::time::Instant::now();

        if !which_ok("curl", ctx) {
            // TODO_M2: native HTTP client when curl is missing.
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Failed,
                elapsed_ms: started.elapsed().as_millis() as u64,
                returns: serde_json::json!({
                    "stage": "preflight",
                    "msg": "uri: curl not found on host (TODO_M2: native HTTP)",
                }),
            });
        }

        let url = plan.diff.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let method = plan.diff.get("method").and_then(|v| v.as_str()).unwrap_or("GET").to_string();
        let body = plan.diff.get("body").and_then(|v| v.as_str()).map(String::from);
        let body_format = plan
            .diff
            .get("body_format")
            .and_then(|v| v.as_str())
            .unwrap_or("raw")
            .to_string();
        let allowed: Vec<i64> = plan
            .diff
            .get("status_code")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_i64()).collect())
            .unwrap_or_else(|| vec![200]);
        let headers: Vec<(String, String)> = plan
            .diff
            .get("headers")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_array())
                    .filter_map(|p| {
                        let k = p.first()?.as_str()?.to_string();
                        let v = p.get(1)?.as_str()?.to_string();
                        Some((k, v))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let return_content = plan
            .diff
            .get("return_content")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let dest = plan.diff.get("dest").and_then(|v| v.as_str()).map(String::from);

        // Build curl invocation.
        let mut argv: Vec<String> = vec![
            "curl".into(),
            "-sS".into(),
            "-X".into(),
            method.clone(),
            "-o".into(),
            "-".into(),
            "-w".into(),
            "\n%{http_code}".into(),
        ];
        for (k, v) in &headers {
            argv.push("-H".into());
            argv.push(format!("{k}: {v}"));
        }
        if let Some(b) = &body {
            match body_format.as_str() {
                "json" => {
                    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
                        argv.push("-H".into());
                        argv.push("Content-Type: application/json".into());
                    }
                    argv.push("--data".into());
                    argv.push(b.clone());
                }
                "form" => {
                    argv.push("--data-urlencode".into());
                    argv.push(b.clone());
                }
                _ => {
                    argv.push("--data".into());
                    argv.push(b.clone());
                }
            }
        }
        argv.push(url.clone());

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

        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        // Last line is the http_code we requested via -w.
        let (body_str, status) = split_body_and_status(&stdout);

        if let Some(d) = &dest {
            if let Err(e) = std::fs::write(d, body_str.as_bytes()) {
                return Ok(Outcome {
                    module: plan.module.clone(),
                    host: ctx.host.name.clone(),
                    status: OutcomeStatus::Failed,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    returns: serde_json::json!({
                        "stage": "write_dest",
                        "msg": e.to_string(),
                    }),
                });
            }
        }

        let status_ok = allowed.iter().any(|s| *s as i64 == status);
        let mut returns = serde_json::json!({
            "status": status,
            "url": url,
        });
        if return_content {
            returns["content"] = serde_json::Value::String(body_str.clone());
            // Try to parse as JSON; if it parses, expose under "json".
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body_str) {
                returns["json"] = v;
            }
        }

        if !status_ok || out.rc != 0 {
            returns["stage"] = serde_json::json!("status_check");
            returns["allowed_status"] = serde_json::json!(allowed);
            returns["rc"] = serde_json::json!(out.rc);
            return Ok(Outcome {
                module: plan.module.clone(),
                host: ctx.host.name.clone(),
                status: OutcomeStatus::Failed,
                elapsed_ms: started.elapsed().as_millis() as u64,
                returns,
            });
        }

        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Ok,
            elapsed_ms: started.elapsed().as_millis() as u64,
            returns,
        })
    }
}

fn split_body_and_status(out: &str) -> (String, i64) {
    // We told curl to print "\n%{http_code}" at the very end. The body is
    // everything except the last line.
    if let Some(idx) = out.rfind('\n') {
        let last = out[idx + 1..].trim();
        if let Ok(code) = last.parse::<i64>() {
            return (out[..idx].to_string(), code);
        }
    }
    (out.to_string(), 0)
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
