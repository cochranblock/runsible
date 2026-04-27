//! Minimal package-path discovery for M0.
//!
//! M0 keeps configuration simple — the test runner takes a package directory
//! (default `.`) and walks upward only to discover the project-level
//! `runsible.toml` for the `env --show` report.

use std::path::{Path, PathBuf};

/// Resolve the package directory: if the caller passes a path, use it as-is;
/// otherwise default to the current working directory. Always canonicalize so
/// `.` resolves to a real directory name (improves report labels).
pub fn package_path<P: AsRef<Path>>(arg: Option<P>) -> std::io::Result<PathBuf> {
    let raw = match arg {
        Some(p) => p.as_ref().to_path_buf(),
        None => std::env::current_dir()?,
    };
    raw.canonicalize().or(Ok(raw))
}

/// Walk upward from `start` looking for a `runsible.toml`. Returns the path
/// to the manifest file if found, otherwise `None`. This is used by `env --show`
/// to report the project root, NOT by the sanity engine (which inspects an
/// explicit package directory).
pub fn discover_project_runsible_toml(start: &Path) -> Option<PathBuf> {
    let mut dir = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };

    loop {
        let candidate = dir.join("runsible.toml");
        if candidate.exists() {
            return Some(candidate);
        }
        match dir.parent() {
            Some(p) if p != dir => dir = p.to_path_buf(),
            _ => return None,
        }
    }
}
