use std::process;

use clap::Parser;

use runsible_playbook::run;

#[derive(Parser)]
#[command(
    name = "runsible-playbook",
    about = "Run a TOML playbook against an inventory",
    version
)]
struct Cli {
    /// Playbook file to run.
    playbook: String,

    /// Inventory: a file path, an inline host list (`host1,host2,`), or a
    /// single hostname. Repeatable; multiple inventories are merged.
    #[arg(short = 'i', long = "inventory", required = true)]
    inventory: Vec<String>,

    /// Only run plays and tasks tagged with these tags. (M1)
    #[arg(long)]
    tags: Vec<String>,

    /// Skip plays and tasks tagged with these tags. (M1)
    #[arg(long)]
    skip_tags: Vec<String>,

    /// Dry-run: plan only, do not apply. (M1)
    #[arg(long = "check")]
    check_mode: bool,
}

fn main() {
    let cli = Cli::parse();

    if cli.check_mode {
        eprintln!("note: --check mode is not yet implemented (M1)");
        process::exit(2);
    }

    let src = match std::fs::read_to_string(&cli.playbook) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading {:?}: {e}", cli.playbook);
            process::exit(1);
        }
    };

    // Merge all -i specs into a comma-joined inline list for now.
    let inv_spec = cli.inventory.join(",") + ",";

    match run(&src, &inv_spec, &cli.playbook) {
        Ok(result) => process::exit(result.exit_code()),
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}
