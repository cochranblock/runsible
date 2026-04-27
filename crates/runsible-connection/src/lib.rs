//! runsible-connection
//!
//! M0: LocalConnection + SshSystemConnection (system ssh/scp binary).
//! M1 will add a native russh-based implementation.

pub mod errors;
pub mod local;
pub mod local_sync;
pub mod spec;
pub mod ssh_cert;
pub mod ssh_system;

pub use errors::{ConnectionError, Result};
pub use local::LocalConnection;
pub use local_sync::LocalSync;
pub use spec::{ConnectionKind, ConnectionSpec};
pub use ssh_system::SshSystemConnection;

// ---------------------------------------------------------------------------
// f30 — TRIPLE SIMS smoke gate
// ---------------------------------------------------------------------------

/// Smoke gate: exercise the public LocalSync API end-to-end. Spawn `echo
/// f30`, verify rc/stdout, then exercise `file_exists` and `slurp` against
/// a real tempfile and a bogus path. Returns 0 on success or a non-zero
/// stage code on failure. Used by the runsible-connection-test binary's
/// TRIPLE SIMS gate. Skips with code 0 if `echo` is unavailable.
pub fn f30() -> i32 {
    use runsible_core::traits::{Cmd, SyncConnection};

    // If `echo` isn't on PATH, skip cleanly.
    if std::process::Command::new("echo")
        .arg("probe")
        .output()
        .is_err()
    {
        eprintln!("skip: echo unavailable");
        return 0;
    }

    let conn = LocalSync;

    // Stage 1: exec `echo f30` and verify rc=0, stdout="f30\n".
    let cmd = Cmd {
        argv: vec!["echo".into(), "f30".into()],
        stdin: None,
        env: vec![],
        cwd: None,
        become_: None,
        timeout: None,
        tty: false,
    };
    let outcome = match conn.exec(&cmd) {
        Ok(o) => o,
        Err(_) => return 1,
    };
    if outcome.rc != 0 {
        return 2;
    }
    if outcome.stdout.as_slice() != b"f30\n" {
        return 3;
    }

    // Stage 2: file_exists + slurp on a real tempfile.
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("rsl-conn-f30-{pid}-{nanos}.bin"));
    let payload: &[u8] = b"\x00\x01runsible-connection f30\xff";
    if std::fs::write(&path, payload).is_err() {
        return 4;
    }

    // Stage 3: file_exists must return true for the tempfile.
    match conn.file_exists(&path) {
        Ok(true) => {}
        Ok(false) => {
            let _ = std::fs::remove_file(&path);
            return 5;
        }
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            return 6;
        }
    }

    // Stage 4: file_exists must return false for a bogus path.
    let bogus = std::path::Path::new("/nonexistent/path/runsible-conn-f30-xyz");
    match conn.file_exists(bogus) {
        Ok(false) => {}
        Ok(true) => {
            let _ = std::fs::remove_file(&path);
            return 7;
        }
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            return 8;
        }
    }

    // Stage 5: slurp must return the exact bytes that were written.
    let slurped = match conn.slurp(&path) {
        Ok(b) => b,
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            return 9;
        }
    };
    let _ = std::fs::remove_file(&path);
    if slurped.as_slice() != payload {
        return 10;
    }

    0
}

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
            ca_key_path: None,
            ca_principal: None,
            ca_validity_seconds: None,
        };

        let conn = spec.build();
        let cmd = base_cmd(vec!["echo", "spec_build_ok"]);
        let out = conn.exec(&cmd).await.expect("exec via spec.build()");
        assert_eq!(out.rc, 0);
        assert_eq!(out.stdout, b"spec_build_ok\n");
    }
}
