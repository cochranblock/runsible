//! Just-in-time SSH user certificate minting via `ssh-keygen -s`.
//!
//! Pattern: a CA private key signs a short-lived user certificate for an
//! existing public key. OpenSSH automatically presents the certificate
//! alongside the key when both `<key>` and `<key>-cert.pub` exist on disk.
//!
//! The classic deployment problem this solves: long-lived SSH key pairs
//! distributed widely. JIT certs let an operator hold only a CA key and
//! mint a short-lived (e.g. 60-second) user cert per task — the user
//! pubkey never leaves the controller, the certificate is bound to a
//! single principal name, and expiry is enforced by the SSH daemon.
//!
//! `mint_jit_cert` shells out to `ssh-keygen` because it produces the exact
//! OpenSSH-format cert that sshd checks. Implementing the certificate
//! signing logic in Rust would mean hand-rolling Ed25519/RSA cert
//! serialization that sshd already understands; not worth the maintenance
//! burden for M1.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::errors::ConnectionError;

/// Configuration for minting a JIT user certificate.
#[derive(Debug, Clone)]
pub struct CaConfig {
    /// Path to the CA private key. Used by `ssh-keygen -s`.
    pub ca_key_path: PathBuf,
    /// The principal (remote login name) embedded in the cert.
    pub principal: String,
    /// Validity window in seconds (e.g. 60).
    pub validity_seconds: u64,
    /// `key_id` field — visible in sshd auth logs. Useful for audit.
    pub key_id: String,
    /// Optional `force-command` extension — locks the cert to a single command.
    pub force_command: Option<String>,
    /// Optional `source-address` restriction (CIDR, comma-separated).
    pub source_address: Option<String>,
}

impl CaConfig {
    pub fn new(ca_key_path: impl Into<PathBuf>, principal: impl Into<String>) -> Self {
        Self {
            ca_key_path: ca_key_path.into(),
            principal: principal.into(),
            validity_seconds: 60,
            key_id: format!("runsible-jit-{}", std::process::id()),
            force_command: None,
            source_address: None,
        }
    }

    pub fn with_validity_seconds(mut self, secs: u64) -> Self {
        self.validity_seconds = secs;
        self
    }

    pub fn with_key_id(mut self, id: impl Into<String>) -> Self {
        self.key_id = id.into();
        self
    }
}

/// Mint a short-lived user cert from the given user public key, signing it
/// with the CA private key in `ca`. Returns the path to the resulting
/// `<user_pubkey>-cert.pub` file (which OpenSSH auto-presents alongside
/// `<user_pubkey>`'s matching private key).
///
/// Uses the system `ssh-keygen` binary. The `validity_seconds` window
/// starts at "now"; sshd will reject the cert after expiry.
pub fn mint_jit_cert(user_pubkey: &Path, ca: &CaConfig) -> Result<PathBuf, ConnectionError> {
    if !ca.ca_key_path.exists() {
        return Err(ConnectionError::Ssh(format!(
            "ssh ca: ca_key_path does not exist: {}",
            ca.ca_key_path.display()
        )));
    }
    if !user_pubkey.exists() {
        return Err(ConnectionError::Ssh(format!(
            "ssh ca: user_pubkey does not exist: {}",
            user_pubkey.display()
        )));
    }

    let mut cmd = Command::new("ssh-keygen");
    cmd.arg("-q") // quiet
        .arg("-s")
        .arg(&ca.ca_key_path)
        .arg("-I")
        .arg(&ca.key_id)
        .arg("-n")
        .arg(&ca.principal)
        .arg("-V")
        .arg(format!("+{}s", ca.validity_seconds.max(1)));

    if let Some(fc) = &ca.force_command {
        cmd.arg("-O").arg(format!("force-command={fc}"));
    }
    if let Some(addr) = &ca.source_address {
        cmd.arg("-O").arg(format!("source-address={addr}"));
    }

    cmd.arg(user_pubkey);

    let output = cmd
        .output()
        .map_err(|e| ConnectionError::Ssh(format!("ssh-keygen spawn failed: {e}")))?;

    if !output.status.success() {
        return Err(ConnectionError::Ssh(format!(
            "ssh-keygen -s failed (rc={}): {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    // ssh-keygen writes <pubkey path with .pub stripped>-cert.pub.
    let cert_path = derive_cert_path(user_pubkey);
    if !cert_path.exists() {
        return Err(ConnectionError::Ssh(format!(
            "ssh-keygen produced no cert at expected path: {}",
            cert_path.display()
        )));
    }
    Ok(cert_path)
}

/// Given `/path/id_ed25519.pub`, return `/path/id_ed25519-cert.pub`.
/// Given `/path/id_ed25519`, return `/path/id_ed25519-cert.pub`.
pub fn derive_cert_path(user_pubkey: &Path) -> PathBuf {
    let s = user_pubkey.to_string_lossy();
    let stem = s.strip_suffix(".pub").unwrap_or(&s);
    PathBuf::from(format!("{stem}-cert.pub"))
}

/// Probe whether `ssh-keygen` is available on the controller.
pub fn ssh_keygen_available() -> bool {
    Command::new("ssh-keygen")
        .arg("-V")
        .output()
        .map(|o| o.status.success() || o.status.code().is_some())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_cert_path_strips_pub_suffix() {
        let p = derive_cert_path(Path::new("/keys/id_ed25519.pub"));
        assert_eq!(p, PathBuf::from("/keys/id_ed25519-cert.pub"));
    }

    #[test]
    fn derive_cert_path_handles_no_pub_suffix() {
        let p = derive_cert_path(Path::new("/keys/id_ed25519"));
        assert_eq!(p, PathBuf::from("/keys/id_ed25519-cert.pub"));
    }

    #[test]
    fn ca_config_defaults_to_60s_validity() {
        let cfg = CaConfig::new("/etc/runsible/ca", "deploy");
        assert_eq!(cfg.validity_seconds, 60);
        assert_eq!(cfg.principal, "deploy");
        assert!(cfg.key_id.starts_with("runsible-jit-"));
    }

    #[test]
    fn ca_config_builder_sets_validity_and_key_id() {
        let cfg = CaConfig::new("/etc/runsible/ca", "deploy")
            .with_validity_seconds(300)
            .with_key_id("smoke-test");
        assert_eq!(cfg.validity_seconds, 300);
        assert_eq!(cfg.key_id, "smoke-test");
    }

    #[test]
    fn mint_fails_when_ca_key_missing() {
        let user_pub = std::env::temp_dir().join(format!("rsl-cert-up-{}.pub", std::process::id()));
        std::fs::write(&user_pub, "ssh-ed25519 AAAA fake\n").unwrap();
        let cfg = CaConfig::new("/nonexistent/path/ca-key", "deploy");
        let r = mint_jit_cert(&user_pub, &cfg);
        let _ = std::fs::remove_file(&user_pub);
        assert!(r.is_err(), "must error when ca_key_path missing");
        assert!(matches!(r.unwrap_err(), ConnectionError::Ssh(_)));
    }

    #[test]
    fn mint_fails_when_user_pubkey_missing() {
        let ca = std::env::temp_dir().join(format!("rsl-cert-ca-{}.tmp", std::process::id()));
        std::fs::write(&ca, b"fake-ca-bytes").unwrap();
        let cfg = CaConfig::new(&ca, "deploy");
        let r = mint_jit_cert(Path::new("/nonexistent/path/user.pub"), &cfg);
        let _ = std::fs::remove_file(&ca);
        assert!(r.is_err());
    }

    /// End-to-end: generate a real CA key + user key via ssh-keygen, sign a
    /// JIT cert, parse the cert via `ssh-keygen -L -f` and verify the principal
    /// and key_id we asked for.
    #[test]
    fn mint_real_cert_end_to_end() {
        if !ssh_keygen_available() {
            eprintln!("skip: ssh-keygen unavailable");
            return;
        }

        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("rsl-jit-{pid}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();

        let ca_key = dir.join("ca");
        let user_key = dir.join("user_ed25519");

        // Mint CA + user keypairs (no passphrase).
        let ca_gen = Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-C", "rsl-test-ca", "-f"])
            .arg(&ca_key)
            .output()
            .expect("ssh-keygen ca");
        assert!(ca_gen.status.success(), "ca keygen: {}", String::from_utf8_lossy(&ca_gen.stderr));

        let user_gen = Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-C", "rsl-test-user", "-f"])
            .arg(&user_key)
            .output()
            .expect("ssh-keygen user");
        assert!(user_gen.status.success(), "user keygen: {}", String::from_utf8_lossy(&user_gen.stderr));

        let user_pub = PathBuf::from(format!("{}.pub", user_key.display()));
        let cfg = CaConfig::new(&ca_key, "deploy")
            .with_validity_seconds(120)
            .with_key_id("rsl-test-cert");

        let cert_path = mint_jit_cert(&user_pub, &cfg).expect("mint should succeed");
        assert!(cert_path.exists(), "cert file should exist");
        assert_eq!(
            cert_path.file_name().unwrap().to_string_lossy(),
            "user_ed25519-cert.pub"
        );

        // Parse the cert with `ssh-keygen -L -f` and verify principals + key_id.
        let parsed = Command::new("ssh-keygen")
            .args(["-L", "-f"])
            .arg(&cert_path)
            .output()
            .expect("ssh-keygen -L");
        assert!(parsed.status.success(), "parse: {}", String::from_utf8_lossy(&parsed.stderr));
        let s = String::from_utf8_lossy(&parsed.stdout);
        assert!(s.contains("Principals:"), "missing principals section: {s}");
        assert!(s.contains("deploy"), "principal 'deploy' not in cert: {s}");
        assert!(s.contains("rsl-test-cert"), "key_id not in cert: {s}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
