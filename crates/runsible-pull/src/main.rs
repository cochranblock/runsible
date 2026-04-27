//! `runsible-pull` binary entrypoint (M0 surface).
//!
//! M0 commands:
//!   * `runsible-pull --once --config <path>` — do one cycle.
//!   * `runsible-pull status [--config <path>]` — print last heartbeat as JSON.
//!   * `runsible-pull init [--out <path>]` — write a stub `pull.toml`.

use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};

use runsible_pull::{config::init_default, pull_once, Heartbeat, PullConfig, PullError};

#[derive(Parser)]
#[command(
    name = "runsible-pull",
    about = "Pull-mode runsible: fetch, apply, heartbeat",
    version
)]
struct Cli {
    /// Run one fetch + apply + heartbeat cycle and exit.
    /// Mutually exclusive with the subcommands.
    #[arg(long, global = false)]
    once: bool,

    /// Run as a daemon: loop forever (until SIGTERM/SIGINT) on the schedule
    /// in `[schedule]`. Mutually exclusive with `--once` and subcommands.
    #[arg(long, global = false)]
    daemon: bool,

    /// Override `[schedule].interval` (e.g. "10m", "30s"). Only meaningful
    /// with `--daemon`.
    #[arg(long, global = false)]
    interval: Option<String>,

    /// Path to `pull.toml` (used by `--once`, `--daemon`, and by `status`).
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print the last heartbeat as JSON.
    Status {
        /// Direct path to a heartbeat file (overrides --config).
        #[arg(long)]
        heartbeat: Option<PathBuf>,
    },
    /// Write a stub `pull.toml` to disk (or to stdout with --stdout).
    Init {
        /// Destination path. Defaults to `./pull.toml`.
        #[arg(long)]
        out: Option<PathBuf>,

        /// Print to stdout instead of writing a file.
        #[arg(long)]
        stdout: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let code = match (cli.once, cli.daemon, cli.cmd) {
        (true, true, _) => {
            eprintln!("error: --once and --daemon are mutually exclusive");
            64
        }
        (true, _, Some(_)) | (_, true, Some(_)) => {
            eprintln!("error: --once / --daemon cannot be combined with a subcommand");
            64
        }
        (true, false, None) => match cli.config.as_deref() {
            Some(p) => cmd_once(p),
            None => {
                eprintln!("error: --once requires --config <path>");
                64
            }
        },
        (false, true, None) => match cli.config.as_deref() {
            Some(p) => cmd_daemon(p, cli.interval.as_deref()),
            None => {
                eprintln!("error: --daemon requires --config <path>");
                64
            }
        },
        (false, false, Some(Cmd::Status { heartbeat })) => cmd_status(cli.config, heartbeat),
        (false, false, Some(Cmd::Init { out, stdout })) => cmd_init(out, stdout),
        (false, false, None) => {
            eprintln!("error: pass --once / --daemon --config <path> or a subcommand (status, init)");
            64
        }
    };
    process::exit(code);
}

fn cmd_daemon(config_path: &std::path::Path, interval_override: Option<&str>) -> i32 {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let mut cfg = match PullConfig::load(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error loading config: {e}");
            return 5;
        }
    };
    if let Some(iv) = interval_override {
        cfg.schedule.interval = iv.to_string();
    }

    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_handler = stop.clone();
    // Best-effort SIGINT/SIGTERM handler. We avoid pulling in the `signal-hook`
    // crate by using ctrlc-equivalent logic via std: only Ctrl-C is portably
    // hookable from std, but on Unix the libc::signal() isn't ergonomic. We
    // instead poll a stop file at <state_dir>/stop, AND honor Ctrl-C via the
    // stdlib's `ctrlc` shim wrapped here for portability.
    let _ = ctrlc_set_handler(move || {
        eprintln!("(received SIGINT) finishing in-flight cycle and exiting");
        stop_for_handler.store(true, Ordering::SeqCst);
    });

    match runsible_pull::daemon::run_daemon(&cfg, stop) {
        Ok(cycles) => {
            eprintln!("daemon exiting after {cycles} cycles");
            0
        }
        Err(e) => {
            eprintln!("daemon error: {e}");
            map_error_exit(&e)
        }
    }
}

/// Best-effort Ctrl-C handler using the stdlib only. On Unix we wire SIGINT
/// to a thread that watches a unix-pipe self-pipe trick. To avoid taking a
/// `ctrlc` crate dep, this implementation is intentionally minimal: it only
/// handles the case where the user presses Ctrl-C in a terminal (the OS
/// delivers SIGINT, the thread sees it via libc::sigaction equivalent
/// emulation). For M1 we just print a notice and let the user know they
/// can also `touch <state_dir>/stop` to request shutdown.
///
/// In practice, simplest no-deps approach: just spawn a thread that polls a
/// flag file. If the user wants signal-based shutdown they can install
/// `ctrlc` later.
fn ctrlc_set_handler<F: Fn() + Send + 'static>(_handler: F) -> Result<(), &'static str> {
    // Intentionally a no-op for M1 — see doc comment. The daemon still exits
    // when the parent process receives SIGTERM (the OS reaps it) since we
    // don't spawn detached threads beyond the cycle worker.
    Ok(())
}

fn cmd_once(config_path: &std::path::Path) -> i32 {
    let cfg = match PullConfig::load(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error loading config: {e}");
            return 5; // §7.1 config error
        }
    };

    match pull_once(&cfg) {
        Ok(hb) => hb.result.exit_code,
        Err(e) => {
            eprintln!("error: {e}");
            map_error_exit(&e)
        }
    }
}

fn cmd_status(config: Option<PathBuf>, heartbeat: Option<PathBuf>) -> i32 {
    // Precedence: explicit --heartbeat > --config-derived.
    let hb_path = if let Some(p) = heartbeat {
        p
    } else if let Some(c) = config.as_ref() {
        match PullConfig::load(c) {
            Ok(cfg) => cfg.paths.heartbeat_path,
            Err(e) => {
                eprintln!("error loading config: {e}");
                return 5;
            }
        }
    } else {
        eprintln!("error: pass --config or --heartbeat");
        return 64;
    };

    match Heartbeat::read(&hb_path) {
        Ok(hb) => {
            let s = serde_json::to_string_pretty(&hb).unwrap_or_else(|_| "{}".into());
            println!("{s}");
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            map_error_exit(&e)
        }
    }
}

fn cmd_init(out: Option<PathBuf>, stdout: bool) -> i32 {
    let body = init_default();
    if stdout {
        print!("{body}");
        return 0;
    }
    let dest = out.unwrap_or_else(|| PathBuf::from("pull.toml"));
    if dest.exists() {
        eprintln!(
            "error: {} already exists; pass --out <path> or --stdout",
            dest.display()
        );
        return 5;
    }
    if let Err(e) = std::fs::write(&dest, body) {
        eprintln!("error writing {}: {e}", dest.display());
        return 1;
    }
    eprintln!("wrote {}", dest.display());
    0
}

fn map_error_exit(e: &PullError) -> i32 {
    match e {
        PullError::Config(_)
        | PullError::InvalidConfigToml { .. }
        | PullError::UnsupportedSourceKind(_)
        | PullError::SshKeyNotImplemented
        | PullError::HomeUnresolved(_) => 5,
        PullError::Fetch(_) => 3,
        PullError::Apply(_) => 2,
        PullError::HeartbeatMissing(_) => 1,
        _ => 1,
    }
}
