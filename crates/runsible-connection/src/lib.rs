//! runsible-connection
//!
//! M0: LocalConnection + SshSystemConnection (system ssh/scp binary).
//! M1 will add a native russh-based implementation.

pub mod errors;
pub mod local;
pub mod spec;
pub mod ssh_system;

pub use errors::{ConnectionError, Result};
pub use local::LocalConnection;
pub use spec::{ConnectionKind, ConnectionSpec};
pub use ssh_system::SshSystemConnection;

#[cfg(test)]
mod tests {
    use super::*;
    use runsible_core::traits::Cmd;

    fn base_cmd(argv: Vec<&str>) -> Cmd {
        Cmd {
            argv: argv.into_iter().map(String::from).collect(),
            stdin: None,
            env: vec![],
            cwd: None,
            become_: None,
            timeout: None,
            tty: false,
        }
    }

    /// ConnectionSpec { kind: Local } builds a working LocalConnection.
    #[tokio::test]
    async fn ssh_system_spec_build() {
        let spec = ConnectionSpec {
            kind: ConnectionKind::Local,
            host: None,
            user: None,
            port: None,
            identity_file: None,
            control_path: None,
            connect_timeout_seconds: None,
        };

        let conn = spec.build();
        let cmd = base_cmd(vec!["echo", "spec_build_ok"]);
        let out = conn.exec(&cmd).await.expect("exec via spec.build()");
        assert_eq!(out.rc, 0);
        assert_eq!(out.stdout, b"spec_build_ok\n");
    }
}
