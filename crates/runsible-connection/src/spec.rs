use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use runsible_core::traits::Connection;

use crate::local::LocalConnection;
use crate::ssh_system::SshSystemConnection;

/// Serializable descriptor for a connection. Pass this in your TOML inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionSpec {
    pub kind: ConnectionKind,
    pub host: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<PathBuf>,
    pub control_path: Option<String>,
    pub connect_timeout_seconds: Option<u64>,

    /// Path to a CA private key. When set, every SSH operation mints a
    /// fresh JIT user certificate via `ssh-keygen -s` before connecting.
    /// Requires `identity_file` (the user keypair whose `.pub` gets signed).
    #[serde(default)]
    pub ca_key_path: Option<PathBuf>,

    /// Principal (remote login name) embedded in the JIT cert. Required
    /// when `ca_key_path` is set.
    #[serde(default)]
    pub ca_principal: Option<String>,

    /// Validity window for minted JIT certs, in seconds. Default 60.
    #[serde(default)]
    pub ca_validity_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionKind {
    Local,
    SshSystem,
}

impl ConnectionSpec {
    /// Construct a boxed [`Connection`] from this spec.
    pub fn build(&self) -> Box<dyn Connection> {
        match self.kind {
            ConnectionKind::Local => Box::new(LocalConnection),
            ConnectionKind::SshSystem => {
                let ca_config = match (&self.ca_key_path, &self.ca_principal) {
                    (Some(ca), Some(principal)) => Some(
                        crate::ssh_cert::CaConfig::new(ca.clone(), principal.clone())
                            .with_validity_seconds(self.ca_validity_seconds.unwrap_or(60)),
                    ),
                    _ => None,
                };
                Box::new(SshSystemConnection {
                    host: self.host.clone().unwrap_or_else(|| "localhost".into()),
                    user: self.user.clone(),
                    port: self.port,
                    identity_file: self.identity_file.clone(),
                    control_path: self.control_path.clone(),
                    connect_timeout: Duration::from_secs(
                        self.connect_timeout_seconds.unwrap_or(10),
                    ),
                    extra_args: vec![],
                    ca_config,
                })
            }
        }
    }
}
