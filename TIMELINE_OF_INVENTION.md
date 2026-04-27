<!-- Unlicense — cochranblock.org -->

# Timeline of Invention

*Dated, commit-level record of what was built, when, and why. Proves human-piloted AI development — not generated spaghetti.*

> Every entry below maps to real commits. Run `git log --oneline` to verify.

---

## Entries

### 2026-04-27 — runsible-playbook M1: roles

**What:** Added role/package include support. New `roles.rs` module loads a role from `packages/<name>/`, `roles/<name>/`, or `~/.runsible/cache/<name>/` (first match wins) — reading `tasks/<entry>.toml` (default entry "main"), `handlers/<entry>.toml`, `defaults/<entry>.toml`, `vars/<entry>.toml`. AST gained `[[plays.roles]]` (name + entry_point + tags + vars). Engine load sequence: pre_tasks → role tasks (in declaration order) → tasks → post_tasks. Per-host vars precedence is now: role defaults < host vars < play vars < role vars < role params (`[plays.roles.vars]`) < extra_vars. Role handlers merge into the play's handler map. Role tags propagate to every task in the role. Added `role_search_paths` override to `RunOptions` to keep tests free of `set_current_dir` races. Workspace: 148/148 tests green (was 142).
**Why:** Roles are how playbooks scale beyond a single file. Without role support, every reusable bundle (nginx setup, postgres setup, hardening rule set) must be inlined or copy-pasted. The runsible-galaxy package format is already shaped around this directory layout, so engine support unblocks the install→run loop.
**Commit:** pending
**AI Role:** AI implemented the roles loader, wired engine integration, fixed the chdir-race in tests by adding `role_search_paths` to RunOptions, and verified workspace test parallelism stays correct.
**Proof:** `~/.cargo/bin/cargo test --workspace` — 148 tests pass. Three role integration tests cover: role's tasks running in the play's task sequence, role params (`[plays.roles.vars]`) overriding role defaults, and missing-role producing a Parse error before any task runs.

### 2026-04-26 — runsible-playbook M1 expansion: 5 new modules + loop/until/block

**What:** Added 5 builtin modules (template, package, service, systemd_service, get_url) bringing the total to 13. Refactored the engine's per-task body into `execute_one_task` so block/rescue/always could recursively reuse it. Added engine support for `loop = [...]`/`loop_control { loop_var, label }` (one task execution per item, item bound in vars), `until = { expr }` + `retries` + `delay_seconds` (re-run with sleep until expression true), and `block`/`rescue`/`always` (block children fail-fast → rescue runs on failure → always always runs). Workspace: 142/142 tests green.
**Why:** Without loop, even basic playbooks (install N packages, restart M services) require N+M task duplication. Without block/rescue, error recovery is impossible without abandoning the play. Without template, every config file becomes a copy + manual sed pipeline. The 13-module catalog now covers ~80% of typical Ansible playbook content.
**Commit:** 504a470
**AI Role:** AI ran 2 parallel agents for the 5 new modules (template alone, then package+service+systemd_service+get_url together), then sequentially refactored the engine for loop/until/block. Human directed prioritization within M1.
**Proof:** `~/.cargo/bin/cargo test --workspace` — 142 tests pass (was 131); 13 builtin modules registered; loop, until, block all have integration tests.

### 2026-04-26 — Module trait refactor + 4 connection-using builtins (command/shell/copy/file)

**What:** Refactored the `DynModule` trait to take `&ExecutionContext` instead of `&Host` — a context bundle (host + vars + sync connection + check_mode flag) defined in `runsible-core::traits`. Added new sync `SyncConnection` trait (exec/put_file/slurp/file_exists) alongside the existing async `Connection` trait, plus a `LocalSync` impl in `runsible-connection` (std::process + std::fs, no async runtime). Updated all 4 existing modules (debug, ping, set_fact, assert) to use ctx.host. Implemented 4 new connection-using builtins: `command` (argv, no shell, with creates/removes idempotence guards), `shell` (sh -c with same guards), `copy` (src OR content → dest with byte-equality idempotence check), `file` (state=present/absent/directory/touch). Engine constructs a fresh `ExecutionContext { host, &vars, &LocalSync, check_mode: false }` per task per host. Workspace: 131/131 tests green.
**Why:** The M1 module library needs access to a connection — debug/ping/set_fact/assert can run engine-side, but command/shell/copy/file have to run somewhere. Adding a sync facade alongside the async trait avoids dragging tokio into every module without giving up future SSH support (M2). The ExecutionContext bundle stabilizes the trait — adding new fields (check_mode is already there; diff_mode, become, etc.) is non-breaking.
**Commit:** 1ed0079
**AI Role:** AI designed the trait split (sync facade + async original), implemented the 4 new modules end-to-end, and ran the integration smoke test (set_fact → mkdir → copy → cat → debug-from-registered-result → shell pipe → debug → cleanup, all in one run, exit 0).
**Proof:** `~/.cargo/bin/cargo test --workspace` — 131 tests pass; `./target/debug/runsible-playbook crates/runsible-playbook/examples/m1-modules.toml -i localhost,` produces a clean NDJSON stream demonstrating all 4 new modules + register + templating chained together.

### 2026-04-26 — Phase 5 partial: runsible-playbook M1 (templating, when, register, tags, handlers, set_fact, assert)

**What:** Extended the runsible-playbook engine from M0 single-task happy-path to a full M1 execution model. Added MiniJinja templating (`Templater::{render_str, render_value, eval_bool}` with strict undefined handling). Engine now: merges per-host vars (host vars + play vars + extra_vars + magic `inventory_hostname`), filters tasks by `--tags`/`--skip-tags` (with `always`/`never` semantics), evaluates `when = { expr = "..." }` as a Jinja boolean (skipped tasks emit Skipped outcome), templates task args before module dispatch, captures `register = "name"` outcomes into per-host vars, validates `notify = ["handler_id"]` at parse-time (unknown ID = TypeCheck error), and flushes handlers at end-of-play only when at least one host saw `Changed`. Added `set_fact` and `assert` builtin modules with engine-side fact merging and Jinja expression evaluation. Workspace: 123/123 tests green.
**Why:** The M0 engine could only run one informational module against one host. Real playbooks need variable interpolation, conditional execution, captured results, tag-based selective runs, and handler dispatch — these are the minimum viable feature set for converting an actual Ansible playbook (the geerlingguy.docker acceptance gate at M1 close).
**Commit:** b720aca
**AI Role:** AI implemented templating module + set_fact/assert via parallel agents, then sequentially refactored the engine to wire all M1 features through the existing event stream + outcome model. Human directed which features to prioritize within M1.
**Proof:** `~/.cargo/bin/cargo test --workspace` — 123 tests pass; `./target/debug/runsible-playbook crates/runsible-playbook/examples/m1.toml -i localhost,` exercises templating + set_fact + assert + when-skip + tags + always-tag in one run; `--tags web` correctly filters to web-only + always tasks.

### 2026-04-26 — Phase 4 M0: runsible-pull, runsible-test, runsible-console

**What:** Implemented M0 milestones for all three Phase 4 operator-tool crates in parallel. `runsible-pull`: git fetch from HTTPS/file:// URL via system `git`, spawn `runsible-playbook` against fetched bundle, atomic `heartbeat.json` write (`<path>.tmp` + rename), `runsible.pull.heartbeat.v1` schema, `--once`/`status`/`init` CLI. `runsible-test`: 7 sanity rules (S001–S007) over a runsible package directory, `units` runs `cargo test` over package's `crates/`, `env --show` discovery (Rust+cargo+sibling binaries), text+json output, dogfoods against the workspace itself. `runsible-console`: rustyline REPL, `<module> [k=v ...]` grammar reusing the synthetic-playbook engine pattern, colored summary line via `colored` crate, `quit`/`exit`/Ctrl-D exit cleanly. Workspace: 98/98 tests green.
**Why:** Phase 4 ships the operator-experience surface. Pull-mode is the P2 MSP wedge (one config per tenant + systemd timer + heartbeat). The test runner is the P3 compliance wedge (signed, reproducible test runs). The console is the P4 solo/homelab wedge (interactive ad-hoc with feedback in <100ms).
**Commit:** f139ffa
**AI Role:** AI implemented all three crates in parallel agent runs. Human directed Phase 4 execution and validated dogfooding behavior.
**Proof:** `~/.cargo/bin/cargo test --workspace` — 98 tests pass; `./target/debug/runsible-test sanity .` correctly reports S001+S007 against workspace root (expected); `echo quit | ./target/debug/runsible-console` exits clean

### 2026-04-26 — Phase 3 M0: runsible, runsible-lint, runsible-doc, runsible-galaxy

**What:** Implemented M0 milestones for all four Phase 3 surface-tool crates in parallel. `runsible` ad-hoc CLI: added `ping` module to playbook catalog, synthetic-playbook approach reuses engine, `runsible all -m runsible_builtin.ping -i localhost,` works. `runsible-lint`: 20 rules (L001–L020) across schema/style/safety categories, `text`+`json` output, `noqa` suppression, `.runsible-lint.toml` discovery, `--profile`/`--explain`/`--list-rules` CLI. `runsible-doc`: `ModuleDoc` schema, `DocRegistry`, hand-authored docs for `debug`+`ping`, `list`/`show`/`snippet` CLI with text/json/markdown output. `runsible-galaxy`: package manifest, `.runsible-pkg` tarball format, file:// registry index, greedy dependency resolver, `runsible.lock` r/w, `init`/`build`/`install`/`list`/`info`/`add` CLI. Workspace: 72/72 tests green.
**Why:** These four crates are the user-facing surface of runsible. The ad-hoc CLI (`runsible`) is the first thing any new user runs. Galaxy is the package ecosystem that makes the module catalog useful beyond the builtins. Lint and doc are the quality and discoverability tools that make the ecosystem trustworthy.
**Commit:** e72641f
**AI Role:** AI implemented all four crates in parallel agent runs. Human directed Phase 3 execution, validated test suite, reviewed provenance compliance.
**Proof:** `~/.cargo/bin/cargo test --workspace` — 72 tests pass; `./target/debug/runsible all -m runsible_builtin.ping -i localhost,` exits 0

### 2026-04-26 — Phase 2 M0: runsible-playbook engine

**What:** Implemented `runsible-playbook` M0 — TOML playbook parser, object-safe `DynModule` catalog, `debug` builtin module, plan→apply execution loop, NDJSON event stream (auto-detect TTY/non-TTY), and exit codes. Smoke test: `runsible-playbook examples/hello.toml -i localhost,` emits structured NDJSON and exits 0. 6/6 tests green. Workspace at 37/37.
**Why:** runsible-playbook is the central engine; every other crate either feeds it or is driven by it. M0 proves the end-to-end execution path: parse → type-check → plan → apply → event stream, which is the foundation every subsequent module and feature builds on.
**Commit:** f679710
**AI Role:** AI implemented all source files (ast.rs, parse.rs, catalog.rs, modules/debug.rs, engine.rs, output.rs, lib.rs, main.rs, examples/hello.toml) per the M0 spec in docs/plans/runsible-playbook.md. Human directed scope, reviewed design, validated smoke test output.
**Proof:** `~/.cargo/bin/cargo test --workspace` — 37 tests pass; `./target/debug/runsible-playbook crates/runsible-playbook/examples/hello.toml -i localhost,` exits 0

### 2026-04-26 — Phase 1: four adapter crates (parallel)

**What:** Implemented M0 milestones for `runsible-inventory`, `runsible-vault`, `runsible-connection`, and `yaml2toml` in parallel. runsible-inventory: TOML parser, range expansion, full pattern engine (union/intersection/exclusion/glob/regex), `--list`/`--host` CLI. runsible-vault: age X25519 envelope format (`$RUNSIBLE_VAULT;1;CHACHA20-POLY1305;AGE;N`), keygen, encrypt/decrypt files, encrypt-string TOML snippet, recipients list, keystore. runsible-connection: `LocalConnection` + `SshSystemConnection` (system ssh/scp), sudo become, `ConnectionSpec` builder. yaml2toml: YAML→TOML for playbook/inventory/vars profiles, null coercion, key quoting, auto-detect. 31/31 tests green across workspace.
**Why:** These four crates are the adapter layer between the outside world and the runsible engine. No engine work can proceed without inventory resolution, secret management, remote execution, and YAML import. All four are independent at M0, enabling parallel development.
**Commit:** f679710
**AI Role:** AI implemented all four crates in parallel agent runs, resolved compile errors, and verified test results. Human directed architecture, reviewed approach, validated outputs.
**Proof:** `~/.cargo/bin/cargo test --workspace` — 31 tests pass before playbook crate added

### 2026-04-26 — Phase 0: runsible-core + runsible-config M0

**What:** Scaffolded `runsible-core` (shared types/traits/errors/events) and implemented `runsible-config` M0. runsible-core: `Module` trait, `Connection` trait, `Cmd`/`ExecOutcome`/`BecomeSpec`, `Event` enum with NDJSON, `Plan`/`Outcome`/`OutcomeStatus`, all errors via thiserror. runsible-config: 8-section Config struct with deny_unknown_fields, 4-path search precedence, permission check, `show`/`list`/`dump`/`init`/`validate`/`explain` CLI. 4/4 tests green.
**Why:** runsible-core is the shared contract across the entire workspace — Module trait, Connection trait, event schema, type definitions. Without it nothing else can compile. runsible-config must be first to land so every crate can read configuration.
**Commit:** f6e1fa5 (init), f679710 (Phase 0 implementation)
**AI Role:** AI implemented all source files per plan specs, diagnosed and fixed `toml::Value` Eq incompatibility and schema_version Default bug. Human directed architecture, reviewed all designs.
**Proof:** `~/.cargo/bin/cargo test -p runsible-config` — 4 tests pass; `./target/debug/runsible-config list` shows all keys with sources

### 2026-04-26 — Project foundation: crate reservation + research + master plan

**What:** Reserved all 13 crate names on crates.io under GotEmCoach. Pulled all Ansible documentation into 12 research files (76k words). Wrote Navy Seal B2B consultancy user-story analysis across 5 personas. Cataloged Ansible poor decisions (25 items) with runsible redesigns. Wrote per-crate implementation plans for all 13 crates + MASTER.md ordering plan. Added license-preservation policy for yaml2toml conversion of third-party Ansible content.
**Why:** Building a pure-Rust Ansible replacement requires a full understanding of what Ansible does right, what it does wrong, and where the market gaps are. The research phase ensures the redesigns are principled rather than arbitrary, and the per-crate plans give every AI agent a self-contained spec to execute against.
**Commit:** f6e1fa5
**AI Role:** AI performed all research (8 parallel agent runs reading Ansible docs), synthesized user stories, cataloged poor decisions, wrote all plans. Human directed research scope ("ALL of Ansible docs, skip nothing"), validated the poor-decisions analysis, directed the B2B consultancy framing, and specified license preservation requirements.
**Proof:** `ls docs/research/ docs/plans/` — 25 files, ~135k words total
