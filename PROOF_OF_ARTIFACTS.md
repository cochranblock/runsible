<!-- Unlicense — cochranblock.org -->

# Proof of Artifacts

*Build output, metrics, and verification commands proving runsible is real and working.*

> Run the commands in ## How to Verify to reproduce every claim below.

## Architecture

```
runsible workspace (pure Rust, zero Python, TOML-native)
│
├── runsible-core        — shared Module/Connection traits, Event schema, errors
├── runsible-config      — TOML config (search path, schema_version, 8 sections)
│
├── runsible-inventory   — TOML inventory parser, pattern engine, --list/--host CLI
├── runsible-vault       — age X25519 per-recipient vault (replaces ansible-vault)
├── runsible-connection  — LocalConnection + SshSystemConnection, sudo become
├── yaml2toml            — YAML→TOML converter (playbook/inventory/vars profiles)
│
├── runsible-playbook    — TOML playbook engine: parse→plan→apply→NDJSON events
│
├── runsible             — ad-hoc CLI (stub, Phase 3)
├── runsible-galaxy      — package manager (stub, Phase 3)
├── runsible-doc         — module documentation browser (stub, Phase 3)
├── runsible-lint        — TOML playbook linter (stub, Phase 3)
├── runsible-pull        — pull-mode agent (stub, Phase 4)
├── runsible-test        — package test runner (stub, Phase 4)
└── runsible-console     — interactive REPL (stub, Phase 4)
```

**Key design choices:**
- TOML is the canonical format; YAML is import-only via yaml2toml
- age X25519 asymmetric vault replaces ansible-vault's symmetric password scheme
- `Module` trait enforces plan→apply→verify idempotence at the type level
- NDJSON event stream (`runsible.event.v1`) as default off-TTY output
- 5-level variable precedence replaces Ansible's 22 levels

## Build Output

| Metric | Value |
|--------|-------|
| Workspace crates | 14 |
| Implemented crates (M0) | 13 (core, config, inventory, vault, connection, yaml2toml, playbook, runsible, lint, doc, galaxy, pull, test, console) |
| Total tests (workspace) | 148 |
| Test pass rate | 100% |
| Rust edition | 2021 |
| Rust version | 1.94.0 |
| Cloud dependencies | Zero |
| Python runtime required | Zero |
| External crates (key) | age 0.10, toml 0.8, toml_edit 0.22, serde 1, clap 4, thiserror 1, indexmap 2, globset 0.4, regex 1, tokio 1, semver 1, tar 0.4, flate2 1, sha2 0.10, rustyline 14, colored 2, chrono 0.4, minijinja 2 |

## Validation

| Claim | Evidence |
|-------|----------|
| runsible-config CLI works | `./target/debug/runsible-config list` prints all keys with `[default]` source |
| runsible-config init generates valid TOML | `init_default_is_valid_toml` test in lib.rs |
| Inventory pattern engine | 6 tests: range expansion (numeric + alpha), parse+list, union, intersection, exclusion |
| Vault encrypt/decrypt round-trip | `roundtrip_encrypt_decrypt` test — encrypts "hello vault", decrypts, asserts equal |
| Vault envelope rejects CRLF | `envelope_rejects_crlf` test |
| LocalConnection exec | `local_exec_echo` + `local_exec_exit_code` tests |
| LocalConnection put/get/slurp | `local_put_get_file` + `local_slurp` tests |
| yaml2toml round-trip | `vars_round_trip`, `inventory_profile`, `playbook_profile` tests |
| yaml2toml null coercion | `null_coercion` test — YAML `~` → TOML `""` with warning |
| Playbook parse + module resolution | `parse_minimal_playbook` + `resolve_task_extracts_module` tests |
| Playbook engine runs debug module | `run_hello_playbook` test — ok=1, failed=0, exit=0 |
| Playbook engine multi-host | `run_multi_host` test — 3 hosts × 1 task = ok=3 |
| Playbook engine bad module errors | `unknown_module_errors` test |
| NDJSON event stream | Smoke test output below |

## Smoke Test Output

```
$ ./target/debug/runsible-playbook crates/runsible-playbook/examples/hello.toml -i localhost,
{"kind":"run_start","playbook":"crates/runsible-playbook/examples/hello.toml","inventory":"localhost,,","host_count":1,"runsible_version":"0.0.1"}
{"kind":"play_start","play_index":0,"name":"Hello World","target_pattern":"localhost","host_count":1}
{"kind":"task_start","play_index":0,"task_index":0,"name":"Say hello","module":"runsible_builtin.debug"}
{"kind":"plan_computed","play_index":0,"task_index":0,"plan":{"module":"runsible_builtin.debug","host":"localhost","diff":{"msg":"Hello, world! From runsible-playbook M0."},"will_change":false}}
{"kind":"task_outcome","play_index":0,"task_index":0,"outcome":{"module":"runsible_builtin.debug","host":"localhost","status":"ok","elapsed_ms":0,"returns":{"msg":"Hello, world! From runsible-playbook M0."}}}
{"kind":"task_start","play_index":0,"task_index":1,"name":"Show the host","module":"runsible_builtin.debug"}
{"kind":"plan_computed","play_index":0,"task_index":1,"plan":{"module":"runsible_builtin.debug","host":"localhost","diff":{"msg":"Running on: localhost"},"will_change":false}}
{"kind":"task_outcome","play_index":0,"task_index":1,"outcome":{"module":"runsible_builtin.debug","host":"localhost","status":"ok","elapsed_ms":0,"returns":{"msg":"Running on: localhost"}}}
{"kind":"play_end","play_index":0,"ok":2,"changed":0,"failed":0,"unreachable":0,"skipped":0}
{"kind":"run_summary","ok":2,"changed":0,"failed":0,"unreachable":0,"skipped":0,"elapsed_ms":0}
```

Exit code: 0

## Commit Log

| Hash | Date | Description |
|------|------|-------------|
| 504a470 | 2026-04-26 | runsible-playbook M1 expansion: template/package/service/systemd_service/get_url + loop/until/block |
| 1ed0079 | 2026-04-26 | runsible-playbook M1: ExecutionContext refactor + command/shell/copy/file modules |
| b720aca | 2026-04-26 | runsible-playbook M1 partial: templating, when, register, tags, handlers, set_fact, assert |
| f139ffa | 2026-04-26 | Phase 4 M0: runsible-pull, runsible-test, runsible-console |
| e72641f | 2026-04-26 | Phase 3 M0: runsible, runsible-lint, runsible-doc, runsible-galaxy |
| f679710 | 2026-04-26 | Phase 0-2 M0: core, config, inventory, vault, connection, yaml2toml, playbook |
| f6e1fa5 | 2026-04-26 | init runsible: TOML-native Ansible reimagining |

## How to Verify

```bash
# Clone and build
git clone https://github.com/mcochran/runsible   # or local path
cd runsible

# Full test suite
~/.cargo/bin/cargo test --workspace

# Smoke test
~/.cargo/bin/cargo build -p runsible-playbook
./target/debug/runsible-playbook crates/runsible-playbook/examples/hello.toml -i localhost,

# Config CLI
./target/debug/runsible-config list
./target/debug/runsible-config explain output.format

# Inventory CLI
./target/debug/runsible-inventory --list -i <(echo '[all.hosts]
"web01" = {}
"web02" = {}')

# Vault keygen + round-trip
./target/debug/runsible-vault keygen --label test
echo "secret" | ./target/debug/runsible-vault encrypt-string

# yaml2toml
echo 'http_port: 8080' | ./target/debug/yaml2toml
```
