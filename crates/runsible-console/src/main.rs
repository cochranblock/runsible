//! runsible-console binary entry point.
//!
//! Thin clap wrapper around `runsible_console::run_repl`. M0 accepts
//! `--target`, `--connection`, and `--user`; only `--target` is meaningfully
//! used (against the built-in `debug`/`ping` modules running locally).

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "runsible-console",
    about = "Interactive REPL for the runsible engine",
    version
)]
struct Cli {
    /// Target host pattern (single host at M0).
    #[arg(long = "target", default_value = "localhost")]
    target: String,

    /// Connection plugin (only `local` works at M0).
    #[arg(long = "connection", default_value = "local")]
    connection: String,

    /// Remote user override (accepted, unused at M0).
    #[arg(long = "user")]
    user: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    if let Err(e) = runsible_console::run_repl(&cli.target, &cli.connection, cli.user.as_deref()) {
        eprintln!("runsible-console: {e}");
        std::process::exit(1);
    }
}
