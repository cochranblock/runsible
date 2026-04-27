<!-- Unlicense — cochranblock.org -->

# runsible — work artifact

A pure-Rust, TOML-native reimplementation of Ansible. Built solo. Public domain.

## Impact

- **Cold start: ~10ms** vs Ansible's 1–3s Python startup. Ad-hoc tooling that runs hundreds of times a day is no longer waiting on a Python interpreter.
- **No more shared vault password.** Replaces ansible-vault's symmetric password-file model with age + SSH-key per-recipient encryption. Rotating a teammate is a 32-byte DEK rewrap, not a re-encrypt of every byte. The "vault password leaked on Slack" incident class goes away.
- **JIT SSH user certificates.** Operators hold only a CA private key. Each task mints a 60-second user cert before connecting; sshd enforces expiry. No long-lived keys distributed to controllers — the canonical SSH key-management nightmare is gone.
- **Idempotence enforced at the type system.** Every module implements `plan() → apply() → verify()`. The compiler refuses code that "claims to be idempotent but isn't" — vs Ansible's convention-based `changed: true` returns.
- **Parse-time errors instead of mid-play surprises.** Handler typos, undeclared tags, unknown module aliases, missing required args — all caught before any host is touched. Ansible discovers these mid-run after partially mutating production.
- **NDJSON event stream by default.** Off-TTY output is structured JSON ready for jq, log aggregators, observability pipelines. Ansible's text stream needed regex parsing.
- **5-level variable precedence** vs Ansible's 22. The single most-asked Stack Overflow question in the Ansible tag — "why is my variable wrong?" — has a deterministic 5-step answer here.
- **Per-merge-site `merge = "deep"`** instead of global `hash_behaviour`. The footgun that made Ansible's vars unsafe to share across teams is fixed.

## Scale

- 14 crates in one workspace
- 576 unit tests, all passing
- 14 / 14 TRIPLE SIMS gating binaries (each crate's public API exercised 3× via `exopack`)
- ~21,000 lines of Rust outside `target/`
- ~135,000 words of design documents — research digest of every line of Ansible documentation, plus per-crate plans and a master phasing plan

## Engine surface

`runsible-playbook` covers:

- 28 built-in modules — debug, ping, set_fact, assert, command, shell, copy, file, template, package (apt/dnf/yum dispatch), service, systemd_service, get_url, setup (fact gathering), lineinfile, blockinfile, replace, stat, find, fail, pause, wait_for, uri, archive, unarchive, user, group, cron, hostname
- Conditional `when` (Jinja boolean), `register`, `loop` + `loop_control`, `until` + `retries` + `delay_seconds`
- `block` / `rescue` / `always` (recursive)
- Handlers with typed IDs (notify typo = parse-time error)
- Roles loaded from `packages/<name>/`, `roles/<name>/`, or `~/.runsible/cache/`
- `vars_files`, `module_defaults`, `include_tasks`/`import_tasks`, `delegate_to`, `run_once`
- `gather_facts` auto-prepend; magic vars (inventory_hostname, groups, hostvars, play_hosts, ansible_check_mode, ansible_diff_mode, ansible_run_tags, ansible_version, omit)
- `--check` / `--diff` (skips apply for mutating modules; emits unified before/after)
- `--forks N` parallel host execution via tokio JoinSet
- `--list-tasks` / `--list-hosts` / `--syntax-check` / `--start-at-task` / `--tags` / `--skip-tags`
- 43 MiniJinja filters + 16 tests + 10 lookup functions matching the Ansible catalog

## Surface tools

- `runsible` — ad-hoc CLI (synthetic-playbook over the engine), parity with `ansible -m <module> [pattern]`
- `runsible-galaxy` — package manager: manifest, tarball, file:// registry, lockfile, dependency resolver
- `runsible-doc` — module documentation registry, all 28 builtins documented, text/markdown/JSON render
- `runsible-lint` — 50 lint rules across schema, idiom, and safety bands
- `runsible-pull` — git fetch + apply + atomic heartbeat, with daemon mode + HTTP POST + retry queue + jitter
- `runsible-test` — 7 sanity rules + cargo unit-test runner
- `runsible-console` — rustyline REPL with tab completion + persistent history
- `runsible-vault` — keygen, encrypt, decrypt, recipients add/remove/rekey, encrypt-string, import-ansible
- `runsible-config` — TOML config with 8 sections + search-path precedence
- `runsible-inventory` — TOML/YAML/INI parser, host_vars/group_vars dirs, full pattern grammar
- `runsible-connection` — local + system SSH with JIT CA cert minting
- `yaml2toml` — YAML → TOML converter (playbook/inventory/vars profiles)
- `runsible-core` — shared traits and event schema

## Provenance

- `TIMELINE_OF_INVENTION.md` — commit-level record with What / Why / Commit / AI-Role / Proof fields per entry
- `PROOF_OF_ARTIFACTS.md` — architecture, build metrics, validation table, "How to verify" commands
- Every crate ships an `f30()` entrypoint exercising real public API end-to-end; the `<crate>-test` binary runs it three times via `exopack::triple_sims::f60` — all three must pass for the gate to exit 0
- License: Unlicense (public domain) on the entire workspace

## Contact

mclarkfyrue@gmail.com
