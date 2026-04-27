//! `runsible_builtin.setup` — gather host facts.
//!
//! Mirrors Ansible's `setup` module. Probes the connected host (via
//! `ctx.connection.exec`) for well-known fact values and returns them under an
//! `ansible_facts` map. The engine merges those facts into the per-host vars
//! after apply (see engine.rs special-case alongside `set_fact`).
//!
//! Per `gather_subset`:
//!   - "min" (default): hostname/fqdn/user/python/date_time/pkg_mgr
//!   - "distribution": /etc/os-release fields + uname
//!   - "kernel": uname -r / uname -v
//!   - "network": default route + addresses + interfaces
//!   - "hardware": cpu count + memory totals
//!   - "all": all of the above
//!
//! Status is `Ok` (gathering facts is read-only). Any probe that fails is
//! silently dropped — the module never fails because some command is missing.

use runsible_core::traits::{Cmd, ExecutionContext};
use runsible_core::types::{Outcome, OutcomeStatus, Plan};

use crate::catalog::DynModule;
use crate::errors::Result;

pub struct SetupModule;

impl DynModule for SetupModule {
    fn module_name(&self) -> &str {
        "runsible_builtin.setup"
    }

    fn check_mode_safe(&self) -> bool {
        true
    }

    fn plan(&self, args: &toml::Value, ctx: &ExecutionContext) -> Result<Plan> {
        let subset = extract_subset(args);
        let timeout = args
            .get("gather_timeout")
            .and_then(|v| v.as_integer())
            .map(|i| i.max(1) as u64)
            .unwrap_or(30);

        Ok(Plan {
            module: self.module_name().into(),
            host: ctx.host.name.clone(),
            diff: serde_json::json!({
                "gather_subset": subset,
                "gather_timeout": timeout,
            }),
            will_change: true,
        })
    }

    fn apply(&self, plan: &Plan, ctx: &ExecutionContext) -> Result<Outcome> {
        let subset: Vec<String> = plan
            .diff
            .get("gather_subset")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| vec!["min".to_string()]);

        let want_all = subset.iter().any(|s| s == "all");
        let want_min = want_all || subset.iter().any(|s| s == "min");
        let want_dist = want_all || subset.iter().any(|s| s == "distribution");
        let want_kernel = want_all || subset.iter().any(|s| s == "kernel");
        let want_net = want_all || subset.iter().any(|s| s == "network");
        let want_hw = want_all || subset.iter().any(|s| s == "hardware");

        let mut facts = serde_json::Map::new();

        let started = std::time::Instant::now();

        if want_min {
            gather_min(ctx, &mut facts);
        }
        if want_dist {
            gather_distribution(ctx, &mut facts);
        }
        if want_kernel {
            gather_kernel(ctx, &mut facts);
        }
        if want_net {
            gather_network(ctx, &mut facts);
        }
        if want_hw {
            gather_hardware(ctx, &mut facts);
        }

        let elapsed_ms = started.elapsed().as_millis() as u64;

        Ok(Outcome {
            module: plan.module.clone(),
            host: ctx.host.name.clone(),
            status: OutcomeStatus::Ok,
            elapsed_ms,
            returns: serde_json::json!({
                "ansible_facts": serde_json::Value::Object(facts),
                "changed": false,
            }),
        })
    }
}

fn extract_subset(args: &toml::Value) -> Vec<String> {
    if let Some(arr) = args.get("gather_subset").and_then(|v| v.as_array()) {
        let v: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        if !v.is_empty() {
            return v;
        }
    }
    if let Some(s) = args.get("gather_subset").and_then(|v| v.as_str()) {
        return vec![s.to_string()];
    }
    vec!["min".to_string()]
}

/// Run an argv via the connection; return Some(stdout) on rc=0, else None.
fn run_argv(ctx: &ExecutionContext, argv: &[&str]) -> Option<String> {
    let cmd = Cmd {
        argv: argv.iter().map(|s| s.to_string()).collect(),
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
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Run a `sh -c` script; return Some(stdout) on rc=0, else None.
fn run_sh(ctx: &ExecutionContext, script: &str) -> Option<String> {
    let cmd = Cmd {
        argv: vec!["/bin/sh".into(), "-c".into(), script.to_string()],
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
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn gather_min(ctx: &ExecutionContext, f: &mut serde_json::Map<String, serde_json::Value>) {
    if let Some(s) = run_argv(ctx, &["hostname"]) {
        f.insert("ansible_hostname".into(), serde_json::Value::String(s));
    }
    if let Some(s) = run_argv(ctx, &["hostname", "-f"]) {
        f.insert("ansible_fqdn".into(), serde_json::Value::String(s));
    } else if let Some(s) = run_argv(ctx, &["hostname"]) {
        f.insert("ansible_fqdn".into(), serde_json::Value::String(s));
    }
    if let Some(s) = run_argv(ctx, &["id", "-un"]) {
        f.insert("ansible_user_id".into(), serde_json::Value::String(s));
    }
    if let Some(s) = run_argv(ctx, &["id", "-u"]) {
        if let Ok(n) = s.parse::<i64>() {
            f.insert("ansible_user_uid".into(), serde_json::Value::Number(n.into()));
        } else {
            f.insert("ansible_user_uid".into(), serde_json::Value::String(s));
        }
    }
    if let Some(s) = run_argv(ctx, &["id", "-g"]) {
        if let Ok(n) = s.parse::<i64>() {
            f.insert("ansible_user_gid".into(), serde_json::Value::Number(n.into()));
        } else {
            f.insert("ansible_user_gid".into(), serde_json::Value::String(s));
        }
    }
    if let Some(s) = run_sh(ctx, "echo \"$SHELL\"") {
        if !s.is_empty() {
            f.insert("ansible_user_shell".into(), serde_json::Value::String(s));
        }
    }
    if let Some(raw) = run_sh(ctx, "python3 --version 2>&1 || python --version 2>&1") {
        // Output is "Python X.Y.Z"
        let trimmed = raw.trim();
        if let Some(rest) = trimmed.strip_prefix("Python ") {
            f.insert(
                "ansible_python_version".into(),
                serde_json::Value::String(rest.trim().to_string()),
            );
        } else if !trimmed.is_empty() {
            f.insert(
                "ansible_python_version".into(),
                serde_json::Value::String(trimmed.to_string()),
            );
        }
    }

    // Date/time via chrono — controller-local clock. For remote SSH this
    // would need to be probed via `date` on the target; we leave that for M2.
    {
        use chrono::{Datelike, Local, Timelike};
        let now = Local::now();
        let weekday = now.format("%A").to_string();
        let iso = now.to_rfc3339();
        let dt = serde_json::json!({
            "date": now.format("%Y-%m-%d").to_string(),
            "time": now.format("%H:%M:%S").to_string(),
            "year": format!("{:04}", now.year()),
            "month": format!("{:02}", now.month()),
            "day": format!("{:02}", now.day()),
            "hour": format!("{:02}", now.hour()),
            "minute": format!("{:02}", now.minute()),
            "second": format!("{:02}", now.second()),
            "epoch": now.timestamp().to_string(),
            "weekday": weekday,
            "iso8601": iso,
        });
        f.insert("ansible_date_time".into(), dt);
    }

    // Package manager detection.
    let pkg_mgr = if run_sh(ctx, "command -v apt-get >/dev/null 2>&1").is_some() {
        "apt"
    } else if run_sh(ctx, "command -v dnf >/dev/null 2>&1").is_some() {
        "dnf"
    } else if run_sh(ctx, "command -v yum >/dev/null 2>&1").is_some() {
        "yum"
    } else if run_sh(ctx, "command -v zypper >/dev/null 2>&1").is_some() {
        "zypper"
    } else if run_sh(ctx, "command -v pacman >/dev/null 2>&1").is_some() {
        "pacman"
    } else if run_sh(ctx, "command -v apk >/dev/null 2>&1").is_some() {
        "apk"
    } else if run_sh(ctx, "command -v brew >/dev/null 2>&1").is_some() {
        "homebrew"
    } else {
        "unknown"
    };
    f.insert(
        "ansible_pkg_mgr".into(),
        serde_json::Value::String(pkg_mgr.to_string()),
    );
}

fn gather_distribution(ctx: &ExecutionContext, f: &mut serde_json::Map<String, serde_json::Value>) {
    let mut id: Option<String> = None;
    if let Some(body) = run_sh(ctx, "cat /etc/os-release 2>/dev/null") {
        for line in body.lines() {
            if let Some((k, v)) = line.split_once('=') {
                let val = v.trim().trim_matches('"').to_string();
                match k.trim() {
                    "ID" => {
                        id = Some(val.clone());
                        f.insert(
                            "ansible_distribution".into(),
                            serde_json::Value::String(val),
                        );
                    }
                    "VERSION_ID" => {
                        f.insert(
                            "ansible_distribution_version".into(),
                            serde_json::Value::String(val),
                        );
                    }
                    "VERSION_CODENAME" => {
                        f.insert(
                            "ansible_distribution_release".into(),
                            serde_json::Value::String(val),
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    let family = match id.as_deref() {
        Some("ubuntu") | Some("debian") | Some("kali") | Some("raspbian") | Some("linuxmint") => "Debian",
        Some("rhel") | Some("centos") | Some("fedora") | Some("rocky") | Some("almalinux") | Some("ol") => "RedHat",
        Some("suse") | Some("opensuse") | Some("opensuse-leap") | Some("opensuse-tumbleweed") | Some("sles") => "Suse",
        Some("arch") | Some("manjaro") | Some("endeavouros") => "Archlinux",
        Some("alpine") => "Alpine",
        Some("gentoo") => "Gentoo",
        _ => "unknown",
    };
    f.insert(
        "ansible_os_family".into(),
        serde_json::Value::String(family.to_string()),
    );

    if let Some(s) = run_argv(ctx, &["uname", "-r"]) {
        f.insert("ansible_kernel".into(), serde_json::Value::String(s));
    }
    if let Some(s) = run_argv(ctx, &["uname", "-m"]) {
        f.insert("ansible_architecture".into(), serde_json::Value::String(s));
    }
    if let Some(s) = run_argv(ctx, &["uname", "-s"]) {
        f.insert("ansible_system".into(), serde_json::Value::String(s));
    }
}

fn gather_kernel(ctx: &ExecutionContext, f: &mut serde_json::Map<String, serde_json::Value>) {
    if let Some(s) = run_argv(ctx, &["uname", "-r"]) {
        f.insert("ansible_kernel".into(), serde_json::Value::String(s));
    }
    if let Some(s) = run_argv(ctx, &["uname", "-v"]) {
        f.insert("ansible_kernel_version".into(), serde_json::Value::String(s));
    }
}

fn gather_network(ctx: &ExecutionContext, f: &mut serde_json::Map<String, serde_json::Value>) {
    // Default route lookup. `ip route get 8.8.8.8` outputs:
    //   8.8.8.8 via 10.0.0.1 dev eth0 src 10.0.0.42 uid 1000 ...
    if let Some(line) = run_sh(ctx, "ip route get 8.8.8.8 2>/dev/null | head -n 1") {
        let mut address: Option<String> = None;
        let mut interface: Option<String> = None;
        let mut gateway: Option<String> = None;
        let toks: Vec<&str> = line.split_whitespace().collect();
        let mut i = 0;
        while i < toks.len() {
            match toks[i] {
                "src" if i + 1 < toks.len() => {
                    address = Some(toks[i + 1].to_string());
                    i += 2;
                    continue;
                }
                "dev" if i + 1 < toks.len() => {
                    interface = Some(toks[i + 1].to_string());
                    i += 2;
                    continue;
                }
                "via" if i + 1 < toks.len() => {
                    gateway = Some(toks[i + 1].to_string());
                    i += 2;
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
        if address.is_some() || interface.is_some() {
            let mut obj = serde_json::Map::new();
            if let Some(a) = address {
                obj.insert("address".into(), serde_json::Value::String(a));
            }
            if let Some(iface) = interface {
                obj.insert("interface".into(), serde_json::Value::String(iface));
            }
            if let Some(g) = gateway {
                obj.insert("gateway".into(), serde_json::Value::String(g));
            }
            f.insert(
                "ansible_default_ipv4".into(),
                serde_json::Value::Object(obj),
            );
        }
    }

    // All IPv4 addresses.
    if let Some(out) = run_sh(
        ctx,
        "ip -4 -o addr show 2>/dev/null | awk '{print $4}' | cut -d/ -f1",
    ) {
        let addrs: Vec<serde_json::Value> = out
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::Value::String(l.trim().to_string()))
            .collect();
        if !addrs.is_empty() {
            f.insert(
                "ansible_all_ipv4_addresses".into(),
                serde_json::Value::Array(addrs),
            );
        }
    }

    // Interface names.
    if let Some(out) = run_sh(ctx, "ls /sys/class/net 2>/dev/null") {
        let ifaces: Vec<serde_json::Value> = out
            .split_whitespace()
            .map(|s| serde_json::Value::String(s.to_string()))
            .collect();
        if !ifaces.is_empty() {
            f.insert("ansible_interfaces".into(), serde_json::Value::Array(ifaces));
        }
    }
}

fn gather_hardware(ctx: &ExecutionContext, f: &mut serde_json::Map<String, serde_json::Value>) {
    if let Some(s) = run_argv(ctx, &["nproc"]) {
        if let Ok(n) = s.trim().parse::<i64>() {
            f.insert(
                "ansible_processor_count".into(),
                serde_json::Value::Number(n.into()),
            );
        }
    } else if let Some(s) = run_argv(ctx, &["getconf", "_NPROCESSORS_ONLN"]) {
        if let Ok(n) = s.trim().parse::<i64>() {
            f.insert(
                "ansible_processor_count".into(),
                serde_json::Value::Number(n.into()),
            );
        }
    }

    if let Some(body) = run_sh(ctx, "cat /proc/meminfo 2>/dev/null") {
        for line in body.lines() {
            // Format: "MemTotal:        16384000 kB"
            if let Some((k, rest)) = line.split_once(':') {
                let k = k.trim();
                let rest = rest.trim();
                let kb_str = rest.split_whitespace().next().unwrap_or("");
                if let Ok(kb) = kb_str.parse::<i64>() {
                    let mb = kb / 1024;
                    match k {
                        "MemTotal" => {
                            f.insert(
                                "ansible_memtotal_mb".into(),
                                serde_json::Value::Number(mb.into()),
                            );
                        }
                        "MemFree" => {
                            f.insert(
                                "ansible_memfree_mb".into(),
                                serde_json::Value::Number(mb.into()),
                            );
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}
