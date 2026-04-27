//! Synchronous LocalConnection facade — uses std::process / std::fs.
//!
//! Implements the `runsible_core::traits::SyncConnection` trait. M1 modules
//! (command, shell, copy, file, get_url) consume this to run on the controller
//! machine without an async runtime.

use std::path::Path;
use std::process::Command;
use std::time::Instant;

use runsible_core::errors::Result as CoreResult;
use runsible_core::traits::{BecomeMethod, Cmd, ExecOutcome, SecretSource, SyncConnection};

use crate::errors::ConnectionError;

pub struct LocalSync;

impl SyncConnection for LocalSync {
    fn exec(&self, cmd: &Cmd) -> CoreResult<ExecOutcome> {
        if cmd.argv.is_empty() {
            return Err(ConnectionError::ExecFailed("empty argv".into()).into());
        }

        let (program, args) = if let Some(b) = &cmd.become_ {
            build_become_argv(&cmd.argv, b)
        } else {
            (cmd.argv[0].clone(), cmd.argv[1..].to_vec())
        };

        let mut command = Command::new(&program);
        command.args(&args);

        for (k, v) in &cmd.env {
            command.env(k, v);
        }
        if let Some(cwd) = &cmd.cwd {
            command.current_dir(cwd);
        }

        let mut child = command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(ConnectionError::Io)?;

        if let Some(stdin_bytes) = &cmd.stdin {
            use std::io::Write;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(stdin_bytes).map_err(ConnectionError::Io)?;
            }
        } else if let Some(b) = &cmd.become_ {
            // sudo with -S reads password from stdin
            if matches!(b.method, BecomeMethod::Sudo) {
                if let Some(SecretSource::Plaintext(pw)) = &b.password {
                    use std::io::Write;
                    if let Some(mut stdin) = child.stdin.take() {
                        let mut s = pw.clone();
                        s.push('\n');
                        stdin.write_all(s.as_bytes()).map_err(ConnectionError::Io)?;
                    }
                }
            }
        }

        let started = Instant::now();
        let output = child.wait_with_output().map_err(ConnectionError::Io)?;
        let elapsed = started.elapsed();

        let signal = {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                output.status.signal()
            }
            #[cfg(not(unix))]
            { None }
        };

        Ok(ExecOutcome {
            rc: output.status.code().unwrap_or(-1),
            stdout: output.stdout,
            stderr: output.stderr,
            signal,
            elapsed,
        })
    }

    fn put_file(&self, src: &Path, dst: &Path, mode: Option<u32>) -> CoreResult<()> {
        if let Some(parent) = dst.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        std::fs::copy(src, dst).map_err(ConnectionError::Io)?;
        if let Some(mode) = mode {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perm = std::fs::Permissions::from_mode(mode);
                std::fs::set_permissions(dst, perm).map_err(ConnectionError::Io)?;
            }
            #[cfg(not(unix))]
            { let _ = mode; }
        }
        Ok(())
    }

    fn slurp(&self, src: &Path) -> CoreResult<Vec<u8>> {
        std::fs::read(src).map_err(|e| ConnectionError::Io(e).into())
    }

    fn file_exists(&self, path: &Path) -> CoreResult<bool> {
        Ok(path.exists())
    }
}

fn build_become_argv(argv: &[String], b: &runsible_core::traits::BecomeSpec) -> (String, Vec<String>) {
    let mut out = vec!["-n".to_string(), "-u".to_string(), b.user.clone()];
    if matches!(b.password, Some(SecretSource::Plaintext(_))) {
        out.insert(0, "-S".into());
    }
    out.extend(b.flags.iter().cloned());
    out.push("--".into());
    out.extend(argv.iter().cloned());
    let bin = match b.method {
        BecomeMethod::Sudo => "sudo",
        BecomeMethod::Su => "su",
        BecomeMethod::Doas => "doas",
        _ => "sudo",
    };
    (bin.to_string(), out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(argv: Vec<&str>) -> Cmd {
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

    #[test]
    fn sync_exec_echo() {
        let conn = LocalSync;
        let out = conn.exec(&base(vec!["echo", "hi"])).unwrap();
        assert_eq!(out.rc, 0);
        assert_eq!(out.stdout, b"hi\n");
    }

    #[test]
    fn sync_exec_exit_code() {
        let conn = LocalSync;
        let out = conn.exec(&base(vec!["sh", "-c", "exit 7"])).unwrap();
        assert_eq!(out.rc, 7);
    }

    #[test]
    fn sync_put_and_slurp() {
        let conn = LocalSync;
        let src = std::env::temp_dir().join(format!("rsl-sync-src-{}.txt", std::process::id()));
        let dst = std::env::temp_dir().join(format!("rsl-sync-dst-{}.txt", std::process::id()));
        std::fs::write(&src, b"payload").unwrap();
        conn.put_file(&src, &dst, Some(0o644)).unwrap();
        let bytes = conn.slurp(&dst).unwrap();
        assert_eq!(bytes, b"payload");
        assert!(conn.file_exists(&dst).unwrap());
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
    }

    #[test]
    fn sync_file_exists_false() {
        let conn = LocalSync;
        assert!(!conn.file_exists(Path::new("/nonexistent/path/xyz123")).unwrap());
    }

    // ── Added coverage ─────────────────────────────────────────────────

    /// Exec with env vars: FOO=bar then `sh -c 'echo $FOO'` -> stdout "bar\n".
    #[test]
    fn sync_exec_with_env_vars() {
        if std::process::Command::new("sh").arg("-c").arg("true").output().is_err() {
            eprintln!("skip: no sh available");
            return;
        }
        let conn = LocalSync;
        let cmd = Cmd {
            argv: vec!["sh".into(), "-c".into(), "echo $FOO".into()],
            stdin: None,
            env: vec![("FOO".into(), "bar".into())],
            cwd: None,
            become_: None,
            timeout: None,
            tty: false,
        };
        let out = conn.exec(&cmd).unwrap();
        assert_eq!(out.rc, 0);
        assert_eq!(out.stdout, b"bar\n");
    }

    /// Exec with cwd set: `pwd` should print the cwd.
    #[test]
    fn sync_exec_with_cwd() {
        if std::process::Command::new("pwd").output().is_err() {
            eprintln!("skip: no pwd available");
            return;
        }
        let dir = std::env::temp_dir();
        let conn = LocalSync;
        let cmd = Cmd {
            argv: vec!["pwd".into()],
            stdin: None,
            env: vec![],
            cwd: Some(dir.clone()),
            become_: None,
            timeout: None,
            tty: false,
        };
        let out = conn.exec(&cmd).unwrap();
        assert_eq!(out.rc, 0);
        // pwd output ends with \n; canonicalize both sides for symlink tolerance
        let stdout = String::from_utf8_lossy(&out.stdout).trim_end().to_string();
        let want = std::fs::canonicalize(&dir)
            .ok()
            .and_then(|p| p.to_str().map(String::from));
        let got = std::fs::canonicalize(&stdout)
            .ok()
            .and_then(|p| p.to_str().map(String::from));
        match (want, got) {
            (Some(w), Some(g)) => assert_eq!(w, g),
            _ => assert_eq!(stdout, dir.to_string_lossy()),
        }
    }

    /// stdin piped to `cat` is echoed back.
    #[test]
    fn sync_exec_pipe_stdin() {
        if std::process::Command::new("cat").arg("/dev/null").output().is_err() {
            eprintln!("skip: no cat available");
            return;
        }
        let conn = LocalSync;
        let cmd = Cmd {
            argv: vec!["cat".into()],
            stdin: Some(b"hello".to_vec()),
            env: vec![],
            cwd: None,
            become_: None,
            timeout: None,
            tty: false,
        };
        let out = conn.exec(&cmd).unwrap();
        assert_eq!(out.rc, 0);
        assert_eq!(out.stdout, b"hello");
    }

    /// Exec with an empty argv must surface an error.
    #[test]
    fn sync_exec_empty_argv_errors() {
        let conn = LocalSync;
        let cmd = Cmd {
            argv: vec![],
            stdin: None,
            env: vec![],
            cwd: None,
            become_: None,
            timeout: None,
            tty: false,
        };
        let res = conn.exec(&cmd);
        assert!(res.is_err(), "expected error on empty argv");
    }

    /// Non-UTF8 stdout — use `printf '\xff\xfe'` and check the raw bytes.
    #[test]
    fn sync_exec_non_utf8_stdout() {
        if std::process::Command::new("printf").arg("hi").output().is_err() {
            eprintln!("skip: no printf available");
            return;
        }
        let conn = LocalSync;
        let cmd = Cmd {
            argv: vec!["printf".into(), "\\xff\\xfe".into()],
            stdin: None,
            env: vec![],
            cwd: None,
            become_: None,
            timeout: None,
            tty: false,
        };
        let out = conn.exec(&cmd).unwrap();
        assert_eq!(out.rc, 0);
        assert_eq!(out.stdout, vec![0xff, 0xfe]);
    }

    /// put_file should create parent directories if missing.
    #[test]
    fn sync_put_file_creates_parent_dirs() {
        let conn = LocalSync;
        let pid = std::process::id();
        let src = std::env::temp_dir().join(format!("rsl-sync-mkparent-src-{}.txt", pid));
        std::fs::write(&src, b"mkparent").unwrap();
        let nested = std::env::temp_dir()
            .join(format!("rsl-sync-mkparent-{}", pid))
            .join("a")
            .join("b")
            .join("c");
        let dst = nested.join("dst.txt");
        // Make sure the deep dir does not exist before the call
        let _ = std::fs::remove_dir_all(
            std::env::temp_dir().join(format!("rsl-sync-mkparent-{}", pid)),
        );

        conn.put_file(&src, &dst, None).expect("put_file mkparent");
        let bytes = std::fs::read(&dst).unwrap();
        assert_eq!(bytes, b"mkparent");

        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_dir_all(
            std::env::temp_dir().join(format!("rsl-sync-mkparent-{}", pid)),
        );
    }

    /// put_file overwrites an existing destination file.
    #[test]
    fn sync_put_file_overwrites_existing() {
        let conn = LocalSync;
        let pid = std::process::id();
        let src = std::env::temp_dir().join(format!("rsl-sync-overwrite-src-{}.txt", pid));
        let dst = std::env::temp_dir().join(format!("rsl-sync-overwrite-dst-{}.txt", pid));

        std::fs::write(&dst, b"OLD").unwrap();
        std::fs::write(&src, b"NEW").unwrap();

        conn.put_file(&src, &dst, None).expect("overwrite");
        let bytes = std::fs::read(&dst).unwrap();
        assert_eq!(bytes, b"NEW");

        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
    }

    /// file_exists: false for a missing path, true for a real tempfile.
    #[test]
    fn sync_file_exists_true_and_false() {
        let conn = LocalSync;
        let pid = std::process::id();
        let exists_path = std::env::temp_dir().join(format!("rsl-sync-fe-{}.txt", pid));
        std::fs::write(&exists_path, b"x").unwrap();

        assert!(!conn
            .file_exists(Path::new("/nonexistent/path/xyz123-rsl"))
            .unwrap());
        assert!(conn.file_exists(&exists_path).unwrap());

        let _ = std::fs::remove_file(&exists_path);
    }
}
