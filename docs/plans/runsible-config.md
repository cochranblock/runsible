# runsible — `runsible-config`

## 1. Mission

`runsible-config` is the foundation crate every other runsible binary depends on. It reads, validates, dumps, and explains runsible's configuration. The schema is typed Rust, the on-disk format is TOML, and — critically — environment-variable support is **opt-in per key** (per `11-poor-decisions.md` §25), not the implicit hundreds-of-overrides surface that `ANSIBLE_*` evolved into. Every config key has exactly one source-of-truth declaration: name, type, default, env var (if any), description. If `runsible-config` doesn't know about a key, it doesn't exist.

---

## 2. Scope

**In:** read `runsible.toml` from a documented search path; validate against the typed schema (reject unknown keys, mismatched types, deprecated values, out-of-range numerics, all with line/column diagnostics); subcommands `list`, `show`, `dump`, `init`, `validate`, `explain`, `diff`, `import-ansible`; emit defaults; show resolved value with source for any key (`runsible-config explain <key>`); pin a `schema_version`.

**Out:** runtime mutation of config (the `Config` is immutable after load); managing collections / packages (that's `runsible-galaxy`) or vault recipients (that's `runsible-vault`); editing inventory/playbook files; Cargo-style multi-file merge (single file wins; revisit only if users demand it — §14).

---

## 3. Search precedence

Runsible looks for one config file. **First match wins.** Lower-precedence candidates are reported at debug level so users can see what was skipped.

| Order | Source | Notes |
|------:|--------|-------|
| 1 | `RUNSIBLE_CONFIG` env var | Absolute path. Missing/unreadable = **hard error**, no silent fallback. |
| 2 | `./runsible.toml` | CWD-local. Standard repo layout. |
| 3 | `$XDG_CONFIG_HOME/runsible/config.toml` (defaults to `~/.config/runsible/config.toml`) | Per-user. Honors XDG explicitly. |
| 4 | `/etc/runsible/runsible.toml` | System-wide. |
| 5 | Compiled-in defaults | Always present. |

**Inherited from Ansible:** the world-writable CWD check (we warn loudly and fall through to source 3 instead of silently skipping); path-typed keys in the loaded config resolve relative to the config file's directory, not CWD.

**Not inherited:** no `~/.runsible.toml` dotfile (XDG only); no `{{CWD}}` macro; no `RUNSIBLE_HOME` mega-env that relocates a dozen subdirs (each path key is independent).

---

## 4. The runsible config schema

Ansible has ~250 keys; runsible aims for ~60. Anything tied to Python interpreter discovery, Paramiko, persistent connections, callback-plugin sprawl, cowsay, or selinux-libvirt-LXC is dropped. Anything that's a transport tunable lives in `[ssh]`, not five overlapping plugin sections.

### 4.1 Top-level

| Key | Type | Default | Env Var | Purpose |
|---|---|---|---|---|
| `schema_version` | str | `"1"` | (none) | Pin the schema version. Required at file top. |

### 4.2 `[defaults]` — main behavior toggles

Curated subset. Dropped: every `*_plugins` path, Python interpreter knobs, cowsay, `ansible_managed`, `null_representation`, `error_on_undefined_vars`, `module_compression`, `module_strict_utf8_response`, `target_log_info`, `worker_shutdown_*`, `win_async_*`, `display_traceback`, `python_module_rlimit_nofile`, `system_warnings`, `devel_warning`, `localhost_warning`, `internal_poll_interval`.

| Key | Type | Default | Env Var | Purpose |
|---|---|---|---|---|
| `verbosity` | int 0–7 | `0` | `RUNSIBLE_VERBOSITY` | Mirrors `-v` count. |
| `forks` | int | `5` | `RUNSIBLE_FORKS` | Max parallel host workers per play. |
| `inventory` | list[path] | `["./inventory.toml"]` | `RUNSIBLE_INVENTORY` | Default inventory source(s); comma-sep in env. |
| `remote_user` | str | (current user) | `RUNSIBLE_REMOTE_USER` | Default remote login user. |
| `remote_port` | int? | `null` | `RUNSIBLE_REMOTE_PORT` | Null defers to ssh. |
| `private_key_file` | path? | `null` | `RUNSIBLE_PRIVATE_KEY_FILE` | Default SSH private key. |
| `host_key_checking` | bool | `true` | `RUNSIBLE_HOST_KEY_CHECKING` | Validate SSH known_hosts. |
| `connection_timeout` | int s | `10` | `RUNSIBLE_TIMEOUT` | Connection timeout. |
| `task_timeout` | int s | `0` | `RUNSIBLE_TASK_TIMEOUT` | Wall-clock per-task timeout (`0` = unlimited). |
| `gathering` | enum (`implicit`/`explicit`/`lazy`) | `lazy` | `RUNSIBLE_GATHERING` | Fact-gather policy; `lazy` per §12. |
| `force_handlers` | bool | `false` | (none) | Run notified handlers after task failures. |
| `any_errors_fatal` | bool | `false` | (none) | Any failure fatal play-wide. |
| `keep_remote_files` | bool | `false` | `RUNSIBLE_KEEP_REMOTE_FILES` | Don't delete remote temp files. |
| `local_tmp` | path | `~/.runsible/tmp` | `RUNSIBLE_LOCAL_TEMP` | Controller scratch dir. |
| `log_path` | path? | `null` | `RUNSIBLE_LOG_PATH` | Optional file log. |
| `log_filter` | list[str] | `[]` | (none) | Logger names dropped from log file. |
| `editor` | str | `$EDITOR` or `vi` | `RUNSIBLE_EDITOR` | Editor for vault edit. |
| `pager` | str | `$PAGER` or `less` | `RUNSIBLE_PAGER` | Pager for doc. |
| `playbook_dir` | path? | `null` | `RUNSIBLE_PLAYBOOK_DIR` | Default `--playbook-dir` for ad-hoc. |
| `roles_path` | list[path] | `["./roles","~/.runsible/roles"]` | `RUNSIBLE_ROLES_PATH` | Search path for packages-as-roles. |
| `collections_path` | list[path] | `["~/.runsible/packages"]` | `RUNSIBLE_COLLECTIONS_PATH` | Search path for installed packages (one "package" per §5). |
| `hash_behaviour` | enum (`replace`/`merge`) | `replace` | (none) | Dict merge mode; deprecated — prefer per-merge-site `merge = "deep"`. |

### 4.3 `[inventory]`

| Key | Type | Default | Env Var | Purpose |
|---|---|---|---|---|
| `enabled_formats` | list[enum (`toml`/`ini`/`yaml`/`json`/`script`)] | `["toml","ini","yaml"]` | (none) | Loaders to try, in order. `script` opt-in. |
| `cache` | bool | `false` | `RUNSIBLE_INVENTORY_CACHE` | Enable inventory plugin caching. |
| `cache_path` | path? | `null` | (none) | Cache location; null = `$local_tmp/inventory_cache`. |
| `cache_ttl` | int s | `3600` | (none) | Inventory cache TTL. |
| `host_pattern_mismatch` | enum (`warning`/`error`/`ignore`) | `warning` | (none) | Action when `--limit` matches no hosts. |
| `unparsed_is_failed` | bool | `true` | (none) | Fail if **all** inventory sources fail to parse. (Ansible: `false`; flipped — silent zero-host runs are a P3 hazard.) |
| `any_unparsed_is_failed` | bool | `false` | (none) | Stricter: fail if **any** source fails. |
| `ignore_extensions` | list[str] | `[".bak",".retry",".swp",".tmp"]` | (none) | Extensions ignored when reading inventory dirs. |

### 4.4 `[privilege_escalation]` — become

| Key | Type | Default | Env Var | Purpose |
|---|---|---|---|---|
| `become` | bool | `false` | `RUNSIBLE_BECOME` | Globally enable privilege escalation. |
| `method` | enum (`sudo`/`su`/`doas`/`pbrun`/`pfexec`/`runas`/`dzdo`/`ksu`/`machinectl`) | `sudo` | `RUNSIBLE_BECOME_METHOD` | Become method. |
| `user` | str | `"root"` | `RUNSIBLE_BECOME_USER` | Target user. |
| `flags` | list[str] | `[]` | (none) | Extra flags. Typed list, not a stringly-quoted blob. |
| `password_keyring` | str? | `null` | (none) | Keyring entry for the become password (per §16). |
| `password_file` | path? | `null` | `RUNSIBLE_BECOME_PASSWORD_FILE` | Fallback file. Discouraged. |
| `ask_password` | bool | `false` | (none) | Prompt if no other source. |
| `allow_same_user` | bool | `false` | (none) | Force become even when remote == become user. |
| `preflight_check` | bool | `true` | (none) | Verify become works on every host before any task runs. (New — eliminates "host #47 sudoers" surprise.) |

Dropped: `agnostic_become_prompt`, `become_exe`, `become_ask_pass`, `become_plugins`.

### 4.5 `[ssh]` — SSH transport

**Replaces** Ansible's `[ssh_connection]`. Explicitly drops `[paramiko_connection]`, `[persistent_connection]`, `[winrm_connection]`, `[connection]`. Default transport is system OpenSSH; `russh` is opt-in (per §8 of poor-decisions).

| Key | Type | Default | Env Var | Purpose |
|---|---|---|---|---|
| `executable` | path | `"ssh"` | `RUNSIBLE_SSH_EXECUTABLE` | Path to `ssh` binary. |
| `transport` | enum (`openssh`/`russh`) | `openssh` | `RUNSIBLE_SSH_TRANSPORT` | Client implementation. |
| `args` | list[str] | `["-C","-o","ControlMaster=auto","-o","ControlPersist=60s"]` | (none) | Args to all ssh CLI invocations. List, not string. |
| `extra_args` | list[str] | `[]` | (none) | Per-call extra args appended after `args`. |
| `control_path_dir` | path | `~/.runsible/cp` | (none) | Directory for ControlPath sockets. |
| `control_path_template` | str | `"%(directory)s/%%h-%%p-%%r"` | (none) | Socket filename template. |
| `pipelining` | bool | `true` | `RUNSIBLE_SSH_PIPELINING` | Pipeline module exec. Ansible: `false`; flipped — modern sane default. |
| `transfer_method` | enum (`sftp`/`scp`/`piped`/`auto`) | `auto` | (none) | File transfer strategy. |
| `connect_timeout` | int s | `10` | (none) | TCP connect timeout. |
| `reconnection_retries` | int | `0` | (none) | Retry attempts on connection failure. |
| `host_key_checking` | bool | (inherits `defaults.host_key_checking`) | `RUNSIBLE_SSH_HOST_KEY_CHECKING` | Per-transport override. |
| `known_hosts_file` | path? | `null` | (none) | Override `UserKnownHostsFile`. |
| `agent` | enum (`auto`/`none`/`<path>`) | `auto` | `RUNSIBLE_SSH_AGENT` | SSH-agent management. |

Dropped: `password`, `password_mechanism` (use keyring per §16), `pkcs11_provider` (rare; use `extra_args`), `private_key_passphrase` (use the agent), `sshpass_prompt`, `use_tty` (per-task `become.requires_tty`), per-section `verbosity` (one knob).

### 4.6 `[galaxy]`

| Key | Type | Default | Env Var | Purpose |
|---|---|---|---|---|
| `cache_dir` | path | `~/.runsible/galaxy_cache` | `RUNSIBLE_GALAXY_CACHE_DIR` | Galaxy response cache. |
| `server_list` | list[str] | `["default"]` | (none) | Names of `[galaxy.server.<name>]` stanzas, in order. |
| `server_timeout` | int s | `60` | (none) | Default API timeout. |
| `token_path` | path | `~/.runsible/galaxy_token` | `RUNSIBLE_GALAXY_TOKEN_PATH` | Token cache. |
| `disable_gpg_verify` | bool | `false` | (none) | Skip GPG verify. Loud warning when set. |
| `gpg_keyring` | path? | `null` | (none) | Custom GPG keyring. |
| `required_valid_signature_count` | str | `"1"` | (none) | Min valid sigs (`+N` requires N AND all-must-verify). |
| `ignore_signature_status_codes` | list[str] | `[]` | (none) | gpgv status codes to whitelist. |
| `ignore_certs` | bool | `false` | (none) | Skip TLS validation. Loud warning. |

Per-server stanzas: `[galaxy.server.<name>]` with `url` (str, required), `token` (str?), `username` (str?), `password_keyring` (str?), `auth_url` (str?), `client_id` (str?), `validate_certs` (bool, default `true`), `api_version` (str, default `"v3"`), `timeout` (int?), `priority` (int, default `0`).

Dropped: `collection_skeleton*`/`role_skeleton*` (flag-selectable templates in `runsible-galaxy init`), `display_progress` (TTY-detect), `import_poll_*` (built-in), `collections_path_warning`.

### 4.7 `[vault]`

Per §6 of poor-decisions, vault is asymmetric (age + SSH-key recipients), not symmetric password files.

| Key | Type | Default | Env Var | Purpose |
|---|---|---|---|---|
| `recipients_file` | path? | `"./.runsible-recipients.toml"` | `RUNSIBLE_VAULT_RECIPIENTS` | TOML file declaring default recipients. |
| `identity_file` | path? | `~/.config/runsible/age-identity.txt` | `RUNSIBLE_VAULT_IDENTITY` | Default age identity for decryption. |
| `ssh_identity_file` | path? | `~/.ssh/id_ed25519` | (none) | SSH key as decryption identity (age-ssh). |
| `keyring_namespace` | str | `"runsible:vault"` | (none) | System keyring namespace. |
| `legacy_password_file` | path? | `null` | `RUNSIBLE_VAULT_PASSWORD_FILE` | **Compat only** — Ansible-vault password file for one major version. Loud deprecation warning per use. |
| `vault_id_match` | bool | `true` | (none) | Only attempt matching vault ID on decrypt. (Ansible: `false`; flipped for security.) |

Dropped: `vault_encrypt_identity`, `vault_identity`, `vault_identity_list` — replaced by recipients-file content.

### 4.8 `[output]`

Per §10 of poor-decisions, NDJSON is default for non-TTY; pretty for TTY.

| Key | Type | Default | Env Var | Purpose |
|---|---|---|---|---|
| `format` | enum (`auto`/`pretty`/`ndjson`/`json`/`null`) | `auto` | `RUNSIBLE_OUTPUT` | Output format. `auto` = pretty on TTY else ndjson. |
| `color` | enum (`auto`/`always`/`never`) | `auto` | `RUNSIBLE_COLOR` | ANSI policy. Honors `NO_COLOR` env var as alias for `never`. |
| `event_schema_version` | str | `"v1"` | (none) | NDJSON event schema version. |
| `pretty_show_skipped` | bool | `false` | (none) | Show `skipping:` in pretty mode. (Ansible: `true`; flipped — noise.) |
| `pretty_show_ok` | bool | `true` | (none) | Show `ok:` in pretty mode. |
| `pretty_show_task_path_on_failure` | bool | `true` | (none) | file:line on failure. (Ansible: `false`; flipped — P1 needs it.) |
| `failed_to_stderr` | bool | `true` | (none) | Failure rendering to stderr. (Ansible: `false`; flipped.) |
| `display_args_in_header` | bool | `false` | (none) | Echo task args. Off (security). |
| `result_indentation` | int | `2` | (none) | Indent depth. |

Dropped: every callback-plugin sub-section. Replaced by `runsible-playbook --emit profile=top:20` style flags.

### 4.9 `[lint]`

For first-party `runsible-lint` (per §14).

| Key | Type | Default | Env Var | Purpose |
|---|---|---|---|---|
| `profile` | enum (`min`/`basic`/`safety`/`shared`/`production`) | `basic` | `RUNSIBLE_LINT_PROFILE` | Rule set. |
| `exclude_paths` | list[path] | `[]` | (none) | Paths skipped. |
| `enabled_rules` | list[str] | `[]` | (none) | Additive enables. |
| `disabled_rules` | list[str] | `[]` | (none) | Subtractive disables. |
| `warn_as_error` | bool | `false` | (none) | Promote warnings to errors. |
| `format` | enum (`pretty`/`sarif`/`json`/`github`) | `pretty` | `RUNSIBLE_LINT_FORMAT` | Output. SARIF for code-scanning. |

### 4.10 `[test]`

| Key | Type | Default | Env Var | Purpose |
|---|---|---|---|---|
| `default_targets` | list[str] | `["sanity"]` | (none) | Test categories run when none specified. |
| `parallelism` | int | (CPUs/2) | `RUNSIBLE_TEST_PARALLELISM` | Test workers. |
| `timeout` | int s | `300` | (none) | Per-test timeout. |
| `keep_artifacts` | bool | `false` | (none) | Retain artifact dirs. |
| `artifact_dir` | path | `~/.runsible/test-artifacts` | (none) | Artifact location. |
| `coverage` | bool | `false` | (none) | Capture coverage data. |

### 4.11 `[diff]`

| Key | Type | Default | Env Var | Purpose |
|---|---|---|---|---|
| `always` | bool | `false` | `RUNSIBLE_DIFF` | Implicit `--diff`. |
| `context` | int | `3` | (none) | Lines of context. |
| `max_size` | int B | `131072` | (none) | Skip diffing files larger than this; show summary. |
| `binary_summary` | bool | `true` | (none) | One-line summary for binary file changes (size, hash before/after). |

### 4.12 `[colors]`

Single section with named slots. Values are color names from a fixed catalog: `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`, `bright_black`..`bright_white`, plus `reset`. Optional 256-color via `color256:<n>` and 24-bit via `rgb:rrggbb`. No env vars (if you script colors via env you're already in NDJSON territory).

| Key | Default | Purpose |
|---|---|---|
| `changed` | `yellow` | Task `changed`. |
| `ok` | `green` | Task `ok`. |
| `skip` | `cyan` | Task `skipping`. |
| `unreachable` | `bright_red` | Host unreachable. |
| `error` | `red` | Errors. |
| `warn` | `bright_magenta` | Warnings. |
| `deprecate` | `magenta` | Deprecation warnings. |
| `debug` | `bright_black` | Debug. |
| `verbose` | `blue` | `-v` lines. |
| `highlight` | `white` | Generic highlight. |
| `diff_add` | `green` | Added lines. |
| `diff_remove` | `red` | Removed lines. |
| `diff_lines` | `cyan` | Diff metadata/headers. |
| `prompt` | `white` | Interactive prompts. |

Dropped: every `doc_*` slot (one `doc_text` suffices), `included` (no longer a status), every `console_*` (REPL inherits `prompt`).

---

## 5. CLI surface

```
runsible-config [-c CONFIG] [-v...] [--no-color] <subcommand>
```

| Subcommand | Synopsis | Purpose |
|---|---|---|
| `list` | `list [-t TYPE] [--format FMT]` | Enumerate every key with type, default, env var, description. `-t` filters to a section. `--format` ∈ `pretty`, `json`, `toml`, `markdown`, `json-schema`. |
| `show` | `show [<key>] [--format FMT]` | Resolved value for one or all keys. |
| `dump` | `dump [--format toml\|json] [--include-defaults] [--only-changed]` | Full resolved config. Default: `--only-changed`. |
| `explain` | `explain <key>` | Effective value AND every contributing source in precedence order, winner marked. Example: `defaults.forks = 20\n  └─ env RUNSIBLE_FORKS=20  ← effective\n     /etc/runsible/runsible.toml: 5\n     compiled default: 5`. |
| `init` | `init [--path PATH] [--commented] [--minimal]` | Write a default config. `--commented` produces `# key = value` for non-essentials. `--minimal` writes only keys with no sensible OS default. |
| `validate` | `validate [<path>]` | Validate against schema. Reports unknown keys, type mismatches, deprecated values, schema-version mismatches with line/column. `--strict` promotes warnings to errors. |
| `diff` | `diff <a> <b>` | Compare two configs key-by-key. Unified diff over resolved values with source attribution. `--format json-patch` for tooling. |
| `import-ansible` | `import-ansible <ansible.cfg> [--out runsible.toml]` | One-shot importer. See §7. |

**Exit codes:** `0` success; `1` validation errors; `2` argument error; `3` config from `RUNSIBLE_CONFIG`/`-c` not found; `4` schema-version mismatch (file newer than binary); `5` I/O error.

---

## 6. Env-var policy

Per §25: env vars are **opt-in per config key**. Schema declares per-key whether an env var exists and its name. **No `RUNSIBLE_*` env exists without a matching schema entry.** Examples already in §4: `defaults.verbosity` ↔ `RUNSIBLE_VERBOSITY`; `output.format` ↔ `RUNSIBLE_OUTPUT`; `vault.recipients_file` ↔ `RUNSIBLE_VAULT_RECIPIENTS`.

Rules:
1. **Unknown `RUNSIBLE_*` env vars produce a startup warning.** Downgradable to debug, never silenceable.
2. **No colon-separated path env vars.** List-typed env values are **comma-separated**; paths with commas force the TOML file. The colon-list quoting story was an Ansible UX disaster (Windows drive letters, escape rules).
3. **Strict type coercion.** Booleans: `true`/`false`/`1`/`0` only — no `yes`/`no`/`on`/`off` (Norway problem in env clothing). Ints parse cleanly; enums case-sensitive.
4. **No Ansible alias env vars.** `ANSIBLE_FORKS` is **not** consulted; `import-ansible` handles migration. Daily ops = clean `RUNSIBLE_*` namespace.
5. **`NO_COLOR`, `EDITOR`, `PAGER`, `XDG_CONFIG_HOME`** honored as cross-tool conventions; `RUNSIBLE_*` wins when both set.

Culture change for ops. Document loudly. `explain` always prints the resolution stack.

---

## 7. Migration from `ansible.cfg`

`runsible-config import-ansible <ansible.cfg> [--out runsible.toml]`. Parses the INI (handling Ansible quirks: `;` inline comments, `#`/`;` line comments, `{{ ANSIBLE_HOME }}` macro). For each `[section] key = value`:

- **Direct map** → emit (`[defaults] forks = 20` → `defaults.forks = 20`).
- **Renamed** → emit new form (`[ssh_connection] ssh_args = "..."` → `ssh.args = [...]` shell-split to list).
- **Dropped categorically** (`*_plugins`, cowsay, Python interpreter, Paramiko, persistent-conn) → `# DROPPED: become_plugins — runsible has no plugin search path. See docs/migration.md.`
- **Ansible-deprecated** (`sudo`, `ansible_managed`, `null_representation`) → comment pointing to runsible-equivalent.
- **Unknown** (third-party collection config) → `# TODO: unknown key 'foo.bar = baz' — no runsible mapping.`

Stderr summary: `imported N, dropped M, TODO P`. Exit 0 if all mapped; 1 if TODOs remain.

Special handling: `[galaxy_server.<name>]` → `[galaxy.server.<name>]`; `[colors]` mapped slot-by-slot (`bright purple` → `bright_magenta`); relative paths rewritten relative to the new file's dir. Golden-file tests in `crates/runsible-config/tests/import_ansible/`.

---

## 8. Data model (Rust types)

`Config` is the source of truth — every other crate imports it.

```rust
// crates/runsible-config/src/lib.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: String,                          // required, e.g. "1"
    #[serde(default)] pub defaults: Defaults,
    #[serde(default)] pub inventory: InventoryConfig,
    #[serde(default)] pub privilege_escalation: BecomeConfig,
    #[serde(default)] pub ssh: SshConfig,
    #[serde(default)] pub galaxy: GalaxyConfig,
    #[serde(default)] pub vault: VaultConfig,
    #[serde(default)] pub output: OutputConfig,
    #[serde(default)] pub lint: LintConfig,
    #[serde(default)] pub test: TestConfig,
    #[serde(default)] pub diff: DiffConfig,
    #[serde(default)] pub colors: Colors,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GatheringPolicy { Implicit, Explicit, Lazy }
// ... one struct per [section]; enums for every choice-typed value.
```

Design rules: `deny_unknown_fields` everywhere (unknown keys = parse error w/ line:col, non-negotiable); `Option<T>` only for genuinely-distinct null states; enums for every multiple-choice value, no stringly-typed config; companion `ConfigSchema` table (env_var, since, deprecated_in, description) drives `list`/`validate`, generated by `build.rs` from a single declarative spec file.

**Schema versioning:** current `"1"`; file == binary → load; older → run migration chain (1→2→3→current), load with note (`runsible-config migrate <path>` rewrites with backup, M2); newer → exit 4.

---

## 9. Redesigns vs Ansible

This crate implements: **§25** (env opt-in, no `RUNSIBLE_*` shadow); **§3** (5-layer precedence vocabulary; `explain` prints them); **§10** (NDJSON default for non-TTY); **§14** ([lint] is first-party); **§16** ([privilege_escalation] with `password_keyring`, `preflight_check = true`); **§6** ([vault] recipient-based, `legacy_password_file` opt-in); **§5** (one "package" concept; `roles_path`/`collections_path` may collapse in future schemas).

**Cruft dropped:** all `*_plugins` path keys; all `[paramiko_connection]`/`[persistent_connection]`/`[connection]` sections; Python interpreter discovery; cowsay; `[selinux]`; all `[callback_*]` sub-sections; `[tags]` global defaults; `module_args`/`module_name`; `display_traceback`; `internal_poll_interval`/`worker_shutdown_*`/`win_async_*`; `null_representation`/`error_on_undefined_vars`/`allow_broken_conditionals`/`allow_embedded_templates`/`ansible_managed`/`force_valid_group_names`/`invalid_task_attribute_failed`/`inject_facts_as_vars`/`jinja2_*`/`old_plugin_cache_clear`/`private_role_vars`/`retry_files_*`/`run_vars_plugins`/`show_custom_stats`/`system_warnings`/`devel_warning`/`localhost_warning`/`no_target_syslog`/`syslog_facility`/`target_log_info`/`agnostic_become_prompt`/`enable_task_debugger`/`task_debugger_ignore_errors`/`module_strict_utf8_response`.

Net: ~190 of Ansible's ~250 keys vanish. Remaining ~60 in 11 sections (vs 13), one-knob-per-concern, no aliases, no plugin sub-sections.

---

## 10. Milestones

**M0 — read + validate + show + explain.** Minimum useful crate: load `runsible.toml`, validate, print resolved config, explain a single key's source. Powers every other crate's startup. Includes `Config`, the schema metadata table, and the search-precedence resolver.

**M1 — init + dump (with comments) + import-ansible.** Onboarding completeness. Golden-file tests for `import-ansible` against 10–20 real-world `ansible.cfg` files. Also adds `--format json-schema` for IDE tooling.

**M2 — schema versioning + migration tool.** `schema_version` enforcement, the migration chain, and `runsible-config migrate <path>` (rewrites in place with backup). Required before the first v1.0 schema bump.

---

## 11. Dependencies on other crates

Foundation. Every other crate depends on it; it depends on **none**.

External (workspace deps): `serde` + derive; `toml` (read) + `toml_edit` (write-with-comments); `clap` derive (CLI); `anyhow` (binary errors); `thiserror` (library errors); `xdg` (or hand-rolled); `tracing` + `tracing-subscriber`.

Explicitly **not**: no `tokio` (config loading is sync); no `serde_yaml` (TOML-only — `yaml2toml` handles YAML elsewhere).

---

## 12. Tests

This crate's bugs are everyone's bugs.

- **Unit:** round-trip (`Config` → TOML → `Config`); env-var coercion (`true`/`false`/`1`/`0` accepted; `yes`/`on`/`off` rejected); enum variants + rejections; path expansion (`~`, `$XDG_CONFIG_HOME`, relative-to-config-dir).
- **Schema validation:** unknown top-level key → error w/ line:col; unknown sub-key → error; type mismatch → error w/ expected/got; out-of-range → error; schema-version newer than binary → exit 4; missing `schema_version` → error.
- **Precedence resolution:** default-only → all compiled defaults; file overrides default; env overrides file; `RUNSIBLE_CONFIG` overrides search-path; world-writable CWD → skipped with warning, falls through; unknown env var → warning + known vars apply.
- **`import-ansible` golden files:** `tests/golden/ansible-cfg/` with paired `<name>.cfg` and `<name>.expected.toml`. Coverage: minimal cfg, full cfg with every section, deprecated keys, plugin paths (become `# DROPPED:`), `[galaxy_server.*]`, `[colors]`, an unknown `[community.aws]`. Byte-for-byte (modulo timestamps).
- **`explain` correctness:** per layer, set value alone and assert winner; set same key at multiple layers; assert losing layers listed in order; wrong-type env var → `explain` calls out coercion failure rather than silently defaulting.
- **Integration:** end-to-end with temp-dir `runsible.toml` + env vars + `runsible-config show`; `init` → `validate` clean; `dump --include-defaults` round-trips through fresh `init`.
- **Property (`proptest`):** any valid `Config` survives `serialize → parse → serialize → parse`; `dump --only-changed` then `validate` always succeeds.

---

## 13. Risks

- **R1 — Schema bloat.** Ansible reached ~250 keys because every "nice to have" PR added one. Review bar: "real user pain requiring per-deployment tuning, or implementation detail we can hard-code?" Default to hard-coding. Each new key = CLI surface + docs + migration debt + deprecation cycle.
- **R2 — Env-var opt-in is a culture change.** Ops muscle memory says `ANSIBLE_FORKS=...` works for any FOO. First `RUNSIBLE_FOOBAR=1` warning will get filed as a bug. Mitigations: clear warning pointing at schema; every env var documented next to its key; `runsible-config list --env` emits a sourceable `export` script; lead the README/blog with the rationale.
- **R3 — Multi-source merge corners.** Even single-file has env-vs-file-vs-default edges. Decisions (all tested): empty env var = "not set" (file wins); explicit `null` in TOML = type's default (env can override); list-typed env vars **replace**, never merge.
- **R4 — `import-ansible` misses keys.** Mitigation: broad golden corpus; CI runs importer against top 100 starred Galaxy collections' `ansible.cfg`s and asserts zero `# TODO:` for in-schema keys.
- **R5 — Schema migration debt.** Once v1 ships, we own forward migration forever. Mitigation: every bump ships with a tested migration function; chain stays linear (v1→v2→v3, never a graph).
- **R6 — Windows paths.** `\`, drive letters, UNC. Schema uses `PathBuf` throughout; delegate semantics to `std::path`. CI on Windows from M1.
- **R7 — `toml_edit` for commented dumps.** Comment-preserving round-trips are fiddly (comments tied to keys, not values). Mitigation: template `init` output from the schema metadata table (write `# ...` headers per section manually) rather than relying on `toml_edit`.

---

## 14. Open questions

1. **XDG vs dotfile?** Decision: **XDG** for config; `~/.runsible/` for runtime state (cache, tmp, packages). Matches `cargo`/`gh`/`helm`. Add dotfile as deprecated fallback in M2 if users ask.
2. **`runsible-config edit`?** Open in `$EDITOR` with schema-aware comments injected. `git config --edit` ergonomics; low value for already-human-editable TOML. Defer to v1.1.
3. **`runsible-config lint` (config-of-the-config)?** Beyond `validate` — flag dead config, empty sections, env vars that would override on next run. Useful, not blocking. v1.1.
4. **Cargo-style file inheritance?** Org config extended by repo config. Ansible doesn't; users have asked for years. Decision: **defer**, but design loader so adding it is a one-function change. Schema accommodates — every `Option<T>` becomes "unset = inherit" naturally.
5. **`RUNSIBLE_HOME` mega-env?** Currently every output path is independent — relocating all requires N edits. Decision: **defer**; if asked, add `[paths] root = "..."` opt-in (vs Ansible's `{{ ANSIBLE_HOME }}` macro).
6. **Env-var override for lists of structs (e.g., `[galaxy.server.*]`)?** Decision: structured types are **TOML-file only**. Generate the TOML if you need it dynamic.
7. **`runsible-config diff` format?** Decision: unified diff for humans; `--format json-patch` for tooling.
8. **`--emit-schema`?** JSON Schema dump for IDE tooling (Ansible analogue: `ansible-doc --metadata-dump`). Cheap; vscode-runsible will want it. Decision: **yes**, M1; `runsible-config list --format json-schema`.

---

End of plan.
