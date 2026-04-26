# runsible — `runsible-connection`

## 1. Mission

`runsible-connection` is the crate that talks to remote hosts. It is both a library (linked into `runsible-playbook`, `runsible`, `runsible-pull`, and `runsible-console`) and an internal binary (the per-host worker that holds long-lived multiplexed connections across many tasks against one host). The default transport is the system OpenSSH client driven through `ControlMaster=auto`, so we inherit `~/.ssh/config` ergonomics and OpenSSH's hardened multiplexing. The fallback transport is `russh` — a pure-Rust SSH client — for embedded use, sandboxes that block fork/exec, single-binary distributions, or platforms where ControlMaster is unhealthy. The crate also handles the local transport, subprocess-wrapped transports (kubectl/docker/podman), typed privilege escalation, file transfer (scp/sftp/cat-piped), TTY allocation, secret-keyring password handling, and a per-host preflight that aborts cleanly before any task ships if connectivity, exec, or escalation are broken. Every exec returns a structured `ExecOutcome` — never a stream of stringly-typed log lines.

## 2. Scope

**In scope.**
- SSH transport via two backends: the system `openssh` (shell-out + ControlMaster) and `russh` (pure-Rust).
- The `local` transport (`tokio::process::Command` + `tokio::fs`).
- Subprocess-wrapper transports: `kubectl exec`, `docker exec`, `podman exec`, `lxc exec`. These are not their own SSH-like sessions; they are argv builders that wrap a target's command and dispatch through the local transport (or, optionally, through SSH to a host running the wrapper).
- Privilege escalation (become): sudo, su, doas, pbrun, pfexec, dzdo, ksu, runas (Windows-targeted but stub-only in v1), machinectl, sesu, and a `Custom` escape hatch.
- File transfer: scp / sftp / cat-piped (stdin pipelining for small payloads). Auto-pick by host capability with explicit override.
- TTY allocation (`-tt`) on demand, only when a become method requires one (sudo with `requiretty`, su, etc.).
- Password handling routed through a `SecretSource` enum that defaults to the system keyring (libsecret on Linux, Keychain on macOS, Credential Manager on Windows). Plaintext is supported but discouraged and logged.
- Multiplexed sessions: ControlMaster for `SshSystem`; per-(host, user) channel pool for `Russh` with health-check ping.
- Structured exec result: `ExecOutcome { rc, stdout, stderr, signal, elapsed }`. No stringly-typed parsing.
- Per-host preflight: connect → exec `true` → `whoami` under become if configured → tmp file write+delete if file ops are needed.
- Host-key verification honoring `~/.ssh/known_hosts` and `/etc/ssh/ssh_known_hosts`.
- The internal binary `runsible-connection` (long-lived per-host worker) speaking JSON-RPC over a Unix socket.

**Out of scope for v1.**
- WinRM. Windows-managed-from-Linux is deferred to v1.5; the Windows persona is explicitly out of v1 (per the user-story analysis, §4 of `00-user-story-analysis.md`).
- Cloud-specific transports as their own backends (AWS SSM session-manager, GCP IAP-tunnel). These are achievable today by configuring `~/.ssh/config` with a `ProxyCommand`, which the `SshSystem` backend honors automatically.
- Network-device CLIs (`network_cli`, `netconf`, `httpapi`). These move to a plugin model in a later release; the network persona is also explicitly out of v1.
- Python paramiko equivalence. Paramiko is replaced by `russh`; `connection = "paramiko"` in inventory is silently mapped to `russh` with a warning.
- Kerberos/GSSAPI as a first-class auth surface. This works today only via the `SshSystem` backend (because OpenSSH provides it for free); `Russh` does not gain GSSAPI in v1.
- A stable third-party connection-plugin trait. The `Connection` trait exists internally but is not stabilized for out-of-tree implementors until the design settles.

## 3. The `Connection` trait

```rust
#[async_trait]
pub trait Connection: Send + Sync {
    /// Execute a command. Returns the structured outcome.
    async fn exec(&self, cmd: &Cmd) -> Result<ExecOutcome>;

    /// Copy a controller-side file to the target.
    async fn put_file(&self, src: &Path, dst: &Path, mode: Option<u32>) -> Result<()>;

    /// Copy a target-side file to the controller.
    async fn get_file(&self, src: &Path, dst: &Path) -> Result<()>;

    /// Read a target-side file into memory (small files only — bounded).
    async fn slurp(&self, src: &Path) -> Result<Vec<u8>>;

    /// Cleanly close the underlying session(s).
    async fn close(&mut self) -> Result<()>;

    /// Capability matrix for this transport — e.g. whether `tty` is supported,
    /// whether `become` is supported, whether file transfer is supported, etc.
    fn capabilities(&self) -> Capabilities;
}

pub struct Cmd {
    /// Argv exec'd directly. No shell wrapping unless `become_` requires it.
    pub argv: Vec<String>,

    /// Optional bytes piped to the child's stdin.
    pub stdin: Option<Vec<u8>>,

    /// Environment variable additions (not replacement). The transport may
    /// refuse env propagation for some methods (kubectl, machinectl).
    pub env: Vec<(String, String)>,

    /// Working directory on the target. Honored where the transport supports it.
    pub cwd: Option<PathBuf>,

    /// Privilege escalation. None = run as the connection user.
    pub become_: Option<BecomeSpec>,

    /// Hard wall-clock timeout for the exec.
    pub timeout: Option<Duration>,

    /// Allocate a PTY. Default false; set true only when `become_` needs one.
    pub tty: bool,
}

pub struct ExecOutcome {
    pub rc: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// Set if the child was terminated by a signal (Unix only).
    pub signal: Option<i32>,
    pub elapsed: Duration,
}

pub struct Capabilities {
    pub supports_tty: bool,
    pub supports_become: bool,
    pub supports_file_transfer: bool,
    pub supports_env: bool,
    pub supports_cwd: bool,
    pub max_concurrent_channels: Option<u32>,
}
```

`exec` is the single hot path. Modules dispatch through it. File transfer uses `put_file` / `get_file` / `slurp` rather than overloading `exec` with cat-piped stdin, because file transfer needs a transport-aware fast path (sftp, scp, kubectl cp) that exec cannot model cleanly. `slurp` is bounded — the implementation refuses files above a configurable cap (default 16 MiB) so a misconfigured `slurp /var/log/messages` cannot OOM the controller.

## 4. Transports

### `SshSystem` — default

Wraps the system `ssh` binary via the [`openssh`](https://docs.rs/openssh) crate.

- Defaults: `ControlMaster=auto`, `ControlPath=~/.runsible/cm/%r@%h:%p`, `ControlPersist=60s`, `LogLevel=ERROR`. The control-path directory is created with mode `0700` at startup.
- Honors `~/.ssh/config`: `ProxyJump`, `ProxyCommand`, `IdentityFile`, `Match`, `Include`, `ServerAliveInterval`, GSSAPI, SmartCard/PKCS11 via `ssh-agent`.
- Reads stderr separately from stdout. `LogLevel=ERROR` suppresses banner noise so `stderr` carries module stderr, not "Authenticated to ...".
- File transfer: `SshFileTransferStrategy::{Auto, Scp, Sftp, Piped}` with `Auto` defaulting scp for small files, sftp when many transfers queue.
- Pipelining: when the module payload fits in the OS stdin buffer (~64 KiB Linux), source is piped to the interpreter's stdin via the SSH channel, skipping the temp-file dance. Enabled by default; fixes the unprivileged-become permissions problem (see `09-connection-templating-facts.md` §2.5).
- Two `openssh` modes: `Mux` (talks to the control socket — Linux/macOS, faster) is default where supported; `ProcessImpl` (one child `ssh` per command) elsewhere.

### `Russh` — fallback

Pure-Rust SSH on `russh` + `russh-keys` + `russh-sftp`.

- Selected with `connection = "russh"` in inventory, or automatically when `--no-system-ssh` is passed, the OpenSSH binary is missing, or ControlMaster cannot be established.
- Slower than OpenSSH in steady state (~100-200 µs more per exec on a warm session) but much faster to start. Wins for short-burst usage and cold runs.
- We re-implement what `~/.ssh/config` gives `SshSystem` for free: a small parser for `IdentityFile`, `User`, `Port`, `ProxyJump`, `Include`, and common `Match` predicates. `ProxyCommand` is *not* in v1 `Russh`; users with `ProxyCommand` setups stay on `SshSystem`.
- Known-hosts: reads `~/.ssh/known_hosts` + `/etc/ssh/ssh_known_hosts`, hashed entries, `@cert-authority`. New-host policy via `--host-key-checking` (§9).
- Crypto: Ed25519, RSA, ECDSA (P-256/P-384) keys; ChaCha20-Poly1305 / AES-GCM ciphers; curve25519-sha256 KEX. Defaults modern-only.

### `Local`

Runs commands on the controller via `tokio::process::Command`. File ops via `tokio::fs`. No transport overhead. Used for `localhost`, `delegate_to: localhost`, `connection: local`, and as the substrate for `SubprocessWrapper`.

### `SubprocessWrapper`

For `kubectl`, `docker`, `podman`, `lxc-attach`. Builds an argv:

```
[<wrapper>, <subcommand>, <target>, --, <argv>...]
```

Examples:
- `kubectl exec -i --tty=<bool> --namespace=<ns> <pod> --container=<container> -- <argv>...`
- `docker exec -i --tty=<bool> --user=<user> <container> <argv>...`
- `podman exec -i --tty=<bool> --user=<user> <container> <argv>...`

Configured per host:

```toml
[hosts.app1]
connection = "kubectl"
ansible_host = "my-pod"
kubectl.namespace = "default"
kubectl.container = "app"
kubectl.context = "prod-cluster"
```

File transfer is `kubectl cp` / `docker cp` / `podman cp`. The wrapper itself uses the `Local` substrate to spawn the child; if the wrapper needs to run on a remote host (rare — e.g. a kubectl context that lives on a bastion), the user composes by setting `kubectl.via_host = "bastion"` and the wrapper runs through `SshSystem`.

### Selection

The default transport is `SshSystem`. The user picks per host or per group via inventory:

```toml
[groups.k8s_pods]
connection = "kubectl"

[hosts.bastion]
connection = "ssh"

[hosts.controller_local]
connection = "local"

[hosts.embedded_box]
connection = "russh"
```

A play-level or task-level override (`connection = "..."`) wins over the inventory value. CLI `-c/--connection` wins over both. There is also a proposed `connection = "auto"` that probes `SshSystem` first and falls back to `Russh` if ControlMaster is unhealthy — see §15.

## 5. Become

Privilege escalation is a typed sub-document, not a flat keyword grab-bag (one of the redesigns from `11-poor-decisions.md` §16):

```rust
pub struct BecomeSpec {
    pub method: BecomeMethod,
    pub user: String,
    pub flags: Vec<String>,
    pub password: Option<SecretSource>,
    /// Env vars to preserve across the become boundary.
    /// Whether they actually survive depends on the method; sudo will need
    /// matching `Defaults env_keep` on the target.
    pub preserve_env: Vec<String>,
}

pub enum BecomeMethod {
    Sudo,
    Su,
    Doas,
    Pbrun,
    Pfexec,
    Dzdo,
    Ksu,
    Runas,        // stub in v1, full in v1.5 with WinRM
    Machinectl,
    Sesu,
    Custom(String), // operator-supplied wrapper
}

pub enum SecretSource {
    /// Read at the moment of use from the system keyring.
    Keyring { service: String, key: String },
    /// A file descriptor inherited from the parent (one-shot).
    Pipe(RawFd),
    /// Discouraged; only honored if the user explicitly opts in.
    Plaintext(String),
}
```

`runsible-connection` wraps `cmd.argv` per method:

- **Sudo.** `["sudo", "-H", "-S", "-n", "-u", &user, "--", "/bin/sh", "-c", &joined]` plus user `flags`. `-S` reads the password from stdin (streamed through the SSH channel — never via env or `/proc/<pid>/cmdline`). `-n` makes missing-password failure fast and structured. `-H` resets `HOME`. If the host is observed to require a TTY ("sudo: a terminal is required"), we transparently reconnect with `tty: true` and cache the observation.
- **Su.** `["su", "-", &user, "-s", "/bin/sh", "-c", &joined]`. No `-S` equivalent; password fed by writing to the channel after regex-matching the prompt (default `(?i)password.*:`). Always requests a TTY.
- **Doas.** `["doas", "-u", &user, "--", "/bin/sh", "-c", &joined]`. Reads stdin if needed. Honors `/etc/doas.conf`.
- **Pbrun.** `["pbrun", "-u", &user, "/bin/sh", "-c", &joined]`.
- **Pfexec.** `["pfexec", "/bin/sh", "-c", &joined]`. Solaris/illumos; user implicit.
- **Dzdo.** `["dzdo", "-u", &user, "--", "/bin/sh", "-c", &joined]`. Sudo-equivalent; `-S` semantics carry over.
- **Ksu.** `["ksu", &user, "-q", "-e", "/bin/sh", "-c", &joined]`. Requires a valid TGT.
- **Runas.** Stub in v1 — structured "not implemented; use v1.5+ with WinRM" error. Argv shape recorded for v1.5.
- **Machinectl.** `["machinectl", "shell", &format!("{}@", user), "/bin/sh", "-c", &joined]`. Fresh systemd session — populates `XDG_RUNTIME_DIR`, `DBUS_SESSION_BUS_ADDRESS`.
- **Sesu.** `["sesu", "-u", &user, "/bin/sh", "-c", &joined]`. CA Privileged Access Manager.
- **Custom(name).** `[name, &user, "/bin/sh", "-c", &joined]`. Site-local wrapper escape hatch.

If the connection user is already root and `become_user` resolves to root, we short-circuit and skip the wrapper entirely. Argv joining is delegated to `shell-escape` so quotes, dollar signs, backticks, and backslashes survive the `sh -c` boundary.

## 6. Multiplexing

### `SshSystem`

`openssh::Session` opens a master channel on first use; subsequent commands multiplex over it. We never open a second `Session` per host within one process. Two failure modes:

- **Stale control socket.** Dead master + lingering socket file makes `openssh` refuse with a confusing error. On startup we walk `~/.runsible/cm/` and unlink any socket whose master PID is not alive (best-effort, via `/proc` or `lsof`).
- **MaxSessions ceiling.** OpenSSH defaults `MaxSessions=10`. We expose `max_concurrent_channels` per-host (default 5, deliberately under MaxSessions for headroom); execs queue when the ceiling is hit rather than failing.

### `Russh`

A `RusshPool` keyed by `(host, port, user, identity_file)`. Each entry holds one `russh::client::Handle` and a `Vec<russh::Channel>`. Channel reuse across exec calls *is* the multiplexing. Health monitored via `keepalive@openssh.com` every 30 s; failure tears down the handle and the next exec opens fresh.

Pool size: 5 channels per connection by default, configurable via `[connection.russh] max_channels = N`. Above the cap, execs queue.

OpenSSH multiplexing stays the default because it is hardened through years of ops use. `russh` channels are conceptually cleaner (no socket files) but less battle-tested. The pool is small and isolated so we can swap it later.

## 7. The internal binary

`runsible-connection` (the bin) is invoked by `runsible-playbook` per-host as a long-lived worker handling many tasks against one host. It speaks JSON-RPC over a Unix socket at `~/.runsible/pc/<pid>/<host>.sock`. This replaces Ansible's pickle-over-Unix-socket protocol — pickle is a Python-specific RCE surface; JSON-RPC is parseable, schema-checked, and debuggable with standard tools.

**Methods.**
- `exec({argv, stdin, env, cwd, become, timeout, tty}) -> {rc, stdout, stderr, signal, elapsed}`
- `put_file({src_bytes_b64 | src_url, dst, mode})`
- `get_file({src, dst}) -> {bytes_b64}` (chunked for large files)
- `slurp({src}) -> {bytes_b64}`
- `update_capabilities() -> Capabilities`
- `reset()` / `close()`

Bytes are base64-encoded inside the JSON envelope. Method-call ordering is strict per connection; the parent may open multiple JSON-RPC connections to one worker for concurrent execs.

**Lifecycle.** Spawned by `runsible-playbook` as part of preflight (§8). The worker binds the socket, writes a "ready" frame, and loops. Parent disconnect kills it via `prctl(PR_SET_PDEATHSIG, SIGTERM)` on Linux; SIGKILL elsewhere.

**This binary is INTERNAL.** Stand-alone invocation is for debugging only (`runsible-connection --debug --host <h> --user <u>`); the JSON-RPC schema is *not* a stable user-facing contract. `--help` says exactly that.

## 8. Pre-flight check

Before any task runs against a host, `runsible-connection` runs a per-host preflight:

1. **Establish connection.** Open the transport. For `SshSystem`, this means opening the ControlMaster. For `Russh`, this means handshake + auth. For `Local`, it's a no-op. Failures here surface as `Preflight::ConnectFailed { host, reason }` and abort the run for that host immediately.
2. **Verify exec.** Run `Cmd { argv: vec!["true".into()], .. Cmd::default() }`. If `rc != 0`, abort with `Preflight::ExecFailed`.
3. **Verify become.** If the play has any task with `become`, run `Cmd { argv: vec!["whoami".into()], become_: Some(spec), .. }`. Compare `stdout.trim()` to `spec.user`. Mismatches abort with `Preflight::BecomeFailed { expected, got }`.
4. **Verify file transfer.** If the play has any task that uses file transfer (copy, template, fetch, slurp), `put_file` a tiny tmp file (`/tmp/runsible-preflight-<uuid>`), `slurp` it back, compare bytes, then delete it.

A failed preflight aborts the run for that host immediately, with a clear structured error. This catches the "host #47 sudoers misconfigured" surprise from `00-user-story-analysis.md` (P1 persona pain) before any work ships. Ansible has no equivalent; this is one of the redesigns the user-story analysis explicitly identifies as a wedge.

The preflight is parallelized across hosts within the configured `forks` budget. A single host's preflight failure does not block other hosts' preflights; it only aborts that host's task plan.

## 9. Host key verification

The user-facing flag is `--host-key-checking <strict|accept-new|off>`:

- **`strict`** (formal default — but see "transitional default" below). Only known hosts are accepted. Unknown hosts are rejected immediately with a structured error. Honors `~/.ssh/known_hosts` and `/etc/ssh/ssh_known_hosts`. Equivalent to OpenSSH `StrictHostKeyChecking=yes`.
- **`accept-new`** (transitional default in v1). Unknown hosts are accepted on first contact and recorded; subsequent connections with a changed key are rejected. Equivalent to OpenSSH `StrictHostKeyChecking=accept-new`. We default to this because it matches OpenSSH ≥7.6's recommended setting and avoids a "first run breaks" surprise. v2 may flip to `strict` once tooling is mature.
- **`off`**. Ignore known_hosts entirely. Logged loudly; emitted as a structured warning on every connection.

Implementation reads both `~/.ssh/known_hosts` and `/etc/ssh/ssh_known_hosts`, supports hashed entries (`HashKnownHosts yes`), `@cert-authority` markers, `@revoked`, and the modern `ssh-ed25519`/`ssh-rsa`/`ecdsa-sha2-*` key types. The `SshSystem` backend gets all of this from OpenSSH for free; the `Russh` backend re-implements the parser via `russh-keys`.

A separate flag, `--known-hosts <path>`, lets the user point at an alternate file (useful in CI where the host-key set is project-scoped, not user-scoped).

## 10. Redesigns vs Ansible

Concrete improvements over Ansible's connection layer, citing `11-poor-decisions.md` §8 and adding runsible-specific moves.

- **Default to system OpenSSH with explicit `ControlMaster=auto` and `ControlPersist=60s`.** Ansible already does this; we keep it. `russh` is a real fallback, not a parallel implementation users have to learn.
- **Russh, not paramiko.** Paramiko is deprecated in Ansible 2.21 anyway; `russh` is the modern Tokio-native replacement.
- **Container exec backends as enum variants, not plugins.** kubectl/docker/podman/lxc-attach live in-tree as `SubprocessWrapper` variants. Plugin-loading deferred to v1.5+ per the "ruthlessly cut" stance in `00-user-story-analysis.md` §7.
- **JSON-RPC instead of pickle.** Pickle is a Python-specific RCE surface; JSON-RPC is language-agnostic, schema-checked, debuggable.
- **Per-host preflight.** Ansible has no equivalent. We catch sudoers, become, key, and file-transfer failures *before* tasks ship — one of the explicit P1-persona wedges.
- **Typed `BecomeSpec`.** Replaces Ansible's flat `become_method`/`become_user`/`become_password`/`become_flags` soup (`11-poor-decisions.md` §16). Per-method structured fields (e.g. `Runas { logon_type, logon_flags }`) live in the type system, not in untyped strings.
- **Secret keyring as default password source.** `Keyring` is the default for both connection and become passwords. `Plaintext` is supported but logged loudly. `Pipe` for one-shot CI delivery on a fresh fd. No env-var smuggling, no password in argv, no leak via `/proc/<pid>/cmdline`.
- **Bounded `slurp`.** Refuses files above a configurable cap so a misconfigured slurp cannot OOM the controller.
- **Stale control-socket cleanup on startup.** Ansible leaves stale sockets behind regularly; we clean them by checking master PIDs.
- **Reconnection classification.** Errors are classified `Transient | Auth | Fatal`. Only `Transient` retries (capped exponential backoff). Ansible's `reconnection_retries` retries indiscriminately, which is occasionally how an admin gets locked out for bad-password attempts.

## 11. Milestones

- **M0.** `SshSystem` (with ControlMaster) + `Local` + `Sudo` become. Library only — the engine consumes the trait directly. Pipelining on. No internal binary. No preflight beyond a `connect + true`. Enough to drive `runsible-playbook` against a real fleet for the smoke-test.
- **M1.** `Russh` fallback. Full per-host preflight (steps 1-4). JSON-RPC internal binary. `BecomeMethod::{Su, Doas, Pbrun, Pfexec, Dzdo, Ksu, Machinectl, Sesu, Custom}` — every become method in the v1 menu except `Runas`. Stale-socket cleanup. Host-key verification with `strict|accept-new|off`. Bounded `slurp`. Capability matrix for transports.
- **M2.** `SubprocessWrapper` transports (`kubectl`, `docker`, `podman`, `lxc-attach`). `SecretSource::Keyring` backends for libsecret + macOS Keychain (Credential Manager deferred to v1.5 with the rest of the Windows persona). Full reconnection classifier with backoff. Preflight parallelization within forks budget. The "auto" connection mode (probes `SshSystem`, falls back to `Russh`).

These milestones align with the "land `runsible-playbook` with a useful 12-module kit" target in `00-user-story-analysis.md` §9 — `runsible-connection` M1 ships before that gate.

## 12. Dependencies on other crates

Foundational. Imports nothing from the other twelve workspace crates — they all depend on `runsible-connection`, not the other way around. First-party utility imports only: `runsible-error` (error taxonomy), `runsible-secret` (`SecretSource` keyring abstraction).

Third-party: `tokio` (runtime); `openssh` (system-OpenSSH + ControlMaster); `russh` + `russh-keys` + `russh-sftp` (pure-Rust SSH); `async-trait` (object-safety); `serde` + `serde_json` (`ExecOutcome`, RPC, capabilities); `jsonrpsee` or `jsonrpc-core` (internal-binary RPC); `keyring` (cross-platform secret store); `nix` (`prctl(PR_SET_PDEATHSIG)`, fd handling); `shell-escape` (`sh -c` joining); `regex` (su prompt); `uuid` (preflight tmp files); `tracing` (NDJSON-friendly logs).

Consumers: `runsible-playbook` (engine drives execs via the trait), `runsible` (ad-hoc), `runsible-pull` (pull daemon, mostly `Local`), `runsible-console` (REPL).

## 13. Tests

- **Unit.** Argv construction for each become method — frozen-output test per `BecomeMethod` variant asserting exact `argv`, `stdin`, `env`, `tty`.
- **Unit.** Capability matrix per transport (e.g. `SubprocessWrapper(Kubectl).capabilities().supports_env == false`).
- **Unit.** Stale-socket cleanup (mock PID alive-check); reconnection classifier on synthetic errors; argv-joining against quotes/dollar/backtick/UTF-8 payloads; bounded `slurp` cap; host-key parser with hashed/`@cert-authority`/`@revoked` entries.
- **Integration.** Spin up an `sshd` Docker container; run a playbook against it via `SshSystem`, then `Russh`, then `Local`. All three must produce identical exec outcomes.
- **Integration.** `kind` cluster + `SubprocessWrapper(Kubectl)`; Docker container + `SubprocessWrapper(Docker)`.
- **Integration.** Per-host preflight: a host with broken sudoers fails preflight and aborts cleanly without running tasks.
- **Stress.** 100 concurrent connections via OpenSSH ControlMaster — assert all complete, single control socket, no MaxSessions errors at default 5-channel ceiling.
- **Stress.** 100 concurrent connections via `Russh` pool — assert channel reuse and keepalive-warmed sessions.
- **Negative.** Host unreachable (TCP RST, NXDOMAIN, ICMP unreachable); auth fail (bad key, bad password); become fail; command timeout (SIGTERM then SIGKILL); slurp over cap. Each asserts a structured error and clean state — no leaked processes, fds, or sockets.
- **Snapshot.** JSON-RPC wire format — golden frames per method, version-stable.

## 14. Risks

- **Stale ControlMaster sockets.** Own the control-path directory (`~/.runsible/cm/`); clean on startup; never share with `~/.ansible/cp/` to avoid collisions with a co-installed Ansible. Path scheme `%r@%h:%p` (not hashed) since the directory is ours and the 108-byte `sun_path` limit allows reasonable hostnames at the chosen prefix.
- **TTY interacting with pipelining/streaming output.** A PTY merges stdout and stderr at kernel level; our `ExecOutcome` separates them. When `tty: true`, we set `stderr` empty and document the merge in `stdout`. Pipelining is auto-disabled when `tty: true`.
- **Russh feature parity with OpenSSH.** No GSSAPI, no `ProxyCommand`, no smart-card `ssh-agent`, no full `Match` syntax in v1 `Russh`. Limit promised features in the v1 docs; the "auto" mode (§15) detects `ProxyCommand` and routes to `SshSystem`. Test feature parity case-by-case with explicit gates rather than promising "russh works the same."
- **Windows requires WinRM** which is out of v1. Emit a clear "WinRM is v1.5+" error rather than partially implementing.
- **Keyring availability.** Headless servers may have no running keyring. `SecretSource::Keyring` fails with a clear "use --connection-password-file or set up libsecret" error — never a silent plaintext fallback.
- **Pipelining + sudo + requiretty.** Sudoers with `requiretty` rejects even pipelined runs. Detect the "sudo: a terminal is required" stderr token, transparently reconnect with `tty: true`, cache the observation in `runsible-state` so we never pay the discovery cost twice.
- **Preflight cost on a 1000-host run.** Four sequential commands per host adds ~200-500 ms for TCP+auth transports. Parallelize across `forks`; cache preflight outcomes within a single invocation; provide `--skip-preflight` for the 1% case.
- **Single-maintainer bus factor** (per `00-user-story-analysis.md` §8 R7). This plan + in-tree `docs/architecture/` + the trait surface keep the design re-implementable from spec.

## 15. Open questions

- **`connection = "auto"`.** Pick `SshSystem` if (a) `ssh` is on PATH, (b) ControlMaster is healthy, (c) no `ProxyCommand` is configured — fall back to `Russh` otherwise. Recommendation: yes for v1.0 — seamless demo experience. Adds a ~50 ms capability probe per host (`ssh -T -o BatchMode=yes -o ConnectTimeout=2 <host>`), cached after first run.
- **Pipelining default.** Ansible defaults off because of legacy `requiretty`. We default *on* — we ship in 2026, modern sudoers rarely sets `requiretty`, and the perf win is large. Open: a fleet-wide compat flag for old RHEL 6/7 estates (`[connection.ssh] pipelining = false`)? Probably yes.
- **File transfer chunking.** Auto-pick by size: single-shot under 16 MiB, chunked above (parallelizable across `Russh` pool channels). Rate-limiting deferred to v1.1 unless asked.
- **Sudo version probing.** Some flags (`--preserve-env=...`) require sudo ≥ 1.8.5. We could probe `sudo --version` once per host preflight; the win is precise error messages, the cost is one extra roundtrip. Open.
- **JSON-RPC framing.** NDJSON vs length-prefixed. NDJSON is friendlier to human inspection; runsible's whole event story is NDJSON anyway. Probably NDJSON.
- **Worker lifetime across invocations.** Ansible's `ansible-connection` daemon dies with the playbook. Keeping workers alive across invocations is much faster for repeated runs but complicates lifecycle (parent crashes, socket leaks, cross-user boundaries). Defer to v1.1 as `runsible-connection daemon --persistent` opt-in.
- **Stabilizing the `Connection` trait.** Not in v1.0. Mark `#[doc(hidden)]` with a "not stable" notice; revisit in v1.2 after absorbing feedback from in-tree variants.
