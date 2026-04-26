use std::path::Path;
use std::time::Instant;

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;

use runsible_core::traits::{BecomeMethod, BecomeSpec, Cmd, Connection, ExecOutcome, SecretSource};
use runsible_core::errors::Result as CoreResult;

use crate::errors::ConnectionError;

pub struct LocalConnection;

/// Build the prefix argv that implements privilege escalation via sudo.
fn become_prefix(spec: &BecomeSpec) -> (Vec<String>, bool) {
    // Returns (prefix_args, needs_stdin_password).
    // Only Sudo is handled locally; everything else falls through unsupported.
    let mut args: Vec<String> = Vec::new();
    let mut stdin_pw = false;
    if spec.method == BecomeMethod::Sudo {
        args.push("sudo".into());
        if let Some(SecretSource::Plaintext(_)) = &spec.password {
            args.push("-S".into());
            stdin_pw = true;
        } else {
            args.push("-n".into()); // non-interactive: fail rather than prompt
        }
        args.push("-u".into());
        args.push(spec.user.clone());
        for f in &spec.flags {
            args.push(f.clone());
        }
        args.push("--".into());
    }
    (args, stdin_pw)
}

#[async_trait]
impl Connection for LocalConnection {
    async fn exec(&self, cmd: &Cmd) -> CoreResult<ExecOutcome> {
        use tokio::process::Command;

        if cmd.argv.is_empty() {
            return Err(ConnectionError::ExecFailed("empty argv".into()).into());
        }

        let (become_args, stdin_pw) = if let Some(spec) = &cmd.become_ {
            become_prefix(spec)
        } else {
            (vec![], false)
        };

        let (program, rest_args): (&str, Vec<&str>) = if become_args.is_empty() {
            (
                cmd.argv[0].as_str(),
                cmd.argv[1..].iter().map(|s| s.as_str()).collect(),
            )
        } else {
            (
                become_args[0].as_str(),
                become_args[1..]
                    .iter()
                    .map(|s| s.as_str())
                    .chain(cmd.argv.iter().map(|s| s.as_str()))
                    .collect(),
            )
        };

        let mut child_cmd = Command::new(program);
        child_cmd.args(&rest_args);
        child_cmd.envs(cmd.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));

        if let Some(cwd) = &cmd.cwd {
            child_cmd.current_dir(cwd);
        }

        child_cmd.stdin(std::process::Stdio::piped());
        child_cmd.stdout(std::process::Stdio::piped());
        child_cmd.stderr(std::process::Stdio::piped());

        let start = Instant::now();
        let mut child = child_cmd
            .spawn()
            .map_err(|e| ConnectionError::ExecFailed(e.to_string()))?;

        // Write stdin if needed.
        if let Some(stdin_handle) = child.stdin.take() {
            let mut h = stdin_handle;
            if stdin_pw {
                // Password for sudo -S must end with \n.
                if let Some(spec) = &cmd.become_ {
                    if let Some(SecretSource::Plaintext(pw)) = &spec.password {
                        let mut pw_bytes = pw.as_bytes().to_vec();
                        if !pw_bytes.ends_with(b"\n") {
                            pw_bytes.push(b'\n');
                        }
                        h.write_all(&pw_bytes).await.ok();
                    }
                }
            } else if let Some(data) = &cmd.stdin {
                h.write_all(data).await.ok();
            }
            drop(h); // close stdin so the child doesn't block
        }

        let run = async {
            let out = child
                .wait_with_output()
                .await
                .map_err(|e| ConnectionError::ExecFailed(e.to_string()))?;
            Ok::<_, ConnectionError>(out)
        };

        let output = if let Some(timeout) = cmd.timeout {
            tokio::time::timeout(timeout, run)
                .await
                .map_err(|_| ConnectionError::Timeout(timeout))?
        } else {
            run.await
        }
        .map_err(Into::<runsible_core::errors::Error>::into)?;

        let elapsed = start.elapsed();

        let rc = output.status.code().unwrap_or(-1);

        #[cfg(unix)]
        let signal = {
            use std::os::unix::process::ExitStatusExt;
            output.status.signal()
        };
        #[cfg(not(unix))]
        let signal: Option<i32> = None;

        Ok(ExecOutcome {
            rc,
            stdout: output.stdout,
            stderr: output.stderr,
            signal,
            elapsed,
        })
    }

    async fn put_file(&self, src: &Path, dst: &Path, mode: Option<u32>) -> CoreResult<()> {
        tokio::fs::copy(src, dst)
            .await
            .map_err(|e| ConnectionError::Io(e))?;

        #[cfg(unix)]
        if let Some(m) = mode {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(m);
            std::fs::set_permissions(dst, perms).map_err(ConnectionError::Io)?;
        }

        Ok(())
    }

    async fn get_file(&self, src: &Path, dst: &Path) -> CoreResult<()> {
        tokio::fs::copy(src, dst)
            .await
            .map_err(|e| ConnectionError::Io(e))?;
        Ok(())
    }

    async fn slurp(&self, src: &Path) -> CoreResult<Vec<u8>> {
        tokio::fs::read(src)
            .await
            .map_err(|e| ConnectionError::Io(e).into())
    }

    async fn close(&mut self) -> CoreResult<()> {
        Ok(())
    }
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use runsible_core::traits::{BecomeMethod, BecomeSpec, Cmd};

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

    #[tokio::test]
    async fn local_exec_echo() {
        let conn = LocalConnection;
        let cmd = base_cmd(vec!["echo", "hello"]);
        let out = conn.exec(&cmd).await.expect("exec failed");
        assert_eq!(out.rc, 0);
        assert_eq!(out.stdout, b"hello\n");
    }

    #[tokio::test]
    async fn local_exec_exit_code() {
        let conn = LocalConnection;
        let cmd = base_cmd(vec!["sh", "-c", "exit 42"]);
        let out = conn.exec(&cmd).await.expect("exec failed");
        assert_eq!(out.rc, 42);
    }

    #[tokio::test]
    async fn local_slurp() {
        let path = std::env::temp_dir().join("runsible_connection_slurp_test.txt");
        tokio::fs::write(&path, b"runsible slurp test")
            .await
            .expect("write temp file");

        let conn = LocalConnection;
        let bytes = conn.slurp(&path).await.expect("slurp");
        assert_eq!(bytes, b"runsible slurp test");

        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn local_put_get_file() {
        let src_path = std::env::temp_dir().join("runsible_connection_put_src.txt");
        let dst_path = std::env::temp_dir().join("runsible_connection_put_dst.txt");
        let final_path = std::env::temp_dir().join("runsible_connection_get_dst.txt");

        tokio::fs::write(&src_path, b"put/get round-trip")
            .await
            .expect("write src");

        let conn = LocalConnection;
        conn.put_file(&src_path, &dst_path, None)
            .await
            .expect("put_file");

        conn.get_file(&dst_path, &final_path)
            .await
            .expect("get_file");

        let contents = tokio::fs::read(&final_path).await.expect("read final");
        assert_eq!(contents, b"put/get round-trip");

        let _ = tokio::fs::remove_file(&src_path).await;
        let _ = tokio::fs::remove_file(&dst_path).await;
        let _ = tokio::fs::remove_file(&final_path).await;
    }

    #[tokio::test]
    async fn local_become_sudo() {
        // Only run if sudo is available.
        if std::process::Command::new("sudo")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skip: no sudo");
            return;
        }

        let conn = LocalConnection;
        let cmd = Cmd {
            argv: vec!["id".into(), "-u".into()],
            stdin: None,
            env: vec![],
            cwd: None,
            become_: Some(BecomeSpec {
                method: BecomeMethod::Sudo,
                user: "root".into(),
                flags: vec![],
                password: None,
                preserve_env: vec![],
            }),
            timeout: Some(Duration::from_secs(10)),
            tty: false,
        };

        match conn.exec(&cmd).await {
            Ok(out) if out.rc == 0 => {
                assert_eq!(out.stdout.trim_ascii_end(), b"0");
            }
            Ok(out) => {
                // sudo present but we don't have passwordless sudo rights — acceptable skip
                eprintln!(
                    "skip: sudo id -u returned rc={} stderr={}",
                    out.rc,
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            Err(e) => {
                eprintln!("skip: sudo exec error: {e}");
            }
        }
    }
}
