//! `runsible-test` CLI (M0).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

use runsible_test::env::{discover_env, render_text as render_env_text};
use runsible_test::sanity::{run_sanity, Severity};
use runsible_test::units::run_units;

#[derive(Debug, Parser)]
#[command(
    name = "runsible-test",
    version,
    about = "Developer-facing test runner for runsible packages",
    long_about = None,
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Run sanity tests over a package directory.
    Sanity {
        /// Output format.
        #[arg(long, default_value = "text")]
        format: Format,
        /// Package directory (default: current directory).
        package_dir: Option<PathBuf>,
    },
    /// Run unit tests for the package's `crates/<sub>/` modules.
    Units {
        /// Package directory (default: current directory).
        package_dir: Option<PathBuf>,
    },
    /// Show discovered environment.
    Env {
        /// Print the env report.
        #[arg(long)]
        show: bool,
        /// Output format.
        #[arg(long, default_value = "text")]
        format: Format,
    },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Sanity {
            format,
            package_dir,
        } => cmd_sanity(format, package_dir),
        Cmd::Units { package_dir } => cmd_units(package_dir),
        Cmd::Env { show, format } => cmd_env(show, format),
    }
}

fn cmd_sanity(format: Format, package_dir: Option<PathBuf>) -> ExitCode {
    let pkg = match runsible_test::config::package_path(package_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("runsible-test: could not resolve package directory: {e}");
            return ExitCode::from(2);
        }
    };

    let report = run_sanity(&pkg);

    match format {
        Format::Text => print_sanity_text(&report),
        Format::Json => match serde_json::to_string_pretty(&report) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("runsible-test: JSON serialization failed: {e}");
                return ExitCode::from(2);
            }
        },
    }

    if report.has_errors() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn print_sanity_text(report: &runsible_test::SanityReport) {
    println!("runsible-test sanity — package `{}`", report.package);
    println!("─────────────────────────────────");
    if report.findings.is_empty() {
        println!("  no findings — clean.");
        return;
    }
    for f in &report.findings {
        let file = f
            .file
            .as_ref()
            .map(|p| format!(" {}", p.display()))
            .unwrap_or_default();
        println!("  [{}] {}{}\n      {}", f.severity, f.id, file, f.message);
    }
    println!();
    println!(
        "  totals: {} error, {} warning, {} info",
        report.count_by(Severity::Error),
        report.count_by(Severity::Warning),
        report.count_by(Severity::Info),
    );
}

fn cmd_units(package_dir: Option<PathBuf>) -> ExitCode {
    let pkg = match runsible_test::config::package_path(package_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("runsible-test: could not resolve package directory: {e}");
            return ExitCode::from(2);
        }
    };

    let report = run_units(&pkg);

    println!("runsible-test units — package `{}`", report.package);
    println!("─────────────────────────────────");

    if let Some(reason) = &report.skipped_reason {
        println!("  skipped: {reason}");
        return ExitCode::SUCCESS;
    }

    if report.crates_tested.is_empty() {
        println!("  no Rust crates discovered under `crates/`.");
    } else {
        println!("  crates tested: {}", report.crates_tested.join(", "));
        println!("  passed: {}    failed: {}", report.passed, report.failed);
    }

    if report.failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn cmd_env(show: bool, format: Format) -> ExitCode {
    let report = discover_env();
    // `--show` and the default behavior both render; `--show` is kept for
    // muscle-memory parity with `ansible-test env --show`.
    let _ = show;

    match format {
        Format::Text => print!("{}", render_env_text(&report)),
        Format::Json => match serde_json::to_string_pretty(&report) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("runsible-test: JSON serialization failed: {e}");
                return ExitCode::from(2);
            }
        },
    }

    ExitCode::SUCCESS
}
