# runsible — `runsible-doc`

> Per-crate plan. Sibling to `runsible-playbook`, `runsible-galaxy`, `runsible-lint`. Drafted against research docs `00`, `01` §6, `06`, `11` §20, `08`.
>
> Frame: `ansible-doc` is a Python introspection tool — it cracks open module source, parses triple-quoted YAML doc strings, prints them. Wrong model for runsible. Replace with TOML sibling files and design a binary that's fast to start, opinionated, pleasant to browse.

---

## 1. Mission

`runsible-doc` surfaces module, handler, filter, and test documentation — fast, format-agnostic, no source parsing. It reads `*.doc.toml` sibling files bundled with the modules they describe; never imports a module binary or scrapes its source. Its job: answer four questions in under 50 ms each — *what does this module do?*, *what parameters does it take?*, *what does a paste-ready snippet look like?*, *where does it live on disk?* A `serve` mode exposes the same answers as a local HTML+search browser. The binary stays small because the validation logic is a shared crate that `runsible-lint` and `runsible-galaxy` consume too.

---

## 2. Scope

**In (v1):** read `*.doc.toml` from documented search paths; subcommands `list`, `show`, `snippet`, `search`, `serve`, `import-ansible`, `lint`; render to text, JSON, markdown, HTML; lazy-built tantivy search index; one-shot `import-ansible` for converting Python `DOCUMENTATION`/`EXAMPLES`/`RETURN` blocks; CI lint via shared schema crate.

**Out (v1, possibly forever):** parsing Python source at runtime (only one-shot in `import-ansible`); installing/removing modules (`runsible-galaxy`); authoring new modules from a template (future `runsible-package init`); keyword reference for `runsible-config`/`runsible-inventory`/`runsible-playbook` (those crates emit their own keyword docs from the parser — they cannot drift from the runtime); a web-published doc site (`runsible-galaxy registry` + static-site generator on top of our render path).

The narrow scope is deliberate. `ansible-doc` accumulated 17 plugin types (`become`, `cache`, `callback`, `cliconf`, `connection`, `httpapi`, `inventory`, `lookup`, `netconf`, `shell`, `vars`, `module`, `strategy`, `test`, `filter`, `role`, `keyword`) because Ansible's plugin axes are themselves sprawling. runsible has fewer axes — modules, handlers, filters, tests, packages. Plugin types we don't expose in the runtime, we don't expose in the docs.

---

## 3. The `.doc.toml` schema

Every documented item — a module, a handler type, a filter, a test — has a sibling TOML file co-located with the source. For a module `runsible_builtin/copy.rs`, the doc is `runsible_builtin/copy.doc.toml`. For a filter `runsible_builtin/filters/regex_replace.rs`, the doc is `runsible_builtin/filters/regex_replace.doc.toml`. Sibling layout means a package's `docs/` directory is generated from the package tree, not authored separately.

### 3.1 Canonical example — `copy.doc.toml`

```toml
# Hand-maintained; lints against the runsible-doc schema in CI.
schema_version = 1
name = "copy"
package = "runsible_builtin"
fqn = "runsible_builtin.copy"
kind = "module"                       # one of: module, handler, filter, test
version_added = "0.1.0"
maintainers = ["runsible-core"]
license = "Unlicense"

synopsis = "Copy a file (or directory tree) from controller to managed node."

description = """
For variable interpolation use `template` instead. For copying *from* the
managed node back to the controller use `fetch`. The recursive copy facility
does not scale efficiently for hundreds of files or more — archive on the
controller, transfer once, then `unarchive` on the remote.
"""

requirements = ["controller: read source", "remote: writable dest dir"]

# Each parameter is a typed entry; the schema enforces shape.
[[parameters]]
name = "dest"
type = "path"
required = true
description = "Path on the managed node where the file will be written."

[[parameters]]
name = "src"
type = "path"
description = "Source on controller. Mutually exclusive with `content`."

[[parameters]]
name = "content"
type = "string"
description = "Inline file content. Mutually exclusive with `src`."

[[parameters]]
name = "mode"
type = "file_mode"
default = "preserve"
description = "Octal ('0644'), symbolic ('u+rwx'), or 'preserve'."

[[parameters]]
name = "validate"
type = "string"
description = "Pre-promotion validation command with `%s` placeholder. Example: 'visudo -cf %s'."

# Constraints are first-class, not in prose.
[[mutually_exclusive]]
fields = ["src", "content"]
[[required_one_of]]
fields = ["src", "content"]

# Examples are paste-able TOML snippets; the linter parses each body.
[[examples]]
name = "Drop a config with validation"
why = "Most-common idempotent file placement."
toml = '''
[[plays.tasks]]
name = "Place sshd_config"
copy = { src = "files/sshd_config", dest = "/etc/ssh/sshd_config", mode = "0644", validate = "/usr/sbin/sshd -t -f %s" }
'''

# Returns typed; runsible-lint uses them to validate `register:` slots.
[[returns]]
name = "checksum"
type = "string"
description = "SHA256 of the destination file post-write."

[[see_also]]
fqn = "runsible_builtin.template"
why = "Use `template` when content needs variable interpolation."

[notes]
check_mode = "full"
diff = "full"
idempotent = true
connection = "any"
become = "respects"
```

### 3.2 The same module, the Ansible way

Ansible puts this inside `library/copy.py` — three triple-quoted YAML blobs (`DOCUMENTATION`/`EXAMPLES`/`RETURN`) embedded in Python source. The differences vs. our `.doc.toml` are not cosmetic:

- **Python boot to read.** The doc is locked behind an interpreter import. Ours is `cat`-able.
- **Two parser layers.** YAML inside a Python string. We have one parser.
- **Cannot lint or version independently of the module binary.** Editing a doc string means touching `.py` source. Ours is a separate file with its own version.
- **No schema enforcement.** Typos in `description` (`descrption`) survive silently until the renderer chokes. `runsible-doc lint` rejects unknown fields.
- **Mixes prose, schema, examples in three opaque blobs.** Ours has typed `[[parameters]]`, `[[examples]]`, `[[returns]]` sections consumed by the renderer, the linter, and the playbook static-checker from one source.
- **Examples are typed.** `name`, `why`, parseable `toml` body — the renderer validates every example by attempting to parse it.

---

## 4. CLI surface

Six subcommands plus `lint`. Every flag is listed; nothing is left implicit.

### 4.1 `runsible-doc list [-t TYPE] [package]`

Lists documented items, optionally filtered. Flags: `-t module|handler|filter|test|all` (default `module`); `-p NAME` or positional `package`; `-f text|json|names`; `--installed-only`; `--paths` (replaces Ansible's `-F/--list_files`). Subsumes Ansible's `-l/-F/--list_files/--metadata-dump`.

### 4.2 `runsible-doc show <fqn> [--format ...]`

Renders one item. Name must be `package.item` (e.g., `runsible_builtin.copy`); short forms only resolve via a project-local `[imports]` block. Flags: `-f text|json|markdown|html`; `--no-color`; `--no-pager`; `--examples-only`; `--params-only`; `--version VERSION` (default: `runsible.lock`'s pin or latest installed). Drops Ansible's `-j` (use `-f json`) and `-e/--entry-point` (no multi-entrypoint roles — packages are the unit of reuse, §22 of poor-decisions). `-s/--snippet` is promoted to its own subcommand (4.3) for discoverability.

### 4.3 `runsible-doc snippet <fqn>`

Paste-ready TOML task with placeholder values for required params, defaults for the rest. The `ansible-doc -s` analog, TOML-shaped. Flags: `-f toml|json` (no YAML — producing YAML from a TOML-native tool invites users back into the YAML quagmire, §1 of poor-decisions); `--with-comments`; `--include-optional`.

```
$ runsible-doc snippet runsible_builtin.copy
[[plays.tasks]]
name = "TODO: name this task"
copy = { src = "TODO", dest = "TODO" }
```

### 4.4 `runsible-doc search <query>`

Full-text across all installed docs. Index lazy-built and cached at `~/.runsible/index/` (TTL 24h, invalidated when `runsible-galaxy` writes a marker after install/upgrade). Flags: `-t TYPE`; `-p PKG`; `-n MAX` (default 20); `-f text|json`; `--field synopsis|description|parameters|examples`. tantivy BM25 ranks with synopsis 4x over description, 8x over example body.

### 4.5 `runsible-doc serve [--port 8080] [--bind 127.0.0.1]`

Local HTTP server. Loopback default; not multi-tenant, no auth — production goes behind `runsible-galaxy registry` + reverse proxy. Flags: `--port`; `--bind` (refuses `0.0.0.0` without `--allow-public`); `--no-search`; `--watch` (re-scan per request); `--theme default|dark|print`.

Route table, intentionally thin: `GET /` (package list); `GET /pkg/<pkg>/` (items); `GET /pkg/<pkg>/<item>/` (HTML); `GET /pkg/<pkg>/<item>/snippet` (`text/plain`); `GET /search?q=...` (JSON, client JS renders); `GET /api/list`; `GET /healthz`.

### 4.6 `runsible-doc import-ansible <python_module_path>`

One-shot conversion. Walks the tree, extracts `DOCUMENTATION`/`EXAMPLES`/`RETURN` blocks, writes sibling `.doc.toml`. Python parser is feature-gated (`feature = "import-ansible"`). Flags: `--out DIR`; `--package NAME` (else inferred); `--strict` (fail on unmappable fields vs. emit `# TODO`); `--examples-as-yaml-comment` (preserve original YAML for review); `--map FILE` (renames, e.g., `ansible.builtin.yum -> runsible_builtin.dnf`); `--fragment-path DIR` (resolve `extends_documentation_fragment`). This is the one place we accept Python source.

### 4.7 `runsible-doc lint <doc_file>`

Validates against the published schema. Calls the shared `runsible-doc-schema` crate that `runsible-lint` also uses. Flags: `--strict` (non-zero on warnings, the CI default); `-f text|json|sarif`; `--fix` (rewrite idiomatic field order, drop unambiguous duplicates); `--schema-version N` (back-compat checks); `--check-source <source>` (compare against synthesized `.params.toml` from `Module::Input` — see §6.5).

### 4.8 Dropped vs. renamed flags from `ansible-doc`

| Ansible flag | runsible disposition |
|---|---|
| `-t module/lookup/.../keyword` | Reduced to `module/handler/filter/test`. Unsupported plugin axes don't appear. |
| `-l/--list` | Replaced by the `list` subcommand. |
| `-F/--list_files` | Replaced by `list --paths`. The only underscore-named flag in Ansible's CLI is gone. |
| `--metadata-dump` | Replaced by `list -t all -f json`. The "internal, unstable" footnote dies with it. |
| `-s/--snippet` | Promoted to a subcommand for discoverability. |
| `-j/--json` | Replaced by `--format json` everywhere. |
| `-e/--entry-point` | Removed; runsible has one entry point per package. |
| `-r/--roles-path`, `-M/--module-path` | Replaced by `RUNSIBLE_DOC_PATH` plus §5 discovery. |
| `--playbook-dir` | Removed; cwd is enough, override via `RUNSIBLE_DOC_PATH`. |
| `--no-fail-on-errors` | Replaced by `lint -f sarif`, the only production "soft errors" user. |

---

## 5. Doc discovery

`.doc.toml` files are resolved in this order, first match wins per `(package, item)`:

1. **`RUNSIBLE_DOC_PATH`.** TOML escaping, not bash escaping (see §25 of poor-decisions on `ANSIBLE_LIBRARY` quoting). The env var holds a TOML inline-array literal: `RUNSIBLE_DOC_PATH='["/opt/site-docs", "./vendor/docs"]'`. If the value doesn't parse as a TOML array, fall back to OS path separator split (`:` Unix, `;` Windows) with a deprecation warning.
2. **`./packages/<ns>/<pkg>/docs/`** — project-local. Project root detected by walking up for `runsible.toml`.
3. **`~/.runsible/packages/<ns>/<pkg>/<version>/docs/`** — user-installed, version-pinned. Version resolved from `runsible.lock` if present, else "latest installed."
4. **`/usr/share/runsible/packages/<ns>/<pkg>/docs/`** — system-wide.
5. **Built-in package, embedded at compile time** via `include_dir!`. `runsible_builtin.*` always available; no install. Build script embeds an `.index` file so `list` is constant-time.

No implicit "Python sys.path" search. Ansible inherits doc-loading from anywhere on `sys.path`; we don't have one. Dump resolved paths with `runsible-doc list --paths --debug`.

---

## 6. Redesigns vs Ansible

§20 of `11-poor-decisions.md` calls out the central misdesign: doc is parsed from Python source at runtime — slow, fragile (one bad doc string fails the listing), Python-only. Every redesign here cascades from fixing that.

- **TOML sibling files, not embedded YAML strings.** A directory walk plus N file reads — single-digit milliseconds for the ~70 built-ins. `ansible-doc -l` measured in seconds is the baseline; two orders of magnitude faster makes `runsible-doc` a thing users open instead of bookmark.
- **Doc files version with the module.** `runsible-galaxy install runsible_builtin@0.3.2` lands the docs at `0.3.2`. `runsible-doc show` resolves to the installed version, not "latest known to docs.runsible.example." This kills the recurring Ansible footgun where bookmarks on docs.ansible.com lie about parameters that don't exist yet (or were renamed).
- **Lint catches missing fields.** Required: `name`, `package`, `kind`, `synopsis`, `version_added`. `parameters` entries must have `type` and `description`. `examples` entries must have `name`, `why`, and a `toml` body that itself parses. Lint runs alongside `cargo test`.
- **`runsible-lint` and `runsible-doc` share the schema.** `runsible-doc-schema` is a leaf crate (deps: `serde`, `toml` only). Both binaries consume it. `runsible-lint` checks that a playbook task's argument set matches the documented module signature — same parser, same types. This is what `rust-analyzer` does: doc and types are one thing. We avoid Ansible's split where `ansible-lint` is a separate project with its own parser (§14 of poor-decisions).
- **Source-vs-doc drift detection.** For runsible-native modules, the build step emits a synthesized `.params.toml` from `Module::Input`. `runsible-doc lint --check-source` fails if doc params don't match the type. For imported modules (`import-ansible`), synth doesn't exist; the lint warns explicitly. **Pick one source per module:** fully hand-maintained, or fully `derive(Doc)`-generated. Never both. That dual mode is how Ansible's `validate-modules` got into trouble.
- **No virtual "keyword" plugin type.** Ansible's `ansible-doc -t keyword loop` documents the playbook DSL. We don't. `runsible-playbook --explain-keyword loop` prints from the parser's own keyword table. Keyword semantics belong to the language definition, not authored doc files. Same principle as §17 on `meta` becoming first-class control-flow tasks.

---

## 7. The `serve` mode

axum HTTP server. Narrow motivation: browse a package's docs without remembering every FQN; tab-completion is one solution, a 200ms-to-first-paint HTML browser is the other.

**Stack:** axum + askama (compile-time templates) + tantivy (lazy-built, persistent at `~/.runsible/index/`, mtime-invalidated). One CSS, one JS, embedded with `include_str!`. No Node, no bundler. `--theme` flips CSS custom properties.

**What it doesn't do:** no accounts (loopback default), no editing (changes go via `git`), no websocket live reload (`--watch` + refresh), no federation across registries.

**Versioning in the UI:** package list shows installed + latest available (from `runsible-galaxy`'s cached registry index when present). Per-item pages get a version selector when multiple versions are on disk; default is `runsible.lock`'s pin. URL embeds version: `/pkg/runsible_builtin/0.3.2/copy/` for stable links.

**Performance budget:** cold start under 300 ms; warm start under 100 ms; item-page render under 50 ms; search over 200 docs under 30 ms. Miss any by 2x and the feature doesn't ship. The budgets are how we keep `serve` from accreting features.

---

## 8. Milestones

- **M0 — `list` + `show` + `snippet` against a local package tree.** Schema crate published. Built-in docs hand-authored for the v1 module set (`command`, `shell`, `copy`, `template`, `file`, `package`, `service`, `debug`, `set_fact`, `assert`). `lint` validates against the schema. Smoke test: `show runsible_builtin.copy` round-trips through text, json, markdown.
- **M1 — `serve` + `search`.** axum + askama + tantivy per §7. HTML snapshot tests, one per built-in. Performance gates from §7's budget enforced in CI.
- **M2 — `import-ansible`.** `rustpython-parser` for the AST, `serde_yaml` for the embedded YAML blobs. Round-trip tests over the `ansible.builtin` module set: install ansible-core in CI fixture, import all modules, lint every emitted file, gate on a per-module pass rate (publish the rate as a badge). Maintain `import-mapping.toml` for module renames and parameter-name collisions.
- **M3 (post-v1) — Source-vs-doc drift checks.** `derive(Doc)` macro lands on `Module::Input`; build emits a synthesized `.params.toml` shadow file (in `target/`, not committed). `runsible-doc lint --check-source` and `runsible-lint` both consume it. Drift surfaces for runsible-native modules (and explicitly *doesn't* for imported ones — the warn is documentation enough).

---

## 9. Dependencies on other crates

- **Reads from:** the on-disk layout `runsible-galaxy install` produces (path constants live in `runsible-package-layout`); `runsible.toml` and `runsible.lock` for project root detection and version pinning (schema in `runsible-config`).
- **Shares with:** `runsible-doc-schema` — consumed by `runsible-lint` and by `runsible-galaxy` (the latter to validate docs at publish time). Leaf crate, no deps but `serde` and `toml`.
- **Does not depend on:** `runsible-playbook`, `runsible-vault`, `runsible-pull`, `runsible` (ad-hoc). `runsible-doc` is a static analyzer over a file tree — never executes a play, opens a connection, decrypts a vault. That isolation is what lets us hit the cold-start budget. We resolve installed-package paths via `runsible-package-layout` constants, not by shelling out to `runsible-galaxy`.

---

## 10. Tests

- **Schema validation.** Golden corpus at `tests/fixtures/docs/` (the built-ins): load → validate → re-serialize, the round-trip must equal the original. Negative corpus at `tests/fixtures/bad-docs/`: one violation per file (missing required field, wrong type, malformed example body), each producing a specific lint error code.
- **Round-trip `import-ansible` on `ansible.builtin`.** CI installs ansible-core in a fixture container, imports `lib/ansible/modules/`, lints every emitted file. Per-module pass rate recorded; regressions fail the build; rate published as a badge. We never promise 100% — some Ansible docs are themselves malformed and we won't paper over that. Diff harness compares imported doc against hand-authored `runsible_builtin` doc for the same module; reports drift candidates.
- **HTML render snapshot tests.** One snapshot per built-in module via the `serve` render path, in `tests/snapshots/html/`. Regenerated with `cargo test --features regen-snapshots`. CI fails on any unintended diff.
- **Performance.** `criterion` benchmarks for `list`, `show`, `snippet`. `serve` cold and warm start measured via subprocess wall clock (not in-process — want to catch binary-size regressions). Budgets from §7 encoded as `assert!` at the end of each bench.
- **Search relevance.** A 30-pair `(query, expected_top_result)` corpus across the built-ins. Harness reports hit rate at top-1 and top-5; CI gates on a minimum.

---

## 11. Risks

- **R1 — Doc-source drift.** Hand-maintained `.doc.toml` can lag actual module parameters. Mitigation: `--check-source` lint for runsible-native modules; `runsible-lint` warns at playbook lint time when a task uses a parameter the doc omits. Policy (§6.5): pick one source per module and stick to it — either fully hand-maintained (lint enforces sync) or fully `derive(Doc)`-generated. Never both. That dual mode is how Ansible's `validate-modules` got into trouble.
- **R2 — Search index size.** A 1000-doc tree could push tantivy index to tens of MB. Mitigation: lazy build on `serve` start, persistent cache in `~/.runsible/index/`, body truncated past 64 KB per doc (anyone with longer docs has a different problem).
- **R3 — `serve` UX over-engineering.** Easy to spend a quarter on TUI, websocket reload, accounts, version diffs. We won't. The route table in §7.2 is the contract; new routes need written justification. One theme is tested exhaustively. Power users get `runsible-doc list -f json` and a static-site generator of their own.
- **R4 — `import-ansible` on weird Ansible docs.** Some modules use `extends_documentation_fragment` to compose docs across files. Mitigation: `--fragment-path DIR`; unresolved fragments become `# TODO` comments unless `--strict`.
- **R5 — `RUNSIBLE_DOC_PATH` TOML-array form is unfamiliar.** Muscle memory says `=/foo:/bar`. We accept colon-list fallback with a deprecation warning so first use never hard-fails. Re-evaluate removal after two release cycles.
- **R6 — Built-in docs binary bloat.** Embedding ~70 module docs adds 200-400 KB to the binary. Acceptable. Past 500 modules we'd gate behind a `built-in-docs` feature; v1 won't get there.

---

## 12. Open questions

Calls to make before M0 closes.

- **Q1 — Single `.doc.toml` per module, or `<module>.doc.d/` directory of section files?** Lean: single file. Easier to find, lints as a unit, one mtime. Re-evaluate if any built-in doc exceeds 800 lines (e.g., a future `user` module with OS-specific param sections).
- **Q2 — Embed markdown in TOML strings, or reference a sibling `.md` by path?** Lean: TOML-embedded by default; allow `description_path = "copy.description.md"` as escape hatch. Lint warns when an embedded description exceeds 200 lines.
- **Q3 — Auto-generate `.doc.toml` from a `derive(Doc)` macro on `Module::Input`?** Lean: lint-only. Macro emits a `.params.toml` shadow file in `target/` (not committed); `--check-source` compares. Hand-authored `.doc.toml` stays the single source of truth in `git`.
- **Q4 — Live reload in `serve` without restart?** No. `--watch` exists for dev; otherwise restart is cheap and avoids competing with index-rebuild.
- **Q5 — Sub-binaries?** No. Single `runsible-doc` binary with subcommands. The umbrella `runsible doc ...` short-form is dispatched by the meta-binary, per the master plan's "single binary from the user's POV" goal.

