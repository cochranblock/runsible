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
            ConnectionKind::SshSystem => Box::new(SshSystemConnection {
                host: self.host.clone().unwrap_or_else(|| "localhost".into()),
                user: self.user.clone(),
                port: self.port,
                identity_file: self.identity_file.clone(),
                control_path: self.control_path.clone(),
                connect_timeout: Duration::from_secs(
                    self.connect_timeout_seconds.unwrap_or(10),
                ),
                extra_args: vec![],
            }),
        }
    }
}
