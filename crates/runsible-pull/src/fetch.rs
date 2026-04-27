//! Fetch the bundle from a `[source]` config into `<state_dir>/bundle/`.
//!
//! M0 strategy: shell out to the system `git` binary. The plan calls out `gix`
//! as the longer-term direction, but for M0 the contract is "it works", and
//! git-the-binary is the simplest, most universally-available option.
//!
//! Behavior:
//!   * If `<state_dir>/bundle/.git` is missing, run `git clone --branch <branch>`.
//!   * Otherwise, `git fetch origin <branch>` then `git reset --hard origin/<branch>`.
//!   * After either path, `git rev-parse HEAD` to capture the resolved revision.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::PullConfig;
use crate::errors::{PullError, Result};

/// Outcome of a fetch.
#[derive(Debug, Clone)]
pub struct FetchResult {
    /// On-disk path to the bundle root (the working tree, not the `.git` dir).
    pub bundle_dir: PathBuf,
    /// Resolved HEAD revision (full SHA).
    pub source_rev: String,
}

/// Clone-or-fetch the bundle into `<state_dir>/bundle/`.
pub fn fetch_or_clone(cfg: &PullConfig) -> Result<FetchResult> {
    if cfg.source.kind != "git" {
        return Err(PullError::UnsupportedSourceKind(cfg.source.kind.clone()));
    }
    if cfg.source.ssh_key.is_some() && !cfg.source.url.starts_with("https://")
        && !cfg.source.url.starts_with("file://")
    {
        // Per the plan, SSH key auth is in scope but the M0 fallback is to
        // restrict to HTTPS / file:// and stub SSH.
        return Err(PullError::SshKeyNotImplemented);
    }

    std::fs::create_dir_all(&cfg.paths.state_dir)?;
    let bundle_dir = cfg.paths.state_dir.join("bundle");
    let git_dir = bundle_dir.join(".git");

    if !git_dir.exists() {
        git_clone(&cfg.source.url, &cfg.source.branch, &bundle_dir)?;
    } else {
        git_fetch_reset(&bundle_dir, &cfg.source.branch)?;
    }

    let source_rev = git_head_rev(&bundle_dir)?;

    Ok(FetchResult {
        bundle_dir,
        source_rev,
    })
}

fn git_clone(url: &str, branch: &str, dest: &Path) -> Result<()> {
    // Make sure dest's parent exists; git will create dest itself.
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Don't pre-create dest; `git clone` rejects a non-empty target.
    if dest.exists() {
        // If it exists but has no .git, scrub it (M0 tolerates this).
        std::fs::remove_dir_all(dest)?;
    }

    let out = Command::new("git")
        .arg("clone")
        .arg("--branch")
        .arg(branch)
        .arg("--single-branch")
        .arg("--")
        .arg(url)
        .arg(dest)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| PullError::Fetch(format!("spawning git: {e}")))?;

    if !out.status.success() {
        return Err(PullError::Fetch(format!(
            "git clone failed (exit {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(())
}

fn git_fetch_reset(dest: &Path, branch: &str) -> Result<()> {
    // git -C <dest> fetch origin <branch>
    let out = Command::new("git")
        .arg("-C")
        .arg(dest)
        .arg("fetch")
        .arg("origin")
        .arg(branch)
        .output()
        .map_err(|e| PullError::Fetch(format!("spawning git fetch: {e}")))?;
    if !out.status.success() {
        return Err(PullError::Fetch(format!(
            "git fetch failed (exit {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        )));
    }

    // git -C <dest> reset --hard origin/<branch>
    let out = Command::new("git")
        .arg("-C")
        .arg(dest)
        .arg("reset")
        .arg("--hard")
        .arg(format!("origin/{branch}"))
        .output()
        .map_err(|e| PullError::Fetch(format!("spawning git reset: {e}")))?;
    if !out.status.success() {
        return Err(PullError::Fetch(format!(
            "git reset --hard failed (exit {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        )));
    }

    Ok(())
}

fn git_head_rev(dir: &Path) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .map_err(|e| PullError::Fetch(format!("spawning git rev-parse: {e}")))?;
    if !out.status.success() {
        return Err(PullError::Fetch(format!(
            "git rev-parse HEAD failed (exit {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(sha)
}

/// Returns true if a `git` binary is available on `$PATH`. Tests use this to
/// skip themselves on minimal CI images.
pub fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ApplyConfig, PathsConfig, PullConfig, SourceConfig};

    fn init_local_repo(repo_root: &Path) -> Result<()> {
        // Initialize a tiny repo with one commit on branch `main`.
        let runs = [
            vec!["init", "-b", "main"],
            vec!["config", "user.email", "test@example.invalid"],
            vec!["config", "user.name", "test"],
            vec!["config", "commit.gpgsign", "false"],
        ];
        for argv in runs {
            let out = Command::new("git")
                .arg("-C")
                .arg(repo_root)
                .args(&argv)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {argv:?}: {}", String::from_utf8_lossy(&out.stderr));
        }

        std::fs::write(repo_root.join("hello.txt"), "hi\n").unwrap();

        let out = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(["add", "hello.txt"])
            .output()
            .unwrap();
        assert!(out.status.success(), "git add: {}", String::from_utf8_lossy(&out.stderr));

        let out = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(["commit", "-m", "initial"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git commit: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(())
    }

    #[test]
    fn git_fetch_local_repo() {
        if !git_available() {
            eprintln!("skipping git_fetch_local_repo: `git` binary not on PATH");
            return;
        }

        let upstream = tempfile::tempdir().unwrap();
        init_local_repo(upstream.path()).unwrap();

        let state = tempfile::tempdir().unwrap();
        let cfg = PullConfig {
            source: SourceConfig {
                kind: "git".into(),
                url: format!("file://{}", upstream.path().display()),
                branch: "main".into(),
                ssh_key: None,
            },
            apply: ApplyConfig {
                playbook: PathBuf::from("hello.txt"),
                extra_vars: vec![],
            },
            paths: PathsConfig {
                state_dir: state.path().to_path_buf(),
                heartbeat_path: state.path().join("heartbeat.json"),
            },
        };

        // First call: clone.
        let r1 = fetch_or_clone(&cfg).expect("first fetch_or_clone");
        assert!(r1.bundle_dir.join(".git").exists());
        assert!(r1.bundle_dir.join("hello.txt").exists());
        assert_eq!(r1.source_rev.len(), 40, "rev should be a full SHA");

        // Second call on the same state_dir: fetch + reset, no change in HEAD.
        let r2 = fetch_or_clone(&cfg).expect("second fetch_or_clone");
        assert_eq!(r1.source_rev, r2.source_rev);
    }
}
