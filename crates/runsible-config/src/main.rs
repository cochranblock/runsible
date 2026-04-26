use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};

use runsible_config::{dump_with_defaults, find_config_file, init_default, load, load_from_path, search_path, Source};

#[derive(Parser)]
#[command(name = "runsible-config", about = "Manage runsible configuration")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show the current effective config (merged, all defaults filled in).
    Show {
        /// Print a specific dotted key, e.g. `defaults.forks`.
        #[arg(long)]
        key: Option<String>,
    },
    /// List all config keys with their current values and sources.
    List,
    /// Dump the full config with all defaults as valid TOML.
    Dump,
    /// Write a commented default config to ./runsible.toml (or stdout with --stdout).
    Init {
        #[arg(long)]
        stdout: bool,
    },
    /// Validate config file. Exits 0 if valid, non-zero on error.
    Validate {
        /// Path to validate. Defaults to the normal search path.
        path: Option<PathBuf>,
    },
    /// Explain the source of a specific config key.
    Explain {
        /// Dotted key, e.g. `output.format`.
        key: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.cmd {
        Cmd::Show { key } => cmd_show(key),
        Cmd::List => cmd_list(),
        Cmd::Dump => cmd_dump(),
        Cmd::Init { stdout } => cmd_init(stdout),
        Cmd::Validate { path } => cmd_validate(path),
        Cmd::Explain { key } => cmd_explain(key),
    }
}

fn cmd_show(key: Option<String>) {
    let loaded = load().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });

    let toml_str = dump_with_defaults(&loaded.config).unwrap_or_else(|e| {
        eprintln!("error serializing config: {e}");
        process::exit(1);
    });

    if let Some(k) = key {
        let val: toml::Value = toml::from_str(&toml_str).unwrap();
        match dotted_get(&val, &k) {
            Some(v) => println!("{v}"),
            None => {
                eprintln!("error: key '{k}' not found");
                process::exit(1);
            }
        }
    } else {
        print!("{toml_str}");
    }
}

fn cmd_list() {
    let loaded = load().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });

    let source_label = match &loaded.source {
        Source::EnvVar(v) => format!("env:{v}"),
        Source::File(p) => format!("file:{}", p.display()),
        Source::Default => "default".to_string(),
    };

    let toml_str = dump_with_defaults(&loaded.config).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });

    let val: toml::Value = toml::from_str(&toml_str).unwrap();
    let mut pairs: Vec<(String, String)> = Vec::new();
    collect_pairs("", &val, &mut pairs);

    for (k, v) in &pairs {
        println!("{k} = {v}  [{source_label}]");
    }
}

fn cmd_dump() {
    let loaded = load().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });
    match dump_with_defaults(&loaded.config) {
        Ok(s) => print!("{s}"),
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

fn cmd_init(stdout: bool) {
    let content = init_default();
    if stdout {
        print!("{content}");
    } else {
        let dest = PathBuf::from("runsible.toml");
        if dest.exists() {
            eprintln!("error: runsible.toml already exists; use --stdout to print instead");
            process::exit(1);
        }
        std::fs::write(&dest, &content).unwrap_or_else(|e| {
            eprintln!("error writing runsible.toml: {e}");
            process::exit(1);
        });
        eprintln!("wrote runsible.toml");
    }
}

fn cmd_validate(path: Option<PathBuf>) {
    let p = match path {
        Some(p) => p,
        None => match find_config_file() {
            Some(p) => p,
            None => {
                eprintln!("no config file found in search path");
                process::exit(1);
            }
        },
    };
    match load_from_path(&p) {
        Ok(_) => eprintln!("ok: {}", p.display()),
        Err(e) => {
            eprintln!("invalid: {e}");
            process::exit(1);
        }
    }
}

fn cmd_explain(key: String) {
    let loaded = load().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });

    let source_label = match &loaded.source {
        Source::EnvVar(v) => format!("env var {v}"),
        Source::File(p) => format!("file {}", p.display()),
        Source::Default => "compiled-in default".to_string(),
    };

    let search: Vec<String> = search_path().iter().map(|p| p.display().to_string()).collect();

    let toml_str = dump_with_defaults(&loaded.config).unwrap();
    let val: toml::Value = toml::from_str(&toml_str).unwrap();

    match dotted_get(&val, &key) {
        Some(v) => {
            println!("{key} = {v}");
            println!("source: {source_label}");
            println!("search path: {}", search.join(" → "));
        }
        None => {
            eprintln!("error: key '{key}' not found");
            process::exit(1);
        }
    }
}

fn dotted_get<'a>(val: &'a toml::Value, key: &str) -> Option<String> {
    let mut cur = val;
    for part in key.split('.') {
        match cur {
            toml::Value::Table(t) => cur = t.get(part)?,
            _ => return None,
        }
    }
    Some(toml_value_display(cur))
}

fn toml_value_display(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => format!("\"{}\"", s.replace('"', "\\\"")),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Datetime(d) => d.to_string(),
        toml::Value::Array(a) => {
            let items: Vec<String> = a.iter().map(toml_value_display).collect();
            format!("[{}]", items.join(", "))
        }
        toml::Value::Table(t) => {
            let items: Vec<String> = t.iter().map(|(k, v)| format!("{k} = {}", toml_value_display(v))).collect();
            format!("{{ {} }}", items.join(", "))
        }
    }
}

fn collect_pairs(prefix: &str, val: &toml::Value, out: &mut Vec<(String, String)>) {
    match val {
        toml::Value::Table(t) => {
            for (k, v) in t {
                let full = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                collect_pairs(&full, v, out);
            }
        }
        _ => out.push((prefix.to_string(), toml_value_display(val))),
    }
}
