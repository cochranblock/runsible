//! Discover and report the test environment (M0 scope).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::config;

/// Snapshot of what the runner found on the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvReport {
    pub runsible_test_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rust_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cargo_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playbook_bin: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lint_bin: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_bin: Option<PathBuf>,
    pub cwd: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_runsible_toml: Option<PathBuf>,
}

/// Build an `EnvReport` from the live environment.
pub fn discover_env() -> EnvReport {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_runsible_toml = config::discover_project_runsible_toml(&cwd);

    EnvReport {
        runsible_test_version: env!("CARGO_PKG_VERSION").to_string(),
        rust_version: rustc_version(),
        cargo_path: find_cargo(),
        playbook_bin: find_runsible_bin("runsible-playbook"),
        lint_bin: find_runsible_bin("runsible-lint"),
        doc_bin: find_runsible_bin("runsible-doc"),
        cwd,
        project_runsible_toml,
    }
}

/// Pretty, human-readable rendering for `env --show`.
pub fn render_text(report: &EnvReport) -> String {
    let mut out = String::new();
    out.push_str("runsible-test environment\n");
    out.push_str("─────────────────────────\n");
    out.push_str(&format!("  runsible-test version : {}\n", report.runsible_test_version));
    out.push_str(&format!(
        "  rust version          : {}\n",
        report.rust_version.as_deref().unwrap_or("(not found)")
    ));
    out.push_str(&format!(
        "  cargo                 : {}\n",
        report
            .cargo_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(not found)".into())
    ));
    out.push_str(&format!(
        "  runsible-playbook     : {}\n",
        report
            .playbook_bin
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(not found)".into())
    ));
    out.push_str(&format!(
        "  runsible-lint         : {}\n",
        report
            .lint_bin
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(not found)".into())
    ));
    out.push_str(&format!(
        "  runsible-doc          : {}\n",
        report
            .doc_bin
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(not found)".into())
    ));
    out.push_str(&format!("  cwd                   : {}\n", report.cwd.display()));
    out.push_str(&format!(
        "  project runsible.toml : {}\n",
        report
            .project_runsible_toml
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(not found)".into())
    ));
    out
}

// ---------------------------------------------------------------------------
// Discovery helpers (also used by `units`)
// ---------------------------------------------------------------------------

/// Try `~/.cargo/bin/cargo` first (the workspace convention on this box),
/// then fall back to PATH lookup.
pub fn find_cargo() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("HOME") {
        let p = Path::new(&home).join(".cargo/bin/cargo");
        if p.exists() {
            return Some(p);
        }
    }
    which_in_path("cargo")
}

/// Locate a runsible binary by name. Checks PATH first, then
/// `target/debug/<name>` relative to cwd, then `target/release/<name>`.
fn find_runsible_bin(name: &str) -> Option<PathBuf> {
    if let Some(p) = which_in_path(name) {
        return Some(p);
    }
    let cwd = std::env::current_dir().ok()?;
    for sub in ["target/debug", "target/release"] {
        let p = cwd.join(sub).join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Hand-rolled `which`: walk $PATH and return the first match.
fn which_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Spawn `rustc --version` and capture stdout, trimmed. Tries `rustc` on PATH
/// first, then falls back to `~/.cargo/bin/rustc` (rustup's default install).
fn rustc_version() -> Option<String> {
    let candidates: Vec<PathBuf> = {
        let mut v: Vec<PathBuf> = Vec::new();
        if let Some(p) = which_in_path("rustc") {
            v.push(p);
        }
        if let Some(home) = std::env::var_os("HOME") {
            let p = Path::new(&home).join(".cargo/bin/rustc");
            if p.exists() {
                v.push(p);
            }
        }
        v
    };

    for rustc in candidates {
        if let Ok(out) = Command::new(&rustc).arg("--version").output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_discovery_works() {
        let report = discover_env();
        // At least one of cargo or rustc should be discoverable on any sane
        // dev box; on this one both are.
        assert!(
            report.cargo_path.is_some() || report.rust_version.is_some(),
            "expected to discover cargo or rustc; got: {:#?}",
            report
        );
        assert!(!report.runsible_test_version.is_empty());
    }

    #[test]
    fn find_cargo_returns_dot_cargo_bin_when_present() {
        // If `~/.cargo/bin/cargo` exists on this box (it does on the test
        // node), `find_cargo()` MUST prefer it. Otherwise the function may
        // fall back to PATH lookup, in which case the assertion is relaxed
        // to "some cargo was found OR none was found" — never a panic.
        let home = std::env::var_os("HOME");
        let preferred = home
            .as_ref()
            .map(|h| Path::new(h).join(".cargo/bin/cargo"));
        let found = find_cargo();

        match (preferred, found) {
            (Some(pref), Some(found)) if pref.exists() => {
                assert_eq!(
                    found, pref,
                    "find_cargo must prefer ~/.cargo/bin/cargo when it exists"
                );
            }
            (Some(pref), None) if pref.exists() => {
                panic!("~/.cargo/bin/cargo exists but find_cargo returned None");
            }
            _ => {
                // Either no HOME or no ~/.cargo/bin/cargo. Anything is fine
                // here — we already exercise the negative path in the
                // env_discovery_works test.
            }
        }
    }

    #[test]
    fn project_runsible_toml_finds_workspace_root_from_subdir() {
        // Create a fake project with a runsible.toml at the root and a
        // nested subdirectory; discovery starting from the subdir must walk
        // upward and find the root manifest.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("runsible.toml"),
            "[package]\nname = \"x\"\nversion = \"0.0.1\"\n",
        )
        .unwrap();
        let sub = root.join("subpkg/inner");
        std::fs::create_dir_all(&sub).unwrap();

        let found = crate::config::discover_project_runsible_toml(&sub);
        assert_eq!(
            found,
            Some(root.join("runsible.toml")),
            "discovery must walk up from subdir to the workspace root manifest"
        );

        // From a directory tree that has NO manifest anywhere upward, the
        // discovery must stop without panicking. We can't guarantee that no
        // ancestor has a runsible.toml on this box, so this branch only
        // checks the function returns Option<PathBuf> cleanly.
        let lonely = tempfile::tempdir().unwrap();
        let _ = crate::config::discover_project_runsible_toml(lonely.path());
    }
}
