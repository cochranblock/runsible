//! runsible — ad-hoc command-line tool.
//!
//! Usage:
//!   runsible <pattern> -m <module> [-a <args>] -i <inventory> [-f <forks>] [-v]

use clap::Parser;
use runsible::{build_synthetic_playbook, exit_code, parse_args, run};

#[derive(Parser, Debug)]
#[command(
    name = "runsible",
    about = "Run a single module ad-hoc against an inventory pattern",
    version
)]
struct Cli {
    /// Inventory pattern (e.g. `all`, `webservers`, `localhost`)
    pattern: String,

    /// Fully-qualified module name (default: runsible_builtin.command)
    #[arg(short = 'm', long = "module-name", default_value = "runsible_builtin.command")]
    module_name: String,

    /// Module arguments as `key=val key2=val2` pairs or a JSON object string
    #[arg(short = 'a', long = "args", default_value = "")]
    args: String,

    /// Inventory file path or inline host list (`host1,host2,`)
    #[arg(short = 'i', long = "inventory")]
    inventory: String,

    /// Number of parallel forks (accepted, ignored at M0)
    #[arg(short = 'f', long = "forks", default_value_t = 5)]
    forks: usize,

    /// Verbosity (accepted, ignored at M0)
    #[arg(short = 'v', action = clap::ArgAction::Count)]
    verbose: u8,
}

fn main() {
    let cli = Cli::parse();

    // Parse -a args.
    let args_value = match parse_args(&cli.args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("runsible: error parsing --args: {e}");
            std::process::exit(1);
        }
    };

    // Build synthetic playbook.
    let playbook_src = match build_synthetic_playbook(&cli.pattern, &cli.module_name, &args_value) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("runsible: error building playbook: {e}");
            std::process::exit(1);
        }
    };

    // Execute.
    match run(&playbook_src, &cli.inventory, "ad-hoc") {
        Ok(result) => {
            std::process::exit(exit_code(&result));
        }
        Err(e) => {
            eprintln!("runsible: {e}");
            std::process::exit(1);
        }
    }
}
