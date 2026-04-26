use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use runsible_doc::{DocRegistry, render_markdown, render_snippet, render_text};

#[derive(Debug, Clone, ValueEnum)]
enum ListFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, ValueEnum)]
enum ShowFormat {
    Text,
    Json,
    Markdown,
}

#[derive(Debug, Parser)]
#[command(
    name = "runsible-doc",
    about = "Browse documentation for runsible modules",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// List all available modules
    List {
        /// Output format
        #[arg(long, value_enum, default_value = "text")]
        format: ListFormat,
    },
    /// Show full documentation for a module
    Show {
        /// Fully-qualified module name (e.g. runsible_builtin.debug)
        module: String,
        /// Output format
        #[arg(long, value_enum, default_value = "text")]
        format: ShowFormat,
    },
    /// Print a minimal TOML task snippet ready to paste
    Snippet {
        /// Fully-qualified module name (e.g. runsible_builtin.debug)
        module: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let registry = DocRegistry::builtins();

    match cli.command {
        Cmd::List { format } => {
            let docs = registry.list();
            match format {
                ListFormat::Text => {
                    println!("{:<40}  {}", "MODULE", "DESCRIPTION");
                    println!("{}", "-".repeat(80));
                    for doc in docs {
                        println!("{:<40}  {}", doc.name, doc.short_description);
                    }
                }
                ListFormat::Json => {
                    let list: Vec<_> = docs
                        .iter()
                        .map(|d| {
                            serde_json::json!({
                                "name": d.name,
                                "short_description": d.short_description,
                            })
                        })
                        .collect();
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&list)
                            .context("failed to serialize list to JSON")?
                    );
                }
            }
        }
        Cmd::Show { module, format } => {
            let doc = registry
                .get(&module)
                .ok_or_else(|| anyhow::anyhow!("module not found: '{}'", module))?;
            match format {
                ShowFormat::Text => print!("{}", render_text(doc)),
                ShowFormat::Markdown => print!("{}", render_markdown(doc)),
                ShowFormat::Json => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(doc)
                            .context("failed to serialize module doc to JSON")?
                    );
                }
            }
        }
        Cmd::Snippet { module } => {
            let doc = registry
                .get(&module)
                .ok_or_else(|| anyhow::anyhow!("module not found: '{}'", module))?;
            print!("{}", render_snippet(doc));
        }
    }

    Ok(())
}
