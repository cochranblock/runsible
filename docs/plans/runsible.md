# runsible — `runsible` (ad-hoc binary)

## 1. Mission

`runsible` is the imperative one-shot of the toolkit: pick a host pattern, pick a module, pass arguments, execute in parallel across the matched inventory, exit with a structured result. It is the binary an SRE reaches for in place of `ssh host 'cmd'` when "host" is fifty hosts and "cmd" is "run module X with arguments Y." It exists separately from `runsible-playbook` because the user stories are different (J7: "I'm running ad-hoc; pay zero start-up cost; use the tool the way I use ssh"). Playbook is for declared, reviewable, repeatable change; `runsible` is for diagnosis, fan-out probes, fleet-wide one-shots, and the demo every blog post leads with — `runsible all -m runsible_builtin.ping`. P4 (homelab) and P1 (platform engineer running pre-deploy probes) are the dominant audiences. If `runsible` does not start in under 100 ms and emit clean NDJSON, the project's reputation is dead before `runsible-playbook` ships.

## 2. Scope

**In:**
- Parse CLI: positional host pattern, `-m`/`-a`, plus the cross-cutting flag set (inventory, connection, become, vault, output).
- Resolve the host pattern via `runsible-inventory` (TOML native, YAML/INI through yaml2toml read path).
- For each matched host, in parallel up to `--forks`: open a connection through `runsible-connection`, invoke the named module with typed arguments, collect the typed `Outcome`, write one NDJSON event per host. Exit non-zero if any host failed.
- Pretty-printed text when stdout is a TTY; NDJSON otherwise (per §10).
- `--check` (plan only), `--diff` (render diff into events), `--list-hosts`, `--explain-vars HOST` (per §3).
- Typed become via `BecomeSpec` (per §16).
- Vault: lazy decryption of inventory-side encrypted files via age recipients (per §6); `--vault-id`/`--vault-password-file` honored for legacy ansible-vault files.
- FQCN-only module references (per §22).
- Async as `--background` (fire-and-forget) and `--task-timeout` (cap on synchronous wait), per §24.
- Granular exit codes that distinguish parse / unreachable / module-failed / become-preflight / vault.

**Out:**
- Playbooks, plays, tasks, blocks, handlers, roles, packages — all in `runsible-playbook`.
- Tags / `--start-at-task` / `--list-tasks` — playbook-only.
- Plugin-loadable modules. v1 links the static `runsible-builtin` set; dynamic-library modules deferred past v1.5.
- `--tree DIR`. NDJSON to stdout subsumes it.
- `--one-line`. There is no third format.
- `--playbook-dir`. Ad-hoc has no playbook-relative resolution layer (per §6 of inventory critique).
- `-B`/`-P` Ansible naming for async/poll. Replaced by `--background`/`--task-timeout`.
- Plugin compatibility with Python collections.

## 3. CLI surface

Inheriting `ansible`'s shape so muscle memory carries, but stripped of the ambiguities flagged in §3, §10, §16, §22, §24.

### Target selection

| Flag                            | Type      | Default        | Env                  | Purpose                                                            |
|---------------------------------|-----------|----------------|----------------------|--------------------------------------------------------------------|
| `<pattern>` (positional)        | string    | required       | —                    | Pattern grammar from `03-inventory.md` §3 (union/intersect/exclude).|
| `-i, --inventory PATH[,...]`    | path list | `./inventory/` | `RUNSIBLE_INVENTORY` | Repeatable. Last-writer-wins on merge.                             |
| `-l, --limit SUBSET`            | pattern   | `all`          | —                    | Further restricts the pattern.                                     |
| `--list-hosts`                  | bool      | false          | —                    | Print matched hosts; exit.                                         |
| `--flush-cache`                 | bool      | false          | —                    | Drop fact cache for targeted hosts.                                |
| `--explain-vars HOST`           | string    | unset          | —                    | Print var-resolution stack for HOST; exit. (§3.)                   |
| `--strict-pattern`              | bool      | false          | `RUNSIBLE_STRICT_PATTERN` | Unmatched pattern is an error, not a warning.                 |

### Module invocation

| Flag                       | Type       | Default                    | Env  | Purpose                                                          |
|----------------------------|------------|----------------------------|------|------------------------------------------------------------------|
| `-m, --module-name MODULE` | FQCN       | `runsible_builtin.command` | —    | **Fully qualified**. (§22.)                                      |
| `-a, --args 'k=v ...'`     | str/@file  | empty                      | —    | k=v, JSON, or `@path.toml`.                                      |
| `-e, --extra-vars KV`      | str/@file  | empty                      | —    | Repeatable. Becomes the runtime layer (level 3 of 5). (§3.)      |
| `--vars-file PATH`         | path       | unset                      | —    | TOML file merged into runtime layer.                             |

### Connection

| Flag                              | Type   | Default | Env                          | Purpose                                                          |
|-----------------------------------|--------|---------|------------------------------|------------------------------------------------------------------|
| `-c, --connection PLUGIN`         | string | `ssh`   | `RUNSIBLE_CONNECTION`        | `ssh` (system openssh), `russh`, `local`, `docker`, `kubectl`.   |
| `-u, --user USER`                 | string | invent. | `RUNSIBLE_REMOTE_USER`       | Remote user.                                                     |
| `-T, --timeout SECONDS`           | uint   | `10`    | `RUNSIBLE_TIMEOUT`           | TCP connect timeout (one of four explicit timeouts).             |
| `--task-timeout SECONDS`          | uint   | unset   | `RUNSIBLE_TASK_TIMEOUT`      | Wall-clock cap per host on the module call.                      |
| `--private-key PATH`              | path   | unset   | `RUNSIBLE_PRIVATE_KEY_FILE`  | SSH key file.                                                    |
| `-k, --ask-pass`                  | bool   | false   | —                            | TTY prompt. Mutually exclusive with `--connection-password-file`.|
| `--connection-password-file PATH` | path   | unset   | —                            | Password from file.                                              |
| `--ssh-common-args STR`           | string | unset   | `RUNSIBLE_SSH_COMMON_ARGS`   | Forwarded to ssh/scp/sftp.                                       |
| `--ssh-extra-args STR`            | string | unset   | `RUNSIBLE_SSH_EXTRA_ARGS`    | ssh only.                                                        |
| `-f, --forks N`                   | uint   | `5`     | `RUNSIBLE_FORKS`             | Tokio task budget.                                               |
| `--reconnection-retries N`        | uint   | `0`     | —                            | Transient-only retry; auth failures never retry.                 |

### Become (typed; §16)

| Flag                          | Type   | Default | Env                       | Purpose                                                  |
|-------------------------------|--------|---------|---------------------------|----------------------------------------------------------|
| `-b, --become`                | bool   | false   | `RUNSIBLE_BECOME`         | Enable privilege escalation.                             |
| `--become-method METHOD`      | enum   | `sudo`  | `RUNSIBLE_BECOME_METHOD`  | `sudo`/`su`/`doas`/`pbrun`/`pfexec`/`runas`/`enable`.    |
| `--become-user USER`          | string | `root`  | `RUNSIBLE_BECOME_USER`    | Target user.                                             |
| `-K, --ask-become-pass`       | bool   | false   | —                         | TTY prompt; in-memory only.                              |
| `--become-password-file PATH` | path   | unset   | —                         | Mutually exclusive with `-K` and `--become-from-keyring`.|
| `--become-from-keyring KEY`   | string | unset   | `RUNSIBLE_BECOME_KEYRING` | libsecret/Keychain/Credential Manager. (§16.)            |

### Vault

| Flag                         | Type        | Default | Env                            | Purpose                                                     |
|------------------------------|-------------|---------|--------------------------------|-------------------------------------------------------------|
| `--vault-id LABEL@SOURCE`    | repeat str  | unset   | `RUNSIBLE_VAULT_IDENTITY_LIST` | Compatibility with Ansible-vault files.                     |
| `-J, --ask-vault-password`   | bool        | false   | —                              | Mutually exclusive with `--vault-password-file`.            |
| `--vault-password-file PATH` | path        | unset   | `RUNSIBLE_VAULT_PASSWORD_FILE` | Compat path.                                                |
| `--vault-recipient PATH`     | repeat path | unset   | `RUNSIBLE_VAULT_RECIPIENTS`    | runsible-native age/SSH recipient. (§6.)                    |

### Modes & output

| Flag           | Type | Default | Env               | Purpose                                                          |
|----------------|------|---------|-------------------|------------------------------------------------------------------|
| `-C, --check`  | bool | false   | —                 | `module.plan()` only; never `apply()`. (§9.)                     |
| `-D, --diff`   | bool | false   | —                 | Render diff into NDJSON events.                                  |
| `--background` | bool | false   | —                 | Fire-and-forget. Prints `{"job_id": "..."}`. (§24.)              |
| `--output FMT` | enum | auto    | `RUNSIBLE_OUTPUT` | `ndjson`/`pretty`/`auto` (TTY → pretty, else ndjson). (§10.)     |
| `--no-color`   | bool | false   | `NO_COLOR`        | Disable ANSI in pretty mode.                                     |
| `--quiet`      | bool | false   | —                 | Per-host events only in pretty mode.                             |
| `-v, --verbose`| repeat | 0     | `RUNSIBLE_VERBOSITY` | `-v`..`-vvvv`. Adds connection-debug events to NDJSON.        |
| `-h, --help`   | bool | false   | —                 |                                                                  |
| `--version`    | bool | false   | —                 | Build version, git sha, linked module set.                       |

### Removed vs Ansible

`-t TREE`, `-o`, `-B`/`-P`, `--playbook-dir`, `-M`. Reasons in §2.

### Mutual-exclusion pairs

`-k` ⊕ `--connection-password-file`; `-K` ⊕ `--become-password-file` ⊕ `--become-from-keyring`; `-J` ⊕ `--vault-password-file`; `--background` ⊕ `--check`.

### Exit codes

| Code | Meaning                                                             |
|------|---------------------------------------------------------------------|
| 0    | All hosts ok (or zero hosts matched and `--strict-pattern` unset).  |
| 2    | At least one host's module returned `failed: true`.                 |
| 3    | At least one host unreachable (TCP/auth/banner timeout).            |
| 4    | Inventory or argument parse error.                                  |
| 5    | CLI option error (mutual exclusion).                                |
| 6    | Become preflight failed (sudoers misconfigured, etc.).              |
| 7    | Vault decryption failed.                                            |
| 8    | Pattern matched zero hosts and `--strict-pattern` set.              |
| 99   | SIGINT.                                                             |
| 250  | Internal panic.                                                     |

## 4. Data model (Rust types)

```rust
/// 1:1 with the parsed CLI; built by clap-derive merging argv with Config.
pub struct AdHocInvocation {
    pub pattern: HostPattern,
    pub module_ref: ModuleRef,
    pub module_args: ModuleArgs,
    pub inventory_sources: Vec<PathBuf>,
    pub limit: Option<HostPattern>,
    pub connection: ConnectionSpec,
    pub become_spec: Option<BecomeSpec>,
    pub vault: VaultSpec,
    pub forks: u32,
    pub mode: RunMode,
    pub output: OutputSpec,
    pub extra_vars: VarLayer,            // runtime layer (level 3)
    pub task_timeout: Option<Duration>,
    pub reconnection_retries: u32,
    pub strict_pattern: bool,
    pub explain_vars_host: Option<String>,
    pub list_hosts_only: bool,
}

pub enum RunMode { Apply, Check, Background }

pub enum OutputSpec {
    Pretty { color: bool, quiet: bool },
    Ndjson,
    Auto,
}

/// FQCN. No shortening, no aliasing at the CLI. (§22.)
pub struct ModuleRef { pub package: String, pub name: String }

pub enum ModuleArgs {
    KeyValue(BTreeMap<String, RawValue>),
    Json(serde_json::Value),
    Toml(toml::Value),
    Empty,
}

/// One resolved target with all inventory state attached.
pub struct HostTarget {
    pub inventory_hostname: String,
    pub address: ConnectionAddress,
    pub vars: ResolvedVars,              // 5-layer merged
    pub connection: ConnectionSpec,      // host-resolved
    pub become_spec: Option<BecomeSpec>,
    pub group_names: Vec<String>,
}

/// Re-exported from runsible-connection.
pub use runsible_connection::{ConnectionSpec, ConnectionAddress, BecomeSpec};
pub use runsible_inventory::HostPattern;

/// 1 NDJSON line per HostEvent (`runsible.event.v1`).
#[derive(Serialize)] #[serde(tag = "type")]
pub enum HostEvent {
    HostStarted { host: String, ts: DateTime<Utc> },
    HostPlanned { host: String, plan: serde_json::Value },           // -C only
    HostChanged { host: String, outcome: serde_json::Value, diff: Option<DiffArtifact> },
    HostOk      { host: String, outcome: serde_json::Value },
    HostFailed  { host: String, error: ErrorPayload },
    HostUnreachable { host: String, error: ErrorPayload },
    HostBackgrounded { host: String, job_id: String },
    Done { totals: Totals },
}

/// The shared trait. Lives in runsible-module (consumed here).
pub trait Module {
    type Input: serde::de::DeserializeOwned + Send + Sync;
    type Plan: serde::Serialize + Diff + Send;
    type Outcome: serde::Serialize + Send;
    fn plan(&self, input: &Self::Input, host: &HostState) -> Result<Self::Plan>;
    fn apply(&self, plan: &Self::Plan, host: &mut HostState) -> Result<Self::Outcome>;
    fn verify(&self, plan: &Self::Plan, host: &HostState) -> Result<()>;
}

/// 5-tier collapse from §3. Ad-hoc only populates 0/1/3.
#[derive(PartialOrd, Ord)]
pub enum VarLayer {
    ProjectDefaults = 0,
    Inventory       = 1,
    Playbook        = 2,   // inert in ad-hoc
    Runtime         = 3,
    SetFacts        = 4,   // inert in ad-hoc
}
```

`runsible-config`'s `Config` resolves env-var defaults and config-file precedence into a struct used to fill `AdHocInvocation` defaults before clap overrides.

## 5. Redesigns vs Ansible

Each item is a concrete behavior `runsible` ships, citing `11-poor-decisions.md`.

- **§1 (YAML→TOML):** Inventories are TOML by default. Legacy YAML/INI is read through yaml2toml; lossy parses warn the user to commit the converted TOML.
- **§3 (22→5 levels):** Ad-hoc populates layers 0/1/3 only. `--explain-vars HOST` prints which layer won for every var, including which inventory file or `host_vars/` entry contributed.
- **§6 (vault):** Default is age recipients via `--vault-recipient`. `--vault-id`/`--vault-password-file` are honored for legacy files; first encounter emits a one-time nudge to `runsible-vault migrate-from-ansible`.
- **§8 (connection sprawl):** `-c ssh` shells out to system OpenSSH with `ControlMaster=auto`, `ControlPersist=60s`. `-c russh` is the pure-Rust fallback. `-c local`/`docker`/`kubectl` are subprocess wrappers, not plugins. `-c paramiko` aliases to `russh` with a deprecation event in `-v`.
- **§9 (idempotency):** Default mode runs `plan()` first and skips `apply()` if the plan is empty (reported `ok`, not `changed`). After `apply()`, `verify()` re-derives the plan and confirms it's empty. `command`/`shell` are flagged `verify_idempotent = false` and the runner emits a `verify_skipped` field. `--check` runs `plan()` only.
- **§10 (output):** NDJSON to stdout when not a TTY; pretty otherwise. Schema `runsible.event.v1`. `runsible … | jq -c 'select(.type == "host_failed")'` is the canonical CI failure pattern.
- **§16 (typed become):** CLI maps onto a `BecomeSpec` struct. Method-specific knobs come from inventory TOML or `--become-flags='...'`. A preflight `whoami` runs once per host before the actual module; failures exit 6 with a precise diagnostic ("requiretty in sudoers on host42") instead of Ansible's "unable to find sudo password" mush. Preflight result cached per `(host, method, user)` with a 1-hour TTL.
- **§22 (no FQCN shortening):** `-m` must be `<package>.<module>`. `-m ping` is a parse error suggesting `runsible_builtin.ping`. The CLI is intentionally verbose because it's a one-shot, not a routine surface; lexical `[imports]` aliasing is a playbook-only thing.
- **§24 (async vs background):** `--background` is fire-and-forget; per-host `job_id` is emitted. `--task-timeout SECONDS` caps synchronous wait. The two are mutually exclusive. `runsible-job status <id>` and `runsible-job wait <id>` live in a sister binary; `runsible` itself never polls.

§7, §11, §13, §17, §18, §19, §21, §25 are playbook- or galaxy-level and don't touch the ad-hoc binary.

## 6. Milestones

### M0 — `runsible all -m runsible_builtin.ping` works

Smallest demo-able surface.

- clap-derive parses argv + named env vars into `AdHocInvocation`.
- TOML inventory only. `all` and bare host-list patterns.
- Modules: `runsible_builtin.ping`, `runsible_builtin.command`. Statically linked.
- Connection: `-c ssh` (via the `openssh` crate) and `-c local`. No russh, docker, kubectl.
- Become: `--become` + `sudo`, no preflight, no keyring.
- Output: NDJSON only.
- Forks: Tokio `JoinSet` + semaphore on `--forks`.
- Exit codes: 0/2/3/4/5.

Demo: `runsible all -i inv.toml -m runsible_builtin.ping` against 50 SSH hosts. Cold start ≤100 ms. NDJSON event per host.

### M1 — production-grade ad-hoc

Everything a P1/P4 user wires into CI.

- Full pattern grammar (union, intersection, exclusion, regex, range).
- Multi-source inventory merge; yaml2toml read path.
- Modules: `ping`, `command`, `shell`, `copy`, `template`, `file`, `package`, `service`, `setup`, `debug`, `assert` (eleven).
- All connection plugins; all become methods with preflight + keyring.
- Pretty TTY mode, diff rendering, full plan/apply/verify cycle.
- Vault: age decryption transparent; legacy ansible-vault read.
- Async/background, `--explain-vars`, strict-pattern, reconnection-retries.
- All exit codes.

Demo: `runsible 'webservers:&prod:!host42' -m runsible_builtin.copy -a '...' --become --check --diff` produces a human diff across 200 hosts in under 5 s.

### M2 — feature-complete vs `ansible`

- Full builtin module set (~70).
- `--vault-id` syntactic compatibility with multi-identity ansible-vault.
- Fact gathering integrated with `runsible-fact-store` cache.
- TTY interaction (`--ask-vault-pass` etc.).
- Optional structured audit log to syslog or file URL.
- `RUNSIBLE_OUTPUT=ndjson` honored even on TTY.

Beyond M2: `-M`/dynamic-library plugin loading, Windows, network-device modules — all post-v1.5.

## 7. Dependencies on other crates

```
                          ┌──────────────────┐
                          │     runsible     │  (this crate)
                          └────────┬─────────┘
              ┌──────────────────┬─┴────────────────────┐
              ▼                  ▼                      ▼
    ┌────────────────┐  ┌────────────────┐    ┌──────────────────┐
    │ runsible-config│  │runsible-invent │    │runsible-connection│
    └────────────────┘  └───────┬────────┘    └──────────┬───────┘
                                ▼                          ▼
                         ┌────────────┐          ┌────────────────┐
                         │ yaml2toml  │          │  runsible-vault│
                         └────────────┘          └────────────────┘
                                                         ▼
                                                ┌────────────────┐
                                                │runsible-builtin│
                                                │ + module trait │
                                                └────────────────┘
```

| Sister crate          | Form     | Required at | Notes                                                  |
|-----------------------|----------|-------------|--------------------------------------------------------|
| `runsible-config`     | library  | M0          | Env + config-file precedence → `Config`.               |
| `runsible-inventory`  | library  | M0          | Pattern resolution, host/group vars, multi-source merge.|
| `runsible-connection` | library  | M0          | `Connection` trait + `ssh`/`local` at M0; rest at M1.  |
| `runsible-module`     | library  | M0          | The shared `Module` trait.                             |
| `runsible-builtin`    | library  | M0          | Static module set.                                     |
| `runsible-vault`      | library  | M1          | Decryption only (encryption lives in the vault binary).|
| `yaml2toml`           | library  | M1          | Consumed by `runsible-inventory` for legacy reads.     |
| `runsible-playbook`   | none     | —           | Sibling.                                               |
| `runsible-galaxy`/`-doc`/`-lint`/`-test`/`-pull`/`-console` | none | — | Not used by ad-hoc. (`runsible-console` spawns/links `runsible`, not the other way round.) |

## 8. Tests

### Unit (`crates/runsible/`)

- `cli::parse`: every flag, every env override, every mutual-exclusion pair. Each exit-code path has a deliberately bad invocation asserting the code and stderr message.
- `pattern::resolve`: round-trip the grammar from `03-inventory.md` §3; confirm processing-order claim (union → intersect → exclude regardless of write order).
- `dispatch::plan_apply_verify`: with a `MockModule`:
  - `--check` calls only `plan`.
  - Empty plan in default mode skips `apply`, emits `HostOk` with `changed: false`.
  - Non-empty plan calls `apply` then `verify`; result includes verify status.
  - `verify_idempotent = false` modules emit `verify_skipped`.
- `output::ndjson`: every `HostEvent` variant serializes to a stable `runsible.event.v1` shape (insta snapshot).
- `output::pretty`: TTY detection, color toggle, quiet mode, verbosity.

### Integration (`crates/runsible/tests/`)

Fleet fixtures via docker-compose:
- `fleet-3host`: three Alpine sshd containers (happy path).
- `fleet-mixed`: Debian + Alpine + a host with `requiretty` (preflight tests).
- `fleet-broken`: sshd refusing the test user (exit-code-3 tests).

Tests:
- `it_ping`: `runsible all -m ...ping` exits 0; emits 3 `host_ok` + a `done` with `totals.ok=3`.
- `it_become`: `whoami --become` returns `root` for each host; the `requiretty` host is detected, retried with `-tt`, succeeds.
- `it_check`: `--check copy` writes nothing on the targets (verified by follow-up stat).
- `it_explain_vars`: deterministic JSON of layer winners against a multi-source inventory.
- `it_cold_start`: `time runsible localhost -m ...ping -c local` < 100 ms p50 on the reference machine. **CI gate: regression past 200 ms fails the build.**
- `it_yaml_inventory`: `inventory.yaml` resolves equivalently to its `inventory.toml` peer.

### Property (`proptest`)

- Pattern-grammar order-insensitivity.
- `HostEvent` round-trip through serde.

### Benchmarks (`criterion`)

- Cold start (process exec → first NDJSON event): < 100 ms p50, < 200 ms p99.
- 100-host parallel `ping` against an in-memory mock connection: < 500 ms at `--forks 100`.
- 5,000-host TOML inventory parse + resolve `all`: < 250 ms.

## 9. Risks

- **Cold-start regression is reputation-fatal.** The blog-post demo is `runsible all -m ping`. If v1's cold start meaningfully lags pyinfra/fabric, the project is dead at adoption. **Mitigation:** CI gate every commit; clap-derive (smaller binary than builder); panic = abort; LTO on release; document `RUSTFLAGS="-C target-cpu=native"` for user-built releases.
- **FQCN-only will surprise users.** First thing a P4 types is `runsible all -m ping`, hits a parse error, tweets about it. **Mitigation:** the error message must suggest the FQCN; consider relaxing for `runsible_builtin` names via a documented allowlist after measuring real friction.
- **Become preflight overhead.** A `whoami` per host is a 1–2 RTT cost; on 1000-host fleets that's ~10 s wasted. **Mitigation:** preflight cached in `~/.cache/runsible/become-cache.json` keyed by `(host, method, user)`, TTL 1 hour, configurable.
- **NDJSON schema versioning.** Evolving `runsible.event.v1` would break every CI consumer. **Mitigation:** version is in every event's type namespace; breaking changes go to `v2`, with v1 emitted in parallel for one major version under `--ndjson-compat v1`.
- **Two vault formats.** age recipients vs legacy ansible-vault. **Mitigation:** clear errors and a single `--vault-recipient` migration path forward.
- **No long-lived connection daemon in v1.** Ten `runsible …` invocations from a shell pay ten cold-start tax. The right answer is a sister daemon; deferred. Document the workaround (`runsible-console`).

## 10. Open questions

These need a decision before M0 starts.

1. **Default inventory path.** Proposal: `./inventory/` → `./inventory.toml` → `RUNSIBLE_INVENTORY` env var. Never look in `/etc/`. Confirm.
2. **Module-name leniency.** Strict FQCN (redesign §22) vs. allowlist of `runsible_builtin` short names. Pick one for v1.
3. **`-c paramiko` handling.** Silent alias to `russh` with a `-v` warning, or hard error?
4. **Become preflight on by default?** Proposal: yes; `--no-become-preflight` to disable.
5. **stdin behavior.** Currently undefined. Proposal: ignored, with a `-vv` notice if data is seen on stdin.
6. **NDJSON timestamps.** Proposal: RFC3339 with microsecond precision (matches log aggregators; nanoseconds is overkill).
7. **Connection-pool TTL across processes.** No daemon in v1; explicit confirmation that this is the right tradeoff.
8. **`--vars-file` schema.** Proposal: flat top-level TOML, read into the runtime layer wholesale.
9. **`--list-hosts` flag vs. routing through `runsible-inventory --list`.** Proposal: keep flag for convenience; document `runsible-inventory --list` as the canonical surface.
10. **Telemetry / version-check.** Proposal: none. Confirm.
