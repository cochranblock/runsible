# runsible — `runsible-galaxy`

## 1. Mission

`runsible-galaxy` is the package manager for the runsible ecosystem: install, build, publish, verify runsible packages; resolve dependencies via a SAT-style solver; own the project lockfile; provide one-way import paths from Ansible content. Crucially, runsible has exactly **one** unit of redistribution — the **package** (per §5 of `11-poor-decisions.md`). Roles are packages. Collections are packages. Standalone Galaxy roles are packages. The taxonomy that costs every Ansible newcomer a week of "wait, what's the difference?" is collapsed at the schema level. Where Ansible ships three reuse mechanisms with three lifecycles and a `requirements.yml` whose collection and role halves share a filename but not a syntax, runsible-galaxy ships one. The crate is also the project's beachhead for reproducibility (`runsible.lock`), conflict-time detection (SAT, not single-pass walking), and cryptographically honest provenance (signed manifests by default in v1.5+).

---

## 2. Scope

**In scope:** the runsible-native package format (`runsible.toml` + canonical layout) and tarball artifact (`.runsible-pkg`, zstd-compressed tar); `add`/`install`/`update`/`remove`/`list`/`info`/`build`/`publish`/`verify`/`init`/`login`/`logout`/`yank` plus the SAT-resolved graph behind them; `runsible.lock` schema, lifecycle, CLI interactions; a documented HTTP registry contract (Galaxy v3 it is **not** — fresh Cargo/crates.io-shaped wire format) and an opinionated default registry at `registry.runsible.dev`; one-way Ansible import (`import-ansible-role` / `import-ansible-collection` emit a package skeleton + TODO list of plugins/filters/modules that didn't transfer); detached signatures (PGP v1; sigstore/cosign v1.5); multi-registry support (default + private + per-org), credentials at `~/.runsible/credentials.toml`; offline mode with file:// registries and pre-downloaded tarballs.

**Out of scope:** loading Python plugins from imported collections at runtime (we refuse — if a collection's value is Python plugins, the import tool reports "this collection cannot be ported until those plugins are ported"); installing arbitrary Python from collections (no `requirements.txt`, no pip); honoring Ansible's `meta/runtime.yml` `plugin_routing` / `import_redirection` at runtime (we *read* it during import for TODO breadcrumbs but runsible packages have no runtime redirect mechanism — references resolve at parse time via lexical `[imports]` aliases per §22 of poor-decisions); a web UI for the registry (JSON API only in v1/v1.5; web frontend can land in v2); parsing legacy `requirements.yml` at install time (the importer reads it once and emits a `runsible.toml` `[dependencies]` block).

---

## 3. The runsible package format

### 3.1 The manifest (`runsible.toml`)

Every package is rooted at a directory containing `runsible.toml`. The manifest is the single source of truth at development time; the on-disk artifact carries a copy plus integrity metadata; the tarball is byte-deterministic given a source tree.

```toml
[package]
name           = "core_apt"                  # lower_snake_case, [a-z][a-z0-9_]*
version        = "1.4.2"                     # strict semver
description    = "Idempotent apt package and repository management"
license        = "Apache-2.0"                # SPDX identifier (single string)
authors        = ["Jane Roe <jane@example.com>"]
repository     = "https://github.com/runsible/core_apt"
documentation  = "https://docs.runsible.dev/packages/core_apt"
homepage       = "https://runsible.dev"
issues         = "https://github.com/runsible/core_apt/issues"
runsible       = ">=1.0.0,<2.0.0"            # required runsible runtime range
keywords       = ["apt", "debian"]
categories     = ["system"]                  # fixed enum, crates.io style

[exports]
modules        = ["modules/apt.toml", "modules/apt_repository.toml"]
tasks          = ["tasks/install.toml"]
templates      = ["templates/sources.list.j2"]
handlers       = ["handlers/restart_apt.toml"]
vars           = ["vars/defaults.toml"]
playbooks      = ["playbooks/baseline.toml"]

[dependencies]
core_facts     = ">=1.0.0,<2.0.0"
core_service   = "^1.2"
gpg_utils      = { version = "0.5", registry = "registry.runsible.dev" }
secret_loader  = { git = "https://github.com/me/secret_loader.git", rev = "v0.3.1" }
local_helper   = { path = "../local_helper" }   # path deps are dev-only; rejected on publish

[dev-dependencies]
fixture_apt    = "^0.1"

[build]
ignore         = [".git/**", "**/*.swp", "tests/output/**"]   # ADDITIONAL to defaults

[signatures]
required       = false                        # opt-in in v1; default-on in v1.5
keyring        = "~/.runsible/keyrings/registry.runsible.dev.kbx"
```

**Field rationale, vs Ansible:**

- `name` is a single segment — no `namespace.name`, no FQCN. Names globally unique within a registry; default registry adopts flat namespace (à la crates.io). Anyone wanting `community.general` names their package `community_general`. FQCN was Ansible's accommodation of a community + vendor parallel ecosystem; runsible begins as one.
- `version` is **strict** semver. Ansible accepts but doesn't enforce; we refuse to build invalid `semver::Version`.
- `license` is a single SPDX identifier or expression (`"MIT OR Apache-2.0"`). Ansible's `license` xor `license_file` split is collapsed. Custom licenses: `LicenseRef-*` plus a `LICENSE` file.
- `runsible = ">=1.0.0,<2.0.0"` replaces `meta/runtime.yml`'s `requires_ansible` — top-level, not buried.
- `[exports]` is the **explicit** contributions list. **No directory-walking magic.** Ansible inferred plugin types from directory names; we don't. `runsible-doc` renders surface area without parsing 500 files; `runsible-lint` detects orphans; builder refuses unreferenced files.
- `[dependencies]` accepts string ranges and structured forms (`{ version, registry, git, path }`). Path deps are dev-only; build refuses to ship them.
- `[build].ignore` is **additional** to implicit defaults: `.git/**`, `.hg/**`, `.svn/**`, `**/*.swp`, `**/*.bak`, `**/*~`, `**/__pycache__/**`, `**/*.pyc`, `**/*.retry`, `target/**`, `node_modules/**`, plus existing `.runsible-pkg` files in source root. Ansible's `build_ignore` famously *doesn't* include defaults, a footgun (§13.3 of `05-collections-galaxy.md`). Ours always do.

### 3.2 The on-disk layout

```
my_pkg/
  runsible.toml                  # required
  README.md                      # required
  LICENSE                        # required for LicenseRef-*; recommended otherwise
  CHANGELOG.md                   # optional
  modules/                       # optional; in [exports.modules]
    apt.toml                     # module manifest
    apt.rs                       # impl (compiled to .so/.dylib at publish time)
    apt.doc.toml                 # docs (per §20 of poor-decisions)
  tasks/  templates/  handlers/  vars/  playbooks/   # optional; each in [exports.*]
  files/                         # optional payloads; not in [exports]
  tests/                         # not bundled; dev-only
```

**Mandatory:** `runsible.toml`, `README.md`. Everything else optional and explicit.

vs Ansible's collection layout (§2 of `05-collections-galaxy.md`): we drop the 19-plugin-type taxonomy. No `plugins/become/`, `plugins/cliconf/`, etc. as separate directories. Five export categories: modules, tasks, templates, handlers, vars (plus playbooks). Future plugin types are typed `[exports]` entries — never magic directories. We also drop `meta/runtime.yml` entirely; version constraint moves to `[package].runsible`.

vs standalone Galaxy roles: standalone roles ship `library/`, `module_utils/`, `lookup_plugins/`, `filter_plugins/` mixed with task content. runsible packages can't — modules go in `modules/`. Deliberate compat break; the importer flattens these and emits TODOs for anything Python.

### 3.3 The tarball format (`.runsible-pkg`)

The publishable artifact is `<name>-<version>.runsible-pkg` — a zstd-compressed tarball with the on-disk layout plus a `.runsible/` integrity directory containing `MANIFEST.toml` and `FILES.toml`.

- **`MANIFEST.toml`** — `[package]` (verbatim from source) + `[manifest]` (`format_version`, `built_at` RFC 3339 UTC, `built_by`, `files_hash` BLAKE3 of `FILES.toml`).
- **`FILES.toml`** — one `[[file]]` entry per included file: `path`, `size`, `sha256`, `blake3`. Both hashes ship: SHA-256 for interop, BLAKE3 because it's the right hash for new code. Ordering lexicographic (NFC-normalized) for cross-OS determinism.
- Tarball entries written in lexicographic path order with deterministic uid=0/gid=0/mtime (UTC midnight of build day). Matches Cargo/Bazel reproducibility.
- Zstd level 19. `tar.zst`, not `tar.gz`; `.runsible-pkg` extension is unambiguous.
- **Size limit:** default registry enforces 50 MB uncompressed (Ansible's is 20 MB compressed); self-hosted registries configure their own.

TOML for inner manifests (vs Ansible's JSON) because runsible is TOML-native end-to-end — single parser surface.

---

## 4. The lockfile (`runsible.lock`)

`runsible.lock` lives at the project root next to `runsible.toml`. It is **always** committed. Every runsible-galaxy command that mutates the dependency graph reads-modifies-writes it.

### 4.1 Schema

```toml
# runsible.lock — DO NOT EDIT; regenerated by runsible-galaxy
version = 1

[[package]]
name      = "core_apt"
version   = "1.4.2"
source    = "registry+https://registry.runsible.dev"
checksum  = "blake3:8c91b2..."
dependencies = ["core_facts 1.0.5", "core_service 1.2.7"]

[[package]]
name      = "secret_loader"
version   = "0.3.1"
source    = "git+https://github.com/me/secret_loader.git#v0.3.1#a4b9c1..."
checksum  = "blake3:7e22d5..."

[metadata]
generated_by  = "runsible-galaxy 1.0.0"
generated_at  = "2026-04-26T19:14:02Z"
solver        = "pubgrub"
solver_seed   = "blake3-of-runsible-toml-and-registry-state"
```

Closely modeled on `Cargo.lock`. Departures: source URL embeds resolved commit/checksum for git deps; BLAKE3 first-class; `[metadata]` captures solver name and seed so `verify-lock` can re-solve and prove the lock matches.

### 4.2 Lifecycle

- `install` reads lock (preferred); writes if absent. If lock matches manifest, install exactly the locked versions. Missing lock: solve, write, install. Inconsistent lock: error and instruct `update`.
- `add <pkg>` solves with the new dep, rewrites lock, installs.
- `remove <pkg>` resolves without the dep, rewrites lock, uninstalls newly-orphaned transitives.
- `update [pkg]` re-solves allowing newer versions within manifest ranges; with arg, only that pkg + descendants; `--aggressive` discards lock entirely.
- `verify-lock` re-solves and asserts the output matches the lock; nonzero exit on drift.
- `list` / `info` render only.

Cargo-style `--frozen` (refuse any lock mutation) and `--locked` (refuse version updates but allow filling missing entries) ship. Default is `--locked` in CI (detected via `CI=true`) and lenient interactively.

### 4.3 What the lock protects against

A transitive dep shipping a breaking patch (§11 of poor-decisions); a registry mirror serving a different artifact for the same name+version (checksum mismatch = hard error); a git dep's branch silently moving (rev pinned); two engineers resolving to different versions.

---

## 5. Dependency resolution

### 5.1 Algorithm: PubGrub

We use **`pubgrub`** (Rust implementation of the algorithm originated for Dart's `pub`, adopted by `uv`, modern `pip`, `poetry`).

- **`pubgrub`** produces *human-readable conflict explanations* — "you required A 1.0 which requires B >=2.0; you also required C 1.0 which requires B <2.0; no solution exists." Exactly the UX Ansible's single-pass walker doesn't produce. Stable 0.2.x; production-proven by uv; milliseconds on real graphs.
- **`resolvo`** is the strong alternative — Conda uses it; handles platform-specific deps elegantly. We don't need that yet (runsible packages are platform-agnostic). Reconsider for v2 if we admit platform-specific variants.
- A hand-rolled SAT (z3, cadical) is overkill: C dependency, learning curve. PubGrub is purpose-built and explains failures.

### 5.2 Version range syntax

Cargo-style: `"1.2.3"` = `^1.2.3`; `"^1.2.3"` = `>=1.2.3, <2.0.0`; `"~1.2.3"` = `>=1.2.3, <1.3.0`; `"=1.2.3"` = exactly; `">=1.2, <2"` = composed; `"*"` = any (lints warn); `"1.2.3-beta.1"` = exact pre-release pin. Pre-releases excluded by default; opt in per-dep via `=` pin or globally via `--include-prerelease`.

### 5.3 Conflict reporting UX

PubGrub's killer feature is the explanation:

```
error: failed to resolve dependencies for project `my_baseline`

Because every version of core_facts depends on system_collector >=2.0.0
  and core_apt 1.4.2 depends on system_collector >=1.0.0, <2.0.0,
  core_facts and core_apt 1.4.2 are incompatible.
And because the project depends on both core_apt 1.4.2 and core_facts,
  no solution is possible.

Hints:
  - Try `runsible-galaxy info system_collector` for available versions
  - To override, use `--allow-conflict system_collector` (at your own risk)
  - Re-run with -vv for the full derivation tree
```

`--allow-conflict` is intentionally hard to discover; emergency hatch that picks the highest-pinned version and leaves an angry comment in the lockfile. We do **not** ship Ansible's `--ignore-errors` — that flag was a band-aid for a single-pass solver that couldn't backtrack.

### 5.4 Multi-registry resolution

Per-dep registry targeting via `{ version = "1.0", registry = "internal" }`. Resolved against `~/.runsible/credentials.toml`. PubGrub queries each registry only for deps that target it. **No `server_list`-style implicit fallback** — Ansible's is a footgun: a higher version on a later server is silently ignored if a lower acceptable version is on an earlier one (§13.11 of `05-collections-galaxy.md`). Each dep is unambiguous about its registry.

---

## 6. The registry contract

### 6.1 HTTP API

JSON wire format; tarball blob bodies. Auth via `Authorization: Bearer <token>`.

**Read (anonymous on public registries):**
- `GET /v1/packages/<name>` → `{ name, versions[], yanked[], owners[], metadata{description, repository, ...} }`
- `GET /v1/packages/<name>/<version>` → `{ name, version, checksum_blake3, checksum_sha256, size_bytes, dependencies[{name, req, registry}], license, yanked, published_at, published_by, download_url, signatures[{type, url, keyid}] }`
- `GET /v1/packages/<name>/<version>/download` → 200 with `Content-Type: application/vnd.runsible.pkg+zstd`; or 302 to CDN
- `GET /v1/packages/<name>/<version>/manifest` → 200 with the package's `runsible.toml`, JSON-encoded (for tools; solvers use the deps endpoint)
- `GET /v1/search?q=apt&category=system&limit=20` → `{ results[], total }`

**Write (require auth):**
- `POST /v1/packages/<name>/<version>/publish` (body: the `.runsible-pkg`) → 201; 409 if (name, version) exists (versions are immutable); 403 if user is not a registered owner
- `POST /v1/packages/<name>/<version>/yank` → 204 (crates.io semantics)
- `POST /v1/packages/<name>/<version>/unyank` → 204
- `POST /v1/packages/<name>/owners` (body: `{ "owner": "alice" }`) → 201/204
- `DELETE /v1/packages/<name>/owners/<user>` → 204

**Admin / health:** `GET /v1/registry-info` → `{ name, version, max_package_size_bytes, supports_signatures[], supports_compression[] }`; `GET /healthz` → 200 "ok".

### 6.2 Auth model

API tokens live in `~/.runsible/credentials.toml` (mode `0600`; loader refuses world-readable files), one entry per registry name. Created via `runsible-galaxy login [--registry NAME]` (browser → callback → file); `logout` deletes. Token format `rsbl_pat_*` makes leaked tokens grep-able for the registry's secret-scanning sweep.

### 6.3 Hosting policy

We host an opinionated default registry at **`registry.runsible.dev`** (placeholder; canonical URL settled at v0.9). It's a small Axum HTTP service backing onto Postgres + S3-compatible blob storage; AGPL-licensed (encourages private re-deploys to publish patches). Reference deployment fits a 1-vCPU container plus a managed database.

**Why host one rather than punt.** Package managers without a default registry don't reach critical mass. nix's flakes have one, Cargo has one, npm has one. Ansible's Galaxy *is* effectively a default; we shouldn't regress UX by making every user pick a server URL on day one. Hosting is a real cost (bandwidth dominates) but it's the price of admission.

**Self-hosting is first-class.** Configure via `~/.runsible/config.toml`:

```toml
default-registry = "internal"
[registries.internal]
url = "https://artifactory.acme.com/runsible/"
```

`--registry NAME` targets a specific one; `--registry-url URL` for ad-hoc read-only. **Mirror policy:** the default registry publishes a daily index dump (tarball of all `(name, version) → manifest` records) for air-gapped seeding.

---

## 7. CLI surface

`runsible-galaxy` is the binary. All subcommands accept `-v/-vv/-vvv` for verbosity, `--registry NAME`, `--config PATH`, `--no-network` (synonym for `--offline`), `-q/--quiet`.

- **`init <name> [--path DIR] [--license SPDX] [--vcs git|hg|none] [--template TEMPLATE]`** — scaffolds a skeleton at `DIR/<name>/`. `<name>` matches `[a-z][a-z0-9_]*`. Defaults: `--license = "Apache-2.0 OR MIT"`, `--vcs = git`. Templates: `module` / `tasks` / `playbook` (default `tasks`).
- **`add <pkg>[@version] [--registry NAME] [--git URL] [--rev REV] [--branch BR] [--tag T] [--path PATH] [--dev]`** — adds a dep to `runsible.toml` (or `[dev-dependencies]` with `--dev`); re-solves; updates lock. Refuses duplicates without `--force`.
- **`remove <pkg> [--dev]`** — drops the dep, re-solves, removes orphaned transitives.
- **`install [--frozen] [--locked] [--offline] [--registry NAME] [--no-verify-checksum]`** — installs per lockfile; fresh solve without one.
- **`update [<pkg>...] [--aggressive] [--dry-run]`** — re-solves with newer versions allowed; with args, only those (+ descendants).
- **`list [--installed] [--outdated] [--depth N] [--format json|toml|text]`** — `--outdated` consults the registry for newer versions within manifest ranges.
- **`info <pkg>[@version] [--versions] [--registry NAME] [--offline]`**.
- **`build [--out-dir DIR] [--no-verify-toml] [--allow-dirty]`** — verifies clean working tree; validates manifest; refuses `path =` deps; computes deterministic FILES/MANIFEST; writes tarball to `./target/runsible-pkg/` by default.
- **`publish [--registry NAME] [--dry-run] [--token TOKEN] [--allow-dirty]`** — builds if needed and uploads.
- **`verify [<pkg>...] [--all] [--offline] [--keyring PATH] [--required-signatures N]`** — re-hashes installed packages against lock checksums and in-tarball integrity files; with `--keyring`, verifies signatures.
- **`import-ansible-role <git_url|tarball|local_path> [--out DIR] [--name NAME] [--version VERSION]`** — walks an Ansible role; emits package skeleton + `IMPORT_TODO.md` listing every Jinja filter, module reference, `meta:` action, and plugin file (`library/`, `module_utils/`, `lookup_plugins/`, `filter_plugins/`) that didn't transfer.
- **`import-ansible-collection <name|tarball|git_url> [--out DIR] [--split-by-role]`** — same for collections. **No Python plugins translated.**
- **`login [--registry NAME]` / `logout`** — browser flow → `~/.runsible/credentials.toml`; `logout` deletes.
- **`yank <pkg>@<version>` / `unyank`** — crates.io semantics: yanked versions remain installable from existing locks; new resolves skip them.
- **`cache list | clear [--older-than 30d] | info <pkg>@<version>`** — cache at `~/.runsible/cache/`; configurable.

---

## 8. Legacy import (Ansible compatibility)

The bridge into Ansible's userbase. The published Galaxy roles and collections are the network effect we are stealing from; the import tool is our olive branch.

### 8.1 `import-ansible-role`

Walks a standalone Galaxy role:

1. Parses `meta/main.yml` (`galaxy_info` + `dependencies`).
2. Field maps: `role_name` → `[package].name` (hyphens → underscores); `namespace` dropped (use `--keep-namespace`); `author` → list-wrapped `authors`; `description`/`license` → corresponding (SPDX-normalized; falls back to `LicenseRef-Custom`); `min_ansible_version` is **not** mapped to `[package].runsible` (different versioning; importer warns); `platforms` + `galaxy_tags` → `keywords` (deduped); `dependencies` (list of `namespace.role_name`) → `[dependencies]` with version `*` and a TODO entry per dep.
3. Walks `tasks/main.yml` etc. and converts via the workspace's `runsible-yaml2toml` crate to `tasks/`.
4. Copies `templates/` verbatim (Jinja preserved — runtime rejects unsupported filters at parse time, no import-time decision); copies `files/`; converts `handlers/`, `vars/`, `defaults/` via yaml2toml.
5. Emits all plugin directories (`library/`, `module_utils/`, `lookup_plugins/`, `filter_plugins/`, `action_plugins/`, `cache_plugins/`, `vars_plugins/`, `inventory_plugins/`, `connection_plugins/`, `become_plugins/`, `callback_plugins/`, `strategy_plugins/`, `cliconf_plugins/`, `httpapi_plugins/`, `netconf_plugins/`, `terminal_plugins/`, `shell_plugins/`, `test_plugins/`, `doc_fragments/`) into `import-ansible/legacy/<type>/` for reference — **not** into `[exports]`. Each gets a TODO entry.
6. Generates `IMPORT_TODO.md`: Modules to port (Python files with stubbed interface), Filters to port, Other plugins, Jinja templates needing review (any using filters outside runsible's catalog — §2 of poor-decisions), `meta:` actions used (§17, these become first-class control-flow), `set_fact` calls (§4), `become` patterns (§16).

### 8.2 `import-ansible-collection`

Same machinery, with: `galaxy.yml` (or `MANIFEST.json`) as metadata source; collection-level deps → `[dependencies]`; `playbooks/` converted and exported; `roles/<name>/` as nested sub-packages with `--split-by-role` (otherwise merged); `meta/runtime.yml` `requires_ansible` → TODO comment; `plugin_routing` and `action_groups` in TODO with explicit note that runsible has no runtime redirect mechanism — the analog is `[imports]` lexical aliases per file (§22).

### 8.3 Honesty about what doesn't transfer

The TODO file is **prominent** — printed to stdout at end of import as a numbered checklist with file paths. We do not silently produce a "successful" import that won't work. We will not attempt a "best effort Python execution shim." Running CPython from a Rust controller is out of scope. If your value is in Python plugin code, runsible doesn't yet help you; the import tool says so plainly.

### 8.4 License preservation (mandatory)

Conversion produces a **derivative work**. Derivatives inherit the upstream license. The importer therefore enforces:

1. **License detection.** Before any conversion writes to disk, the importer scans for a license declaration in this priority order: `meta/main.yml`'s `license:` field → `galaxy.yml`'s `license:` / `license_file:` → top-level `LICENSE` / `LICENSE.md` / `LICENSE.txt` / `COPYING` → `MANIFEST.json`'s embedded license. The detected SPDX identifier (or `LicenseRef-Custom` for non-SPDX) is recorded.
2. **Refuse on missing / ambiguous.** No license found OR multiple conflicting declarations → import aborts with exit code 9 and a clear message. `--allow-missing-license` is the explicit override; the converted package's manifest then carries `license = "LicenseRef-Unknown"` and a loud warning is printed at every install of that package.
3. **Verbatim copy.** The original `LICENSE` / `COPYING` / `NOTICE` files are copied verbatim into the converted package's root. The runsible package manifest's `[package].license` field is set to the detected SPDX. The runsible package's `[package].imported_from`, `[package].imported_at`, and `[package].imported_license` fields are set unconditionally.
4. **Copyleft propagates.** GPL-2.0 / GPL-3.0 / AGPL inputs produce GPL-licensed runsible packages. The importer **does not relicense**. We do not "put runsible's Unlicense on" third-party content — that would be a license violation in every copyleft case and a notice violation in most permissive cases.
5. **Apache-2.0 NOTICE preservation.** Inputs licensed Apache-2.0 with a `NOTICE` file: NOTICE is copied verbatim AND the runsible package's `[package].notice_file = "NOTICE"` is set so install-time tooling can surface attribution.
6. **Registry surface.** The default registry shows the inherited license prominently on every imported-package page. `runsible-galaxy info <pkg>` prints the license + "imported from" provenance before any other field.
7. **Mixed-license collections.** A collection composed of files under different licenses (common: README under CC-BY, code under Apache-2.0) becomes a runsible package with the *most-restrictive* applicable license at the top level and per-file overrides recorded in `runsible.toml` `[[package.file_licenses]]` entries.

The seed registry (the launch corpus of imported Galaxy roles) is therefore a **mix of licenses, not all-Unlicense.** Most will be GPL-3.0 (Red Hat collections) or Apache-2.0 (community general). The runsible *registry code* (the server) and the runsible *workspace crates themselves* remain Unlicense. Only the *converted packages* carry their original licenses.

The only path to an all-Unlicense seed corpus is **clean-room rewrite**: re-implementing modules from documentation alone, never reading the source. That is a separate program of work outside `import-ansible-*`'s scope; if pursued, it lives in a new crate (`runsible-builtin-rewrite` or similar) and never invokes the converter.

---

## 9. Signing & verification

### 9.1 v1: detached PGP

Signatures are detached PGP/ASCII-armor over `<pkg>-<version>/.runsible/MANIFEST.toml`, mirroring Ansible's signature over `MANIFEST.json`. Same posture as §6 of poor-decisions (vault redesign, distinct problem): cryptographic provenance, not a password file.

Publish: `runsible-galaxy build`, then `gpg --detach-sign --armor` over the extracted MANIFEST.toml, then `runsible-galaxy publish --signature path/to/sig.asc` (repeatable; the registry stores and serves alongside the tarball).

Verify: `install` with `[signatures].required = true` (project-level) or `--required-signatures N` (CLI) requires N successful PGP verifications. Default keyring at `~/.runsible/keyrings/<registry-host>.kbx`; registry publishes `/v1/keys.asc` for first-use bootstrap. Local detached sigs via `--signature file://...` stack on top of registry-provided ones. Fails close when required; warns and proceeds when not.

### 9.2 v1 default: opt-in

In v1, signatures are **opt-in** at project level. Default `[signatures].required = false`. Requiring signatures from day one — before the registry has solved key distribution UX — would block adoption.

### 9.3 v1.5 default: required, with sigstore

In v1.5 we flip the default to required and add **sigstore/cosign** (`[signatures].sigstore = true`): OIDC-keyless signing; publish becomes `--sign sigstore` (no key management; signature bound to OIDC identity); verify checks transparency-log inclusion against `rekor.sigstore.dev` (or a configurable Rekor). PGP remains supported. Positions runsible-galaxy with a more modern provenance story than Ansible has shipped, without forcing it on early users.

### 9.4 Compliance hook (P3 persona)

For the compliance persona (P3 in `00-user-story-analysis.md`), `runsible-galaxy verify --audit-log PATH` emits an NDJSON audit record per package: name, version, source URL, checksum, signatures verified, signer key IDs, verifier identity, timestamp. Complements the signed-run-record story `runsible-pull` will ship.

---

## 10. Redesigns vs Ansible

Cited from `11-poor-decisions.md`:

- **§5 — One concept: package.** The biggest schema decision here. Roles, collections, standalone Galaxy artifacts: all flattened to `runsible-pkg`. The 19-plugin-type taxonomy collapses to a 5-export-type table. The importer is the only place Ansible's three-way taxonomy survives — to translate, not perpetuate.
- **§11 — SAT solver.** PubGrub replaces `ansible-galaxy`'s single-pass walker. Conflicts surface at solve time with a human-readable explanation, not at install time on a fresh CI box.
- **§15 — Lockfile.** `runsible.lock` mandatory. Ansible has no analog; one of the cheapest correctness wins in the project.
- **§22 — No FQCN shortening.** Packages referenced by `name@version` always — no `namespace.collection`, no implicit `collections:` search path. To shorten module references in a single playbook file, use a lexical `[imports]` block (`copy = "core_builtin.copy"`). Aliasing is per-file, parse-time. We **do not** reproduce Ansible's "sometimes propagates, sometimes doesn't" `collections:` bug (§13.16 of `05-collections-galaxy.md`).
- **§6 — Signed provenance.** Same posture as vault: cryptographic, per-recipient, default-on once it's not friction (v1 opt-in, v1.5 default-on with sigstore).
- **§14 — First-party tooling.** Ships from the same workspace as `runsible-playbook` and `runsible-lint`; shared parser; cannot drift in semantics.
- **Build determinism.** Ansible's tarballs aren't byte-deterministic (uid/gid/mtime drift). runsible's are.
- **Default registry inclusion.** Per §10 of `00-user-story-analysis.md`: "Galaxy/collections are political artifacts… build runsible-galaxy to read collections (one-way import) and to publish to a runsible-native registry." Done.

---

## 11. Milestones

**M0 — Local file:// registry.** Manifest parser + validator; on-disk layout reader/writer; tarball builder + extractor; PubGrub; `runsible.lock` r/w; CLI: `init`, `add`, `remove`, `install`, `update`, `list`, `info`, `build`, `verify`, `cache *`. "Registry" = file:// URL pointing at a directory of `.runsible-pkg` files plus a JSON index. No signing, no auth. Integration suite of ~20 toy packages including a deliberate conflict. Unblocks `runsible-playbook` development against real package shapes.

**M1 — HTTP registry, publishing, signing.** Registry HTTP client; reference registry (Axum + Postgres + S3-compatible blob; OAuth-style login; namespace reservation + yank admin scripts); `publish`, `login`, `logout`, `yank`, `unyank`; PGP signing + verification; hosted production deployment of `registry.runsible.dev`; token credentials.

**M2 — Ansible import + ecosystem seeding.** `import-ansible-role`, `import-ansible-collection`; seed corpus (top 50 Galaxy roles imported under `imported_*` prefix with "automated import; community-maintained ports welcome" notes + `IMPORT_TODO.md`); mirror policy + daily index dump endpoint; "porting an Ansible role" walkthrough.

**M3 — Sigstore + v1.5 polish.** Sigstore/cosign; flip default to required signatures; `verify --audit-log`; registry web UI (search + browse only); sub-100ms `install` cold start on a small project given a populated cache.

---

## 12. Dependencies on other crates

`runsible-galaxy` is structurally near the leaves of the workspace dependency graph: it doesn't depend on `runsible-playbook`, `runsible-vault`, `runsible-inventory`, etc. at runtime. The reverse is true: the playbook executor *consumes* packages installed by runsible-galaxy.

**Workspace deps:** `runsible-types` (shared `Version`, `VersionReq`, `PackageName`, `Checksum`, `RegistryRef`); `runsible-toml` (thin shim over `toml_edit`); `runsible-yaml2toml` (import commands only); `runsible-config` (reads `~/.runsible/config.toml`, `./.runsible/config.toml`).

**External:** `pubgrub` (solver), `semver`, `reqwest` (HTTP), `tokio` (async), `tar` + `zstd`, `blake3`, `sha2`, `sequoia-openpgp` (sig verify v1; `sigstore` joins in v1.5), `keyring` (libsecret/Keychain/Credential Manager), `clap` v4, `directories-next` (XDG), `tracing` (NDJSON when stdout is non-TTY, per §10 of poor-decisions).

**Anti-deps:** no Python interop (`pyo3`, `rust-cpython`); no YAML at runtime (`serde_yaml` only inside `runsible-yaml2toml`, which we depend on solely for imports); no `jsonschema` runtime — manifest validation is `serde::Deserialize` + custom validators.

---

## 13. Tests

**Unit:** manifest + lockfile parsers (every field, every error, `proptest` round-trips); resolver (synthetic graphs — linear, diamond solvable + hard-conflict, circular must error cleanly, pre-release exclusion, range edge cases, registry partitioning); tarball builder (determinism property — same source → byte-identical output across builds at different wall-clock times; test fixes `built_at` and asserts BLAKE3 equality); checksum verifier (tamper a file in an extracted package; verify catches); range parser (every Cargo form plus invalid forms).

**Integration:** `tests/synthetic-registry/` with ~30 toy `.runsible-pkg` files: happy-path; two diamond graphs (one solvable, one hard-conflict); a graph requiring 3+ levels of backtracking; circular dep (clean error); yanked dep; git dep (local file://-rooted git repo bundled in test data); package with corrupt FILES.toml (verify catches); manifest referencing a nonexistent `[exports]` file (build catches). Tests drive the CLI as a subprocess via `assert_cmd`/`predicates`, confirming success cases (correct exit + lockfile) and failure cases (correct message + deterministic exit code). An end-to-end "publish + install" against a locally-spawned reference registry runs under `--features integration-registry`.

**Snapshot:** PubGrub's explanation strings are part of the UX. `insta` snapshots the rendered conflict messages so a future PubGrub upgrade changing wording is noticed — the wording is documentation.

**Import-tool corpus (M2):** CI matrix imports the top 50 Galaxy roles and asserts each import succeeds (produces a buildable runsible package even if the runtime cannot execute everything). Protects against importer regressions as upstream roles evolve.

**Fuzz:** `cargo fuzz` targets for manifest parser, lockfile parser, tarball extractor (zstd + tar bombs), checksum verifier.

---

## 14. Risks

- **Network effect.** A package manager with no packages is useless. v0.9 launch must include a seeded registry: top 50 Galaxy roles via the import tool (with "automated port" notes) plus a hand-curated `core_*` set (`core_builtin`, `core_apt`, `core_yum`, `core_systemd`, `core_facts` — modules shipped with runsible-playbook itself, packaged as their own artifacts). Empty registry = vaporware regardless of code quality.
- **Backwards-compat honesty.** Importing a real Galaxy role frequently pulls in plugin code we cannot run. The import tool must emit a TODO list, not a fake "success" — otherwise users discover at runtime what should have been told them at import time. Reputation-damaging failure mode.
- **Hosting cost.** Bandwidth dominates; storage is cheap. Alpha is trivial; "Cargo of automation" scale is material. Mitigation: AGPL the registry source so the community can mirror; document mirror policy upfront; budget for hosting from launch; sponsor or commercial-license hosting in v2 if the project succeeds.
- **Squatting.** Flat namespace invites typo-squatting. Mitigation: at launch, reserve every Ansible namespace name (`community_general`, `ansible_builtin`) for the runsible org with a transparent claim process; pre-reserve common-noun packages (`apt`, `yum`, `service`) into the `core_*` series.
- **Registry abuse.** Spam/malware. Mitigation: rate-limit publishes per token; require verified email; auto-scan tarballs for shell scripts in unexpected locations and flag for review (don't auto-reject — false positives are worse UX than slow review).
- **Solver perf at scale.** PubGrub is good but pathological graphs exist. Mitigation: integration tests include the worst real-world conflict graphs we know (cribbed from `pip` issue trackers); hard 10s solver-time ceiling with a clear error; `--solver-timeout SECONDS` for CI.
- **Sigstore key-distribution edges.** v1.5 default depends on `rekor.sigstore.dev`. Air-gapped installs need a local Rekor; document and allow `[signatures].sigstore_url` override.

---

## 15. Open questions

1. **Default registry URL.** This doc uses `registry.runsible.dev` as a placeholder. Register `.dev`? `.org`? Or punt to community-run mirrors? **Strong opinion: host one ourselves at a single canonical URL.** Cargo's success owes a lot to crates.io being the unambiguous default; nix's flakes UX is murkier partly because there's no single registry.
2. **Signing required by default in v1?** This doc proposes "no, opt-in in v1; default-on in v1.5 with sigstore." Counter-argument: every day v1 ships unsigned packages is a day someone publishes a malicious package and we wear it. **Tentative: opt-in in v1.0, default-on in v1.1.** v1 launches small enough that a v1.1 gap is tolerable.
3. **Imported Galaxy roles as a separate "type"?** `runsible-pkg-ansible-imported` (distinct format flagged as best-effort) vs canonical `runsible-pkg`? **Opinion: no separate type.** A package is a package. Importer annotates `[package].imported_from = "https://galaxy.ansible.com/..."` and `[package].porting_status = "automated|reviewed|maintained"`; tooling reads metadata and warns on `automated` if user hasn't reviewed.
4. **Path deps in production manifests.** Path deps are dev-only; build refuses them. Ship `add --path`, or force `cargo workspace`-style multi-package via separate construct (`runsible-workspace.toml`)? **Tentative: ship `--path` for monorepo convenience but require `[workspace]` for multi-package builds.** The `[workspace]` design belongs in a separate plan.
5. **Yanking semantics around the lockfile.** Yanked version remains installable from existing lock (per crates.io). Should `verify-lock` warn on yanked entries? **Yes — non-fatal warning.** Compliance flows promote to error via `--warn-yanked-as-error`.
6. **Module-as-dynamic-library export.** `[exports].modules` lists module manifest TOMLs, but a module is implemented in Rust and shipped compiled. Where does the `.so/.dylib/.dll` get built — publish time (publisher cross-compiles) or install time (consumer compiles)? **Tentative: at publish time for `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`.** Publisher uploads multi-arch tarball; consumer selects matching arch at install. Authors who can't cross-compile use a CI workflow we provide. Meatier topic; deserves its own plan.
7. **Telemetry from the default registry.** Anonymous download counts let popular packages float in search. Ship `install` with versioned UA (`runsible-galaxy/1.0.0 (linux-x86_64)`) or no-op? **Yes to versioned UA; no to anything beyond UA without explicit opt-in.** Document in the privacy policy from day one.

---

End. The next plan applies these decisions to `runsible-playbook`, which consumes packages via a defined on-disk contract: `./.runsible/packages/<name>-<version>/` — runsible-playbook reads, runsible-galaxy writes.
