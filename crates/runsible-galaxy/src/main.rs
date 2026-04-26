use std::fs;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use runsible_galaxy::errors::GalaxyError;
use runsible_galaxy::init::init_package;
use runsible_galaxy::lockfile::{LockedPackage, Lockfile};
use runsible_galaxy::manifest::{parse_manifest_file, write_manifest_file};
use runsible_galaxy::registry::RegistryIndex;
use runsible_galaxy::resolver::resolve_deps;
use runsible_galaxy::tarball::{build_package, extract_package};

#[derive(Parser)]
#[command(name = "runsible-galaxy")]
#[command(about = "runsible package manager (M0 — file:// registry only)")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a new package directory.
    Init {
        name: String,
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Build a .runsible-pkg tarball from the current directory.
    Build {
        #[arg(long)]
        out_dir: Option<PathBuf>,
    },
    /// Resolve and install dependencies from a file:// registry.
    Install {
        #[arg(long)]
        registry: Option<String>,
        #[arg(long)]
        frozen: bool,
    },
    /// List packages.
    List {
        #[arg(long)]
        installed: bool,
    },
    /// Show manifest info for a package.
    Info {
        pkg: String,
    },
    /// Add a dependency to runsible.toml and re-resolve.
    Add {
        pkg: String,
        #[arg(long)]
        registry: Option<String>,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), GalaxyError> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { name, path } => cmd_init(&name, path.as_deref()),
        Commands::Build { out_dir } => cmd_build(out_dir.as_deref()),
        Commands::Install { registry, frozen } => cmd_install(registry.as_deref(), frozen),
        Commands::List { installed } => cmd_list(installed),
        Commands::Info { pkg } => cmd_info(&pkg),
        Commands::Add { pkg, registry } => cmd_add(&pkg, registry.as_deref()),
    }
}

// ---------------------------------------------------------------------------
// init
// ---------------------------------------------------------------------------

fn cmd_init(name: &str, base: Option<&std::path::Path>) -> Result<(), GalaxyError> {
    init_package(name, base)?;
    let base = base.unwrap_or_else(|| std::path::Path::new("."));
    println!("Initialized package '{}' at {}", name, base.join(name).display());
    Ok(())
}

// ---------------------------------------------------------------------------
// build
// ---------------------------------------------------------------------------

fn cmd_build(out_dir: Option<&std::path::Path>) -> Result<(), GalaxyError> {
    let cwd = std::env::current_dir()?;
    let manifest = parse_manifest_file(&cwd.join("runsible.toml"))?;
    let name = &manifest.package.name;
    let version = &manifest.package.version;

    let default_out = cwd.join("target").join("runsible-pkg");
    let out = out_dir.unwrap_or(&default_out);

    let (path, checksum) = build_package(&cwd, name, version, out)?;
    println!("Built {} ({})", path.display(), checksum);
    Ok(())
}

// ---------------------------------------------------------------------------
// install
// ---------------------------------------------------------------------------

fn cmd_install(registry_url: Option<&str>, frozen: bool) -> Result<(), GalaxyError> {
    let cwd = std::env::current_dir()?;
    let manifest = parse_manifest_file(&cwd.join("runsible.toml"))?;
    let lock_path = cwd.join("runsible.lock");

    if frozen && !lock_path.exists() {
        return Err(GalaxyError::Lockfile(
            "--frozen specified but no runsible.lock exists".into(),
        ));
    }

    let registry_url = registry_url
        .map(|s| s.to_string())
        .unwrap_or_else(|| "file:///tmp/runsible-registry".to_string());

    // Load registry index.
    let registry_dir = registry_url
        .strip_prefix("file://")
        .map(std::path::Path::new)
        .ok_or_else(|| GalaxyError::Registry("only file:// registries supported at M0".into()))?;

    let index = RegistryIndex::load_from_dir(registry_dir)?;

    // Resolve.
    let resolved = resolve_deps(&manifest.dependencies, &index, &registry_url)?;

    // Build lockfile.
    let mut lockfile = Lockfile::new();
    for dep in &resolved {
        lockfile.packages.push(LockedPackage {
            name: dep.name.clone(),
            version: dep.version.to_string(),
            registry: dep.registry_url.clone(),
            checksum: dep.checksum.clone(),
        });
    }
    lockfile.write_to_file(&lock_path)?;

    // Install packages.
    let packages_dir = cwd.join("packages");
    for dep in &resolved {
        let tarball = RegistryIndex::tarball_path(registry_dir, &dep.name, &dep.version.to_string());
        let dest = packages_dir.join(&dep.name);
        fs::create_dir_all(&dest)?;
        extract_package(&tarball, &dest)?;
        println!("Installed {} {}", dep.name, dep.version);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

fn cmd_list(installed: bool) -> Result<(), GalaxyError> {
    if installed {
        let cwd = std::env::current_dir()?;
        let packages_dir = cwd.join("packages");
        if !packages_dir.exists() {
            println!("No packages installed.");
            return Ok(());
        }
        for entry in fs::read_dir(&packages_dir)? {
            let entry = entry?;
            if entry.path().is_dir() {
                // Try to read the manifest inside.
                let manifest_path = entry.path().join("runsible.toml");
                if let Ok(m) = parse_manifest_file(&manifest_path) {
                    println!("{} {}", m.package.name, m.package.version);
                } else {
                    println!("{}", entry.file_name().to_string_lossy());
                }
            }
        }
    } else {
        // List deps from runsible.toml.
        let cwd = std::env::current_dir()?;
        let manifest = parse_manifest_file(&cwd.join("runsible.toml"))?;
        if manifest.dependencies.is_empty() {
            println!("No dependencies.");
        }
        for (name, req) in &manifest.dependencies {
            println!("{} {}", name, req);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// info
// ---------------------------------------------------------------------------

fn cmd_info(pkg_spec: &str) -> Result<(), GalaxyError> {
    // Try local packages/ first.
    let cwd = std::env::current_dir()?;
    let (pkg_name, _version) = parse_pkg_spec(pkg_spec);
    let manifest_path = cwd.join("packages").join(&pkg_name).join("runsible.toml");
    if manifest_path.exists() {
        let m = parse_manifest_file(&manifest_path)?;
        println!("name:    {}", m.package.name);
        println!("version: {}", m.package.version);
        if let Some(d) = &m.package.description {
            println!("desc:    {}", d);
        }
        if let Some(l) = &m.package.license {
            println!("license: {}", l);
        }
    } else {
        eprintln!("Package '{}' not installed locally.", pkg_name);
    }
    Ok(())
}

fn parse_pkg_spec(spec: &str) -> (String, Option<String>) {
    if let Some((name, ver)) = spec.split_once('@') {
        (name.to_string(), Some(ver.to_string()))
    } else {
        (spec.to_string(), None)
    }
}

// ---------------------------------------------------------------------------
// add
// ---------------------------------------------------------------------------

fn cmd_add(pkg_spec: &str, _registry_url: Option<&str>) -> Result<(), GalaxyError> {
    let (pkg_name, version) = parse_pkg_spec(pkg_spec);
    let req = version.as_deref().unwrap_or("*");

    let cwd = std::env::current_dir()?;
    let manifest_path = cwd.join("runsible.toml");

    // Read + parse current manifest.
    let mut manifest = parse_manifest_file(&manifest_path)?;

    if manifest.dependencies.contains_key(&pkg_name) {
        eprintln!(
            "warning: '{}' already in dependencies; updating version requirement",
            pkg_name
        );
    }
    manifest.dependencies.insert(pkg_name.clone(), req.to_string());
    write_manifest_file(&manifest_path, &manifest)?;
    println!("Added {} = \"{}\" to [dependencies]", pkg_name, req);
    Ok(())
}
