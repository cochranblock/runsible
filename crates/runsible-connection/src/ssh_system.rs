use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;

use runsible_core::traits::{BecomeMethod, BecomeSpec, Cmd, Connection, ExecOutcome, SecretSource};
use runsible_core::errors::Result as CoreResult;

use crate::errors::ConnectionError;

/// Wraps the system `ssh` / `scp` binaries. Suitable for M0 before russh lands.
pub struct SshSystemConnection {
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<PathBuf>,
    pub control_path: Option<String>,
    pub connect_timeout: Duration,
    pub extra_args: Vec<String>,
}

/// Single-quote–escape a string for inclusion in a remote shell command.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

impl SshSystemConnection {
    /// Build the common ssh option flags (port, identity, control, timeout, extras).
    fn ssh_options(&self) -> Vec<String> {
        let mut opts: Vec<String> = Vec::new();

        opts.push("-o".into());
        opts.push("BatchMode=yes".into());
        opts.push("-o".into());
        opts.push(format!(
            "ConnectTimeout={}",
            self.connect_timeout.as_secs().max(1)
        ));

        if let Some(p) = self.port {
            opts.push("-p".into());
            opts.push(p.to_string());
        }
        if let Some(id) = &self.identity_file {
            opts.push("-i".into());
            opts.push(id.to_string_lossy().into_owned());
        }
        if let Some(cp) = &self.control_path {
            opts.push("-o".into());
            opts.push("ControlMaster=auto".into());
            opts.push("-o".into());
            opts.push(format!("ControlPath={cp}"));
            opts.push("-o".into());
            opts.push("ControlPersist=60".into());
        }
        for a in &self.extra_args {
            opts.push(a.clone());
        }
        opts
    }

    /// Return `[user@]host` string.
    fn target(&self) -> String {
        match &self.user {
            Some(u) => format!("{}@{}", u, self.host),
            None => self.host.clone(),
        }
    }

    /// Build the remote command string that will be sent to `sh -c` on the far end.
    fn remote_cmd_string(cmd: &Cmd, sudo_prefix: Vec<String>, _stdin_pw: bool) -> String {
        let prefix_str: String = sudo_prefix
            .iter()
            .map(|s| shell_quote(s))
            .collect::<Vec<_>>()
            .join(" ");

        let argv_str: String = cmd
            .argv
            .iter()
            .map(|s| shell_quote(s))
            .collect::<Vec<_>>()
            .join(" ");

        // Env vars
        let env_str: String = cmd
            .env
            .iter()
            .map(|(k, v)| format!("{}={}", shell_quote(k), shell_quote(v)))
            .collect::<Vec<_>>()
            .join(" ");

        let env_prefix = if env_str.is_empty() {
            String::new()
        } else {
            format!("env {env_str} ")
        };

        let cwd_prefix = if let Some(cwd) = &cmd.cwd {
            format!("cd {} && ", shell_quote(&cwd.to_string_lossy()))
        } else {
            String::new()
        };

        if prefix_str.is_empty() {
            format!("{cwd_prefix}{env_prefix}{argv_str}")
        } else {
            format!("{cwd_prefix}{env_prefix}{prefix_str} {argv_str}")
        }
    }

    /// Spawn an `ssh` process and capture its output, with optional timeout.
    async fn run_ssh(
        &self,
        remote_cmd: &str,
        stdin_data: Option<Vec<u8>>,
        timeout: Option<Duration>,
    ) -> Result<(i32, Vec<u8>, Vec<u8>), ConnectionError> {
        use tokio::process::Command;

        let mut args: Vec<String> = self.ssh_options();
        args.push(self.target());
        args.push(remote_cmd.to_string());

        let mut child = Command::new("ssh")
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ConnectionError::Ssh(e.to_string()))?;

        if let Some(mut stdin_h) = child.stdin.take() {
            if let Some(data) = stdin_data {
                stdin_h.write_all(&data).await.ok();
            }
            drop(stdin_h);
        }

        let run = async {
            child
                .wait_with_output()
                .await
                .map_err(|e| ConnectionError::Ssh(e.to_string()))
        };

        let output = if let Some(t) = timeout {
            tokio::time::timeout(t, run)
                .await
                .map_err(|_| ConnectionError::Timeout(t))?
        } else {
            run.await
        }?;

        let rc = output.status.code().unwrap_or(-1);
        Ok((rc, output.stdout, output.stderr))
    }
}

/// Build sudo prefix for remote execution.
fn remote_become_prefix(spec: &BecomeSpec) -> (Vec<String>, bool) {
    let mut args: Vec<String> = Vec::new();
    let mut stdin_pw = false;

    if spec.method == BecomeMethod::Sudo {
        args.push("sudo".into());
        if let Some(SecretSource::Plaintext(_)) = &spec.password {
            args.push("-S".into());
            stdin_pw = true;
        } else {
            args.push("-n".into());
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
impl Connection for SshSystemConnection {
    async fn exec(&self, cmd: &Cmd) -> CoreResult<ExecOutcome> {
        let (sudo_prefix, stdin_pw) = if let Some(spec) = &cmd.become_ {
            remote_become_prefix(spec)
        } else {
            (vec![], false)
        };

        let remote_cmd = SshSystemConnection::remote_cmd_string(cmd, sudo_prefix, stdin_pw);

        // Build stdin bytes: sudo password + optional user-provided stdin.
        let stdin_bytes: Option<Vec<u8>> = if stdin_pw {
            if let Some(spec) = &cmd.become_ {
                if let Some(SecretSource::Plaintext(pw)) = &spec.password {
                    let mut v = pw.as_bytes().to_vec();
                    if !v.ends_with(b"\n") {
                        v.push(b'\n');
                    }
                    if let Some(extra) = &cmd.stdin {
                        v.extend_from_slice(extra);
                    }
                    Some(v)
                } else {
                    cmd.stdin.clone()
                }
            } else {
                cmd.stdin.clone()
            }
        } else {
            cmd.stdin.clone()
        };

        let start = Instant::now();
        let (rc, stdout, stderr) = self
            .run_ssh(&remote_cmd, stdin_bytes, cmd.timeout)
            .await
            .map_err(Into::<runsible_core::errors::Error>::into)?;
        let elapsed = start.elapsed();

        Ok(ExecOutcome {
            rc,
            stdout,
            stderr,
            signal: None, // not easily detectable through system ssh exit code
            elapsed,
        })
    }

    async fn put_file(&self, src: &Path, dst: &Path, mode: Option<u32>) -> CoreResult<()> {
        use tokio::process::Command;

        let mut scp_args: Vec<String> = Vec::new();
        if let Some(p) = self.port {
            scp_args.push("-P".into());
            scp_args.push(p.to_string());
        }
        if let Some(id) = &self.identity_file {
            scp_args.push("-i".into());
            scp_args.push(id.to_string_lossy().into_owned());
        }
        scp_args.push(src.to_string_lossy().into_owned());
        scp_args.push(format!(
            "{}:{}",
            self.target(),
            dst.to_string_lossy()
        ));

        let status = Command::new("scp")
            .args(&scp_args)
            .status()
            .await
            .map_err(|e| ConnectionError::Scp(e.to_string()))?;

        if !status.success() {
            return Err(ConnectionError::Scp(format!(
                "scp exited with {:?}",
                status.code()
            ))
            .into());
        }

        if let Some(m) = mode {
            let chmod_cmd = format!("chmod {:o} {}", m, shell_quote(&dst.to_string_lossy()));
            let (rc, _, stderr) = self.run_ssh(&chmod_cmd, None, None).await
                .map_err(Into::<runsible_core::errors::Error>::into)?;
            if rc != 0 {
                return Err(ConnectionError::Ssh(format!(
                    "chmod failed: {}",
                    String::from_utf8_lossy(&stderr)
                ))
                .into());
            }
        }

        Ok(())
    }

    async fn get_file(&self, src: &Path, dst: &Path) -> CoreResult<()> {
        use tokio::process::Command;

        let mut scp_args: Vec<String> = Vec::new();
        if let Some(p) = self.port {
            scp_args.push("-P".into());
            scp_args.push(p.to_string());
        }
        if let Some(id) = &self.identity_file {
            scp_args.push("-i".into());
            scp_args.push(id.to_string_lossy().into_owned());
        }
        scp_args.push(format!(
            "{}:{}",
            self.target(),
            src.to_string_lossy()
        ));
        scp_args.push(dst.to_string_lossy().into_owned());

        let status = Command::new("scp")
            .args(&scp_args)
            .status()
            .await
            .map_err(|e| ConnectionError::Scp(e.to_string()))?;

        if !status.success() {
            return Err(ConnectionError::Scp(format!(
                "scp exited with {:?}",
                status.code()
            ))
            .into());
        }

        Ok(())
    }

    async fn slurp(&self, src: &Path) -> CoreResult<Vec<u8>> {
        let remote_cmd = format!("cat {}", shell_quote(&src.to_string_lossy()));
        let (rc, stdout, stderr) = self
            .run_ssh(&remote_cmd, None, None)
            .await
            .map_err(Into::<runsible_core::errors::Error>::into)?;

        if rc != 0 {
            return Err(ConnectionError::Ssh(format!(
                "cat failed (rc={rc}): {}",
                String::from_utf8_lossy(&stderr)
            ))
            .into());
        }

        Ok(stdout)
    }

    async fn close(&mut self) -> CoreResult<()> {
        if let Some(cp) = &self.control_path {
            // Ask the control master to exit. Ignore errors — it may not be running.
            let _ = tokio::process::Command::new("ssh")
                .args(["-O", "exit", "-o", &format!("ControlPath={cp}"), &self.host])
                .output()
                .await;
        }
        Ok(())
    }
}
