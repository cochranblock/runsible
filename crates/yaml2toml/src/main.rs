// SPDX-License-Identifier: Unlicense
// Contributors: Cochran Block

//! yaml2toml CLI — converts Ansible YAML files to runsible TOML.

use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process;

use anyhow::Context;
use clap::Parser;

use yaml2toml::{convert, Profile};

// ─── CLI definition ───────────────────────────────────────────────────────────

#[derive(Debug, clap::ValueEnum, Clone, Copy)]
enum ProfileArg {
    Playbook,
    Inventory,
    Vars,
    Auto,
}

impl From<ProfileArg> for Profile {
    fn from(p: ProfileArg) -> Self {
        match p {
            ProfileArg::Playbook => Profile::Playbook,
            ProfileArg::Inventory => Profile::Inventory,
            ProfileArg::Vars => Profile::Vars,
            ProfileArg::Auto => Profile::Auto,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "yaml2toml",
    about = "Convert Ansible YAML to runsible TOML (best-effort lossless)",
    version
)]
struct Cli {
    /// Conversion profile (default: auto-detect)
    #[arg(long, value_enum, default_value = "auto")]
    profile: ProfileArg,

    /// Output file path (default: stdout)
    #[arg(long, short)]
    output: Option<PathBuf>,

    /// Input file path; use `-` or omit for stdin
    input: Option<String>,
}

// ─── main ─────────────────────────────────────────────────────────────────────

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {:#}", e);
        process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Read input
    let yaml = match cli.input.as_deref() {
        None | Some("-") => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .context("reading stdin")?;
            buf
        }
        Some(path) => fs::read_to_string(path)
            .with_context(|| format!("reading input file '{}'", path))?,
    };

    let profile = Profile::from(cli.profile);
    let result = convert(&yaml, profile).map_err(|e| anyhow::anyhow!("{}", e))?;

    // Emit warnings to stderr
    for w in &result.warnings {
        eprintln!("# warn: {}", w);
    }

    // Write output
    match &cli.output {
        None => {
            io::stdout()
                .write_all(result.toml.as_bytes())
                .context("writing to stdout")?;
        }
        Some(path) => {
            fs::write(path, &result.toml)
                .with_context(|| format!("writing output file '{}'", path.display()))?;
        }
    }

    Ok(())
}
