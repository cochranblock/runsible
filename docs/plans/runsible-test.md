# runsible — `runsible-test`

## 1. Mission

`runsible-test` is the developer-facing test runner for runsible **packages** (the One Concept of §5 — playbooks, roles, and collections all collapse to "package"). It gives a package author the same three-tier safety net `ansible-test` gives a collection author — *sanity* (static, schema, structural checks), *units* (per-module Rust + per-task TOML fixtures), *integration* (the package's playbooks executed against real or containerized hosts) — but it sheds every Python-shaped check that exists in the upstream tool only because Ansible itself is Python. No `pep8`, `pylint`, `mypy`, no `validate-modules` AST inspection of `.py` files; in their place go a typed-trait conformance check, a TOML schema validator, and a *provable* idempotence claim per module (per redesign §9 — the typed `Module` trait makes second-run-no-op a property runsible-test can falsify, not hope for). It is the dogfooding harness for the runsible workspace, the gate every published runsible package passes through, and the per-PR CI driver via `--changed`.

## 2. Scope

**In:**
- `runsible-test sanity [TARGET]` — package-level static analysis: schema validity of `runsible.toml`, declared exports exist, no broken handler IDs (§13), no broken module refs, doc files match modules, tag-enum coverage (§19), changelog validity, line-ending hygiene, secret/symlink/filename safety.
- `runsible-test units [TARGET]` — runs `cargo test` for native modules plus **TOML test fixtures** for runsible-defined modules.
- `runsible-test integration [TARGET]` — runs each target's `runme.toml` against a Docker/Podman container or remote host, with alias gating (`destructive`, `needs/root`, `disabled`, `unstable`).
- `runsible-test coverage` — `start`, `stop`, `combine`, `report`, `html`, `xml`, `json`, `erase`; both Rust (via `cargo-llvm-cov`/`tarpaulin`) and **playbook** coverage (line-level reporting on TOML files — Ansible has no real equivalent).
- `runsible-test env` — shows discovered test environment.
- `runsible-test shell` — drops the user into a built test environment for debugging.
- `--changed` mode for sanity/units/integration, driven by `git diff` and an implication graph.
- JSON output (`runsible.test.v1`); JUnit XML and SARIF as alternates.
- Container-engine selection with graceful degradation.

**Out (deferred or never-doing):**
- Python-language sanity tests (`pep8`, `pylint`, `mypy`, `compile`, `import`, `no-assert`, `no-basestring`, `no-dict-iter*`, `no-get-exception`, `no-main-display`, `no-unicode-literals`, `replace-urlopen`, `use-argspec-type-path`, `use-compat-six`, `boilerplate`, `ansible-requirements`, `package-data`, `release-names`) — runsible has no user Python.
- PowerShell `pslint` — until a PowerShell connection plugin exists (v1.5+).
- `validate-modules` AST inspection — replaced by `module-trait-conformance` + `idempotence-claim` (§6).
- `ansible-doc`-driven validation — replaced by `doc-coverage` over `*.doc.toml` (§20).
- `--remote` cloud-VM provisioner — v2 (with bring-your-own credentials).
- `network-integration` and `windows-integration` as separate subcommands — folded into `integration` with aliases.
- Galaxy certification scoring — lives in `runsible-galaxy publish --check`.
- Persistent test-results database — out of scope.

## 3. Subcommands

The top-level surface, mirroring `ansible-test`'s muscle memory wherever it makes sense and dropping the rest:

```
runsible-test sanity      [TARGET...]    # static + schema + structural checks
runsible-test units       [TARGET...]    # Rust unit tests + TOML fixture tests
runsible-test integration [TARGET...]    # playbook-driven tests against hosts
runsible-test coverage    <op> [...]     # coverage data lifecycle + reports
runsible-test env         [--show|--dump|--json]
runsible-test shell       [--docker IMG|--podman IMG]
```

Each subcommand accepts a positional `TARGET` selector. Selector grammar:

- A literal target name resolves under `tests/integration/targets/<name>/` (for `integration`) or under `crates/<name>/` and `modules/<name>/` (for `units`).
- A glob (`copy*`, `network/*`) matches by name.
- `--include PATTERN` and `--exclude PATTERN` (repeatable) refine selection.
- `--list-targets` prints what would run and exits.
- A bare invocation (`runsible-test integration`) runs every enabled target.

`shell` is the debugging affordance: it provisions the same environment a test would run in and drops the user into a shell — equivalent to `ansible-test shell` in §1.9 of the research.

## 4. The integration target layout

A package's `tests/integration/targets/<name>/`:

```
tests/integration/targets/<name>/
  runsible.toml         # manifest (name, summary, aliases, env, deps)
  runme.toml            # entrypoint (replaces Ansible's runme.sh)
  runme.sh              # OPTIONAL escape hatch for shell-driven setup
  vars.toml             # test vars
  defaults.toml         # role-style defaults
  tasks/main.toml       # play body — the test
  handlers/main.toml
  files/                # static fixtures
  templates/            # template fixtures
```

`runsible.toml` for a target:

```toml
[target]
name = "copy_basic"
summary = "copy module — basic file placement, mode, ownership"
aliases = ["posix/ci", "non_destructive"]

[env]
required = ["docker", "podman"]
preferred = "docker"
images = ["default", "ubuntu2204", "alpine320"]

[deps]
targets = ["prepare_test_user"]
```

Migration from Ansible's layout (research §1.6):

| Ansible                                       | runsible                                                            |
|-----------------------------------------------|---------------------------------------------------------------------|
| `aliases` (one tag per line)                  | `aliases = [...]` inside `runsible.toml`                            |
| `meta/main.yml` `dependencies:`               | `[deps] targets = [...]` inside `runsible.toml`                     |
| `defaults/main.yml` / `vars/main.yml`         | `defaults.toml` / `vars.toml`                                       |
| `tasks/main.yml` / `handlers/main.yml`        | `tasks/main.toml` / `handlers/main.toml`                            |
| `runme.sh` + `runme.yml`                      | `runme.toml` (`runme.sh` kept as escape hatch)                      |
| `files/` / `templates/` / `library/`          | `files/` / `templates/` (no `library/`; modules in `crates/<pkg>-modules/`) |

Aliases vocabulary carries over: `destructive`, `non_destructive`, `needs/root`, `needs/privileged`, `needs/httptester`, `needs/target/<name>`, `disabled`, `unstable`, `unsupported`, `slow`, `context/controller`, `context/target`, `skip/<platform>`, `posix/ci`. Renamed: `shippable/<group>/groupN` → `ci/<group>/groupN`. New: `needs/network`, `needs/windows` (folded from `network-integration` / `windows-integration`).

## 5. Test environments

Container-first; local-host as opt-in with loud warnings; remote and cross-arch reserved for v2.

| Flag                              | Type              | Default       | Purpose                                                                  |
|-----------------------------------|-------------------|---------------|--------------------------------------------------------------------------|
| `--docker [IMAGE]`                | optional string   | `default`     | Run inside Docker. Supported tags: `default`, `ubuntu2204`, `ubuntu2404`, `rhel9`, `alpine320`, `debian12`, `fedora41`. |
| `--podman [IMAGE]`                | optional string   | `default`     | Same image set, podman backend.                                          |
| `--container-engine ENGINE`       | enum              | auto          | `docker`/`podman`/`auto`. `auto` prefers docker, falls back to podman.   |
| `--container-network NET`         | string            | unset         | Override container network.                                              |
| `--container-no-pull`             | bool              | `false`       | Do not `docker pull` before run.                                         |
| `--container-keep`                | bool              | `false`       | Keep container after run for debugging.                                  |
| `--container-privileged`          | bool              | `false`       | Pass `--privileged`. Required by some `needs/privileged` targets.        |
| `--local`                         | bool              | `false`       | Run on the host. Loud warning. Must combine with `--allow-destructive`. |
| `--rust-target TRIPLE`            | string            | host          | Reserved for cross-arch unit tests via `cross`. v2.                      |
| `--keep-git`                      | bool              | `false`       | Preserve `.git` inside the test workdir.                                 |
| `--no-temp-workdir`               | bool              | `false`       | Run in-place rather than copying to `/tmp/runsible-test-XXXX/`.          |

Absences vs. `ansible-test`: no `--venv` (no Python venv; slot held by `--rust-target` for v2 cross-arch); no `--remote` (cloud VMs deferred); no `--controller`/`--target` composite specifiers (only meaningful with plugin loading; v2).

Container requirements: POSIX shell, the runsible-builtin tooling footprint (`sh`, `cat`, `cp`, `chmod`, `chown`, `mkdir`, `mv`, `rm`, `stat`, `which`), writable `/tmp`. Not required: `sshd`, `systemd`, root user, python. Rootless first-class.

`--docker` without docker installed exits code 4 (environment error) with a clear message — never silent fall-back to `--local`, because silent fall-back to the host is the worst kind of destructive surprise.

## 6. Sanity tests catalog

Translation table from `ansible-test`'s 42 sanity tests to `runsible-test`'s set, organized by what we drop, what we rename, and what we add.

### 6.1 Dropped (Python / Ansible-shape; no analogue in runsible)

```
compile                          empty-init                       import
no-assert                        no-basestring                    no-dict-iteritems
no-dict-iterkeys                 no-dict-itervalues               no-get-exception
no-main-display                  no-unicode-literals              pep8
pylint                           replace-urlopen                  use-argspec-type-path
use-compat-six                   boilerplate                      ansible-requirements
mypy                             package-data                     release-names
test-constraints                 pslint                           validate-modules
ansible-doc                      action-plugin-docs               required-and-default-attributes
runtime-metadata (in its plugin-routing form)
```

The *concerns* behind some of these (e.g., "don't ship binary cruft" → handled by `cargo` and `git` hygiene; "doc strings exist" → see `doc-coverage` below) survive; the tests themselves do not.

### 6.2 Kept (renamed where the Ansible name was Python-shaped)

| ansible-test                | runsible-test                    | Notes                                                                  |
|-----------------------------|----------------------------------|------------------------------------------------------------------------|
| `yamllint` (on examples)    | `tomlfmt`                        | TOML formatting via `taplo`. Auto-fixable.                             |
| `package-data` (concept)    | `manifest-validity`              | Schema-validate `runsible.toml`. Hard-fail.                            |
| `changelog`                 | `changelog`                      | `CHANGELOG.md` parses + has `[unreleased]`; reno-style fragments.      |
| `line-endings`/`eol-format` | `eol-format`                     | Configurable LF/CRLF expectation per glob; default LF.                 |
| `bom`                       | `bom`                            | No UTF-8 BOM in source files.                                          |
| `no-illegal-filenames`      | `no-illegal-filenames`           | Cross-platform safety (no `:`, no reserved Windows names).             |
| `no-smart-quotes`           | `no-smart-quotes`                | ASCII quotes only in source.                                           |
| `no-unicode-escape-sequences`| `no-unicode-escape-sequences`   | Reject `\u00...` where a literal char is intended.                     |
| `no-symlinks`/`symlinks`    | `no-symlinks`                    | No broken or out-of-tree symlinks.                                     |
| `no-secrets-detected`       | `no-secrets-detected`            | Trufflehog-style scan for keys, PATs, age recipients.                  |
| `shellcheck`                | `shellcheck`                     | Gated to `runme.sh` only. Optional dependency.                         |
| `integration-aliases`       | `integration-aliases`            | Every target's `runsible.toml` declares `aliases = [...]`.             |
| `shebang`                   | `shebang`                        | Only on `runme.sh` files.                                              |
| `no-unwanted-files`         | `no-unwanted-files`              | Reject `target/`, `*.rs.bk`, `.DS_Store`. Configurable patterns.       |
| `obsolete-files`            | `obsolete-files`                 | Removed files must not reappear (per package-level removal manifest).  |
| `ignores`                   | (subsumed)                       | Replaced by `[skip]` table in per-target `runsible.toml`.              |

### 6.3 Dropped because Ansible-internal

```
botmeta                  # bot triage metadata, ansible-org-specific
bin-symlinks             # ansible-core bin/ layout
pymarkdown               # markdown style; we rely on lint
```

### 6.4 New tests (runsible-specific)

These justify the typed module trait and schema-driven package model. None have a clean Ansible analogue.

- **`module-trait-conformance`** — every native module implements the `Module` trait correctly: `Input`/`Plan`/`Outcome` are `serde::DeserializeOwned`/`Serialize`, `plan()`/`apply()`/`verify()` present with matching signatures.
- **`idempotence-claim`** — every module marked `idempotent = true` in its `*.doc.toml` is exercised: `apply()` then `verify()`; `verify()` must return an empty plan. Property-based version of redesign §9 — convention becomes proof.
- **`doc-coverage`** — every module has a sibling `<module>.doc.toml`; every `Input` parameter has a `description`; every example in the doc parses against the module's schema.
- **`schema-coverage`** — every TOML file in the package validates against its declared schema (schemas owned by `runsible-core`).
- **`tag-enum-coverage`** — every referenced tag is declared in `[tags]` (per §19); undeclared tags are an error.
- **`handler-id-resolution`** — every `notify` resolves to a declared handler (per §13); no string-typo no-ops.
- **`fact-coverage`** — every fact referenced in templates / `when` is declared in `facts.required` (per §12).
- **`vault-recipient-coverage`** — every encrypted file in `vars/` lists at least one recipient (per §6).
- **`lockfile-presence`** — `runsible.lock` exists and matches `runsible.toml` (per §15).
- **`tomllint`** — runs `runsible-lint --profile basic` over the package; lint is first-party so the runner and lint share a parser (§14).

### 6.5 Common selection flags

```
--test NAME                  # repeat; only run named tests
--skip-test NAME             # repeat; skip named tests
--list-tests                 # print all test names + grouping
--enable-test NAME           # turn on opt-in tests
--allow-disabled             # include tests marked disabled
--lint                       # machine-readable output (alias for --format pep8)
--junit                      # also write JUnit XML
--sarif                      # also write SARIF
--changed                    # only run tests implicated by git diff
--failure-ok                 # report failures but exit zero
```

## 7. CLI surface

Flag classifications: **(keep)** = preserved from `ansible-test`; **(renamed)** = same intent, new name; **(new)** = runsible-only.

### 7.1 Cross-cutting (every subcommand)

| Flag                              | Type     | Default    | Env                          | Status |
|-----------------------------------|----------|------------|------------------------------|--------|
| `-h, --help`, `--version`         | bool     | —          | —                            | keep   |
| `-v, --verbose`                   | repeat   | `0`        | `RUNSIBLE_TEST_VERBOSITY`    | keep   |
| `--color` / `--no-color`          | bool     | auto       | `NO_COLOR`                   | keep   |
| `--truncate COLS`                 | uint     | `term`     | —                            | keep   |
| `--redact` / `--no-redact`        | bool     | `--redact` | —                            | keep   |
| `--metadata PATH`                 | path     | unset      | —                            | keep   |
| `--time-limit MINUTES`            | uint     | unset      | —                            | keep   |
| `--container-keep`                | bool     | `false`    | —                            | renamed (was `--terminate {success,never,always}`) |
| `--format FMT`                    | enum     | `pretty`   | `RUNSIBLE_TEST_FORMAT`       | new — `pretty`/`json`/`junit`/`sarif`/`pep8` |
| `--output PATH`                   | path     | stdout     | —                            | new    |

### 7.2 `sanity` flags

Selection per §6.5, plus: `--prime-cache` (renamed from `--prime-venvs`); `--changed`, `--changed-from REF` (default `HEAD~1`), `--base-branch BRANCH` (default `main`), `--tracked`/`--untracked`, `--allow-disabled` (all keep).

### 7.3 `units` flags

`--coverage`, `--coverage-check` (gates against `[coverage].minimum` in `runsible.toml`); `--num-workers N` (default `nproc/2`, passed to `cargo test --jobs`); `--tb {auto,long,short,line,no}`; `--keep-failed`; `--changed` (all keep). New: `--nextest` (see §15); `--toml-fixtures`/`--no-toml-fixtures` (default on).

### 7.4 `integration` flags

`--coverage` (collects playbook coverage too); `--allow-destructive`, `--allow-disabled`, `--allow-unstable`, `--allow-unsupported`, `--allow-root`; `--retry-on-error N`, `--continue-on-error`; `--tags`, `--skip-tags`, `--start-at-task`, `--diff` (forwarded to `runsible-playbook`); `--changed`, `--changed-all-target NAME`, `--changed-all-mode {default,include,exclude}`; `--list-targets` (all keep). New: `--parallel N` runs N targets in N containers (see §15).

### 7.5 `coverage` operations

| Op           | Purpose                                                                                          |
|--------------|--------------------------------------------------------------------------------------------------|
| `start`/`stop`| Used internally by other subcommands; exposed for power users.                                  |
| `combine`    | Merge per-process / per-target data into one dataset.                                            |
| `report`/`html`/`xml`/`json`| Console / `target/runsible-coverage/{html,cobertura.xml,coverage.json}`.          |
| `erase`      | Wipe collected data.                                                                             |
| `analyze targets generate <out>` | Per-target line-attribution dataset.                                              |
| `analyze targets missing`        | Lines uncovered by any target.                                                    |

### 7.6 `env` and `shell`

`env`: `--show` (pretty), `--dump` (writes `target/runsible-test/env.json`), `--json` (stdout), `--list-files` (git-tracked file list).

`shell`: `--docker [IMAGE]`, `--podman [IMAGE]`, `--raw` (bypass setup), `--` (rest is a command in the env).

### 7.7 Dropped vs ansible-test

`--remote PLATFORM`/`--remote-*` (cloud VMs deferred); `--venv`/`--venv-system-site-packages` (no Python venv); `--python VERSION`; `--requirements`/`--requirements-mode`/`--no-pip-check`; `--controller {...}`/`--target {...}` composite specifiers (v2). Per-subcommand `--list-tests`/`--list-targets` unified.

## 8. CI mode (`--changed`)

CI mode is the most important runtime affordance for any test runner that ships with a config tool: it is what makes the tool usable on every PR. The implementation is shared across `sanity`, `units`, and `integration`.

### 8.1 Implication graph

Static `path-glob → impacted tests/targets` map. Sources:

- `runsible.toml`'s `[test.implications]` for custom mappings.
- Default: change in `crates/<pkg>-modules/<mod>/` implies `units` for `<mod>` + all integration targets that reference `<mod>` in any `tasks/*.toml`.
- Change in `tests/integration/targets/<name>/` implies that target and (transitively, via `[deps] targets`) anything depending on it.
- Change in `runsible.toml`/`Cargo.toml`/`runsible.lock` implies *all* sanity tests.
- Change in `<mod>.doc.toml` implies `doc-coverage` + `units` of `<mod>`.

### 8.2 Diff source

`--changed` (default base `HEAD~1`); `--changed-from REF`; `--base-branch BRANCH` (PR mode); `--changed-path PATH` (synthetic); `--ignore-committer NAME` (skip rebase-bot diffs).

### 8.3 Output for CI

`runsible-test sanity --changed --format json` emits one event per test + a final summary, schema `runsible.test.v1`:

```json
{ "type": "test_started", "test": "manifest-validity", "target": "my_pkg", "ts": "..." }
{ "type": "test_passed",  "test": "manifest-validity", "target": "my_pkg", "duration_ms": 12 }
{ "type": "test_failed",  "test": "module-trait-conformance", "target": "my_pkg",
  "violations": [ { "file": "...", "line": 12, "rule": "MTC0003", "message": "..." } ] }
{ "type": "summary", "passed": 41, "failed": 1, "skipped": 3, "duration_ms": 1540 }
```

When `GITHUB_ACTIONS=true`, also emits `::error file=...,line=...::message` annotations. JUnit XML and SARIF are alternates via `--junit`/`--sarif`; SARIF is the canonical for security toolchains.

## 9. Coverage

Two independent coverage surfaces, unified in one report:

### 9.1 Rust unit-test coverage

`cargo-llvm-cov` preferred, `tarpaulin` as fallback (selectable via `[coverage].engine` in `runsible.toml`). Emits `.profraw` into `target/runsible-coverage/` during `units`; `coverage combine` merges across crates and processes; renderers are HTML, Cobertura XML, JSON. `--coverage-check` reads `[coverage].minimum = 0.80` and fails if total line coverage is below the threshold.

### 9.2 Playbook coverage

This is what runsible can do that Ansible essentially cannot. The typed AST + executor instrumentation record `(file, line, task_id, host, status)` for every `tasks.*` and `handlers.*` evaluation. Sample data:

```json
{
  "file": "tests/integration/targets/copy_basic/tasks/main.toml",
  "line": 42, "task_id": "ensure_dest_dir", "kind": "task",
  "executions": [
    { "host": "ubuntu2204", "status": "ok", "duration_ms": 18 },
    { "host": "alpine320",  "status": "changed", "duration_ms": 21 }
  ]
}
```

Renders to: per-target HTML pages with hit counts and per-host outcomes; an aggregate `playbook-coverage.html`; a JSON dump for CI; a delta report (`coverage diff <a> <b>`) showing newly-covered or newly-uncovered tasks. Makes "which tasks did our integration suite never run?" a one-command answer and feeds `coverage analyze targets missing`.

### 9.3 Combined report

`runsible-test coverage report` shows both Rust coverage (per-file line %) and playbook coverage (per-task-file line %) in one table, with a unified overall percentage gate.

## 10. Redesigns vs Ansible

Summary of deliberate divergences:

- **Drop Python-language tests** — tests would be theatre (§6.1).
- **Drop the `--remote` cloud-VM provisioner** — different product surface; defer to v2.
- **Drop `validate-modules`-style AST inspection** — replaced by `module-trait-conformance` and `idempotence-claim`, which are stronger (compile-time + runtime fixture-driven). (§9, §13.)
- **Add `idempotence-claim`** — `apply()` then `verify()`; modules that lie about idempotence are caught.
- **Add `module-trait-conformance`** — surfaces compile-time trait-bound failures as a sanity test.
- **Add playbook coverage** (§9.2) — Ansible has limited support; typed AST + executor make this trivial.
- **Drop per-version `ignore-x.x.txt`** — replaced by `[skip]` in per-target `runsible.toml`. Rust forces fixes at compile time, so per-version variants have nothing to track.
- **Fold `network-integration` and `windows-integration` into `integration`** with target aliases (`needs/network`, `needs/windows`).
- **Container engine flexibility** — `--container-engine` is explicit; engine availability checked at startup with a clear error, never silent fallback to `--local`.
- **Lint as a sanity test** — `tomllint` invokes `runsible-lint` inside `sanity`. No separate "is the lint version compatible" CI job.

## 11. Milestones

### M0 — single-package sanity + units (target: 4 weeks)

- `runsible-test sanity` runs the kept + new sanity tests (no `--changed`) over a single package, with text + JSON output.
- `runsible-test units` runs `cargo test` for the package's `crates/*-modules/` and TOML fixtures for any pure-TOML modules.
- `runsible-test env --show` prints discovered environment.
- Dogfooding: passes against the runsible workspace itself.

### M1 — integration + Rust coverage (target: +6 weeks)

- `runsible-test integration` with docker and podman backends; alias gating; target dependencies.
- `runsible-test shell --docker` for debugging.
- Rust coverage via `cargo-llvm-cov`; `coverage report/html/xml/json`.
- JUnit XML output.

### M2 — `--changed` + playbook coverage + cross-arch (target: +8 weeks)

- `--changed` mode for sanity, units, integration.
- Playbook coverage instrumented into `runsible-playbook`'s executor and rendered here.
- SARIF output.
- `--rust-target` for cross-arch unit tests.
- `runsible-test coverage diff` for delta reports.

(v2 candidates, deferred: `--remote` with bring-your-own credentials; `--controller`/`--target` split; Windows/PSRemoting test environments; the `analyze targets generate / expand / filter / combine` family beyond `missing`.)

## 12. Dependencies on other crates

```
runsible-test ─ imports ─→ runsible-core      # parser, schema definitions
             ─ imports ─→ runsible-config    # config discovery + merge
             ─ imports ─→ runsible-galaxy    # package layout + manifest schema
             ─ spawns  ─→ runsible-playbook  # integration test execution
             ─ spawns  ─→ runsible-lint      # tomllint sanity test
             ─ spawns  ─→ runsible-doc       # doc-coverage validation
             ─ spawns  ─→ cargo / cargo-llvm-cov
             ─ uses    ─→ docker / podman CLI (subprocess; no library binding)
             ─ uses    ─→ git (for --changed implication graph)
```

A thin orchestrator over a parser, sibling binaries, and a container runtime. Does not duplicate `runsible-playbook`'s executor; it shells out. This makes test-runner-vs-runtime divergence (the Ansible/ansible-lint version-skew issue) impossible by construction.

## 13. Tests

Dogfooding is the primary acceptance test:

- **Self-sanity.** `runsible-test sanity` over the runsible workspace passes; CI runs this on every push.
- **Self-units.** `runsible-test units --coverage` reports a unified figure; gate at 80% for the test-runner crate itself.
- **Synthetic-failure integration.** `tests/fixtures/runsible-test/broken_pkg/` carries one of each known-bad shape (invalid `runsible.toml`, broken handler ID, undeclared tag, lying-idempotent module); `sanity broken_pkg` is asserted to fail with the exact expected violation set (snapshot).
- **Real-package integration.** `tests/fixtures/runsible-test/example_pkg/` with one ping- and one copy-style module + targets; `integration --docker default example_pkg` is asserted to pass.
- **`--changed` correctness.** Synthetic git history with known modifications; `sanity --changed --changed-from <SHA>` returns the expected implicated set (snapshot).
- **Coverage round-trip.** `units --coverage` then `coverage html`/`xml` produce valid output; xml parses against Cobertura's schema.
- **Output schema.** `--format json` validates against `runsible.test.v1` (in `crates/runsible-test/schemas/`).

## 14. Risks

- **Container engine dependency.** Many machines lack docker/podman. Mitigation: checked at startup with a clear error; fall-back to `--local` requires `--allow-destructive`. Synthetic-failure fixture runs in `--local` so there is at least one dependency-free test path.
- **Test isolation.** A misbehaving target must never corrupt the host. Mitigation: containers by default; loud warnings on `--local`; workdir is `/tmp/runsible-test-XXXXXX/` with SIGINT cleanup hook.
- **Cross-platform.** Linux day-one; macOS M1 (developer ergonomics); Windows v2. macOS units via `--rust-target` once cross-arch lands.
- **Coverage false confidence.** Line coverage tells you what executed, not what was tested. Report distinguishes "ran" from "ran with at least one assertion in the same target." Mutation testing is a v2 nice-to-have.
- **CI provider lock-in.** GitHub Actions gets annotations; others get JUnit XML and JSON. JSON event stream is the universal substrate.
- **Long-running integration suites.** `--changed` and `--parallel` are the mitigations; the implication graph is load-bearing. Snapshot tests on the implication graph itself (§13).
- **`runsible-playbook` divergence.** `runsible-test integration` shells out, so version mismatch is possible. Mitigation: record the `runsible-playbook --version` in every event stream and fail fast on major-version mismatch.

## 15. Open questions

- **`cargo nextest` vs `cargo test`?** Nextest is faster with better failure isolation but is an extra dep. Default `cargo test`; expose `--nextest` opt-in for dev + CI that has it cached. Revisit at v1.0.
- **Parallelism: per-target vs per-engine?** Different containers run in parallel cleanly. v1: `--parallel N` runs N targets in N containers. Within-target parallelism (multiple plays on different hosts) defers to v2 — the playbook executor already parallelizes across hosts.
- **Reporting format: `runsible.test.v1` vs JUnit XML alignment?** Ours carries rule-IDs cleanly; JUnit is the lingua franca. Ship both; `runsible.test.v1` is source of truth, JUnit is an emitter.
- **Package coverage gating policy.** `--coverage-check` on by default in CI? Not in v1 (would fail every fresh package); make it explicit. Revisit at v1.0.
- **`--changed` and new files?** Tentative: new file in `crates/<pkg>-modules/` implies that module's `units` + `module-trait-conformance` sanity for the whole package, not the entire suite.
- **Cross-package integration target deps?** Yes within the workspace (share `prepare_*` targets); no across published packages (hides slow startup).
- **Auto-fix in sanity?** Lint owns auto-fix; `sanity --fix` is a passthrough that defers to `runsible-lint --fix` with the same selection criteria.
