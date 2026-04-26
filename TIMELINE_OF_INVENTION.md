<!-- Unlicense — cochranblock.org -->

# Timeline of Invention

*Dated, commit-level record of what was built, when, and why. Proves human-piloted AI development — not generated spaghetti.*

> Every entry below maps to real commits. Run `git log --oneline` to verify.

---

## Entries

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
