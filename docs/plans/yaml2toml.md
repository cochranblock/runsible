# runsible — `yaml2toml`

## 1. Mission

`yaml2toml` is the existential bridge crate. Every entry in `00-user-story-analysis.md` that talks about adoption — P1 platform engineers with 10k LOC of YAML, P2 MSPs with 30 tenants of legacy playbooks, P4 sysadmins with a homelab full of `geerlingguy.*` roles — runs through this crate's correctness on day one. The user-story analysis is explicit about this in §7 and §8 (R2): real-corpus correctness on the top 200 Galaxy roles is the second-highest project risk. If `yaml2toml` cannot one-shot, losslessly-where-possible, opinionatedly-where-not convert the published Galaxy long-tail to runsible-flavored TOML, the rest of the workspace ships into a vacuum. This crate is the only place in runsible where the *correct* posture toward Ansible is "be charitable, be conservative, be useful even when the input is bad" — every other crate gets to be opinionated. Here the only opinion that matters is *this YAML must come out the other side as TOML the user can review, commit, and run*, with every quirk surfaced not silently swallowed.

---

## 2. Scope

**In scope.**
- Conversion of the full Ansible YAML surface: playbooks, roles (`tasks/`, `handlers/`, `defaults/`, `vars/`, `meta/main.yml`, `meta/argument_specs.yml`), inventories (YAML form per §1.2 of `03-inventory.md`), `requirements.yml`, `galaxy.yml`, `meta/runtime.yml`, vars files (`group_vars/`, `host_vars/` in both file and directory form), and `ansible-vault`-encrypted variants of any of those.
- A two-pass model: a mechanical Pass 1 (structurally faithful) and an opinionated Pass 2 (applies the redesigns from `11-poor-decisions.md`).
- Best-effort comment preservation across both passes.
- A reverse `toml2yaml` shipped as a sibling binary, used internally for round-trip CI and externally by users in hybrid environments.
- A documented JSON conversion report consumed by CI, `runsible-lint`, and the manual-review surface.
- A corpus harness that fetches and converts the top 200 Galaxy roles and top 50 collections (excluding plugin-heavy ones) on every CI run.

**Out of scope.**
- Running converted content (that is `runsible-playbook`'s job).
- Translating Jinja2 expressions to a different syntax. `{{ x | default('y') }}` is preserved byte-identical inside TOML strings; filter compatibility with runsible's fixed catalog (per §2 of poor-decisions) is detected at lint time, not rewritten here.
- Semantic refactoring (no playbook-splitting, no variable renaming).
- Translating Python plugin code (`runsible-galaxy import-ansible-role` consumes us for the YAML half and produces an `IMPORT_TODO.md` for the Python half).
- Acting as a YAML linter; bad YAML errors out, well-formed-but-nonsensical YAML converts and is flagged in the report.

---

## 3. Conversion strategy

A two-pass model. The split is deliberate: it lets a paranoid user verify the mechanical translation before any opinionated rewrites happen, and it lets the opinionated pass evolve independently as runsible's redesigns settle.

### 3.1 Pass 1 — direct map ("mechanical")

Pass 1's contract: parse YAML, emit TOML, preserve structure 1:1 modulo the unavoidable scalar-quirk fixes (Norway, leading zeros, etc. per §4). The output is *not* runsible-idiomatic; it is YAML's shape rendered in TOML's syntax. Specifically:

- A YAML mapping with N keys becomes a TOML table with N keys.
- A YAML sequence becomes a TOML array (inline if small, expanded `[[array]]` if large — see §4.6).
- YAML anchors and aliases are *expanded inline*; TOML has no equivalent. The report records every expansion site.
- Multi-document YAML files emit a single TOML document with a `[[document]]` array unless `--multi-doc` is set.
- `!vault` and `!unsafe` tags translate to the TOML representations in §4.7 / §4.8.
- Comments attach to nearest-key positions per §5.

Pass 1 alone satisfies "I want to read this old playbook in TOML form to see what the original author meant." It is not enough to *run* the result through `runsible-playbook` without further hand-editing.

### 3.2 Pass 2 — runsible normalize ("opinionated")

Pass 2 takes the Pass 1 TOML AST as input and applies the following normalizations (each cited to `11-poor-decisions.md`):

- **Synthesize handler IDs (§13).** Every handler gets a stable snake_case ID derived from its `name`. `notify:` references everywhere are rewritten from name-strings to ID references. Collisions resolve via `_2`, `_3` suffixes.
- **Collect tags into [tags] enum (§19).** All tag strings used anywhere are gathered into a top-level `[tags]` table at the project root. Each tag becomes a key with an empty inline-table value: `release = {}`. CLI tag references downstream become schema-validated.
- **Rewrite `meta:` tasks into typed control-flow tasks (§17).** `meta: flush_handlers` becomes `{ type = "control", action = "flush_handlers" }`. Same for `end_play`, `end_host`, `end_batch`, `clear_facts`, `clear_host_errors`, `refresh_inventory`, `reset_connection`, `end_role`, `noop`.
- **Rewrite `set_fact` (§4).** Classified at convert time:
  - *Mutation pattern* (re-assigned later, or `cacheable: true`, or value depends on a previous `register`): emitted as `set_fact!` with a comment.
  - *Pure derivation* (assigned once, value derives from inputs available at play start): emitted as a `let` block at play level.
  - *Ambiguous*: defaults to `set_fact!` with a TODO; recorded in `manual_review_required`.
- **Split `serial:` + `order:` + `max_fail_percentage:` (§21)** into a typed `[plays.rollout]` sub-document. `order: shuffle` becomes `order = "shuffled-with-seed=<derived-seed>"` for reproducibility.
- **Rewrite `become_*` flat keywords (§16)** into a typed `[…].become` sub-document. `become_password` becomes a TODO comment recommending the keyring.
- **Rewrite `async:`/`poll:` (§24).** `async: <s>` with `poll > 0` becomes `[plays.tasks.async] timeout = "<s>s"`. `async: <s>` with `poll: 0` becomes `[plays.tasks.background]` with a TODO note.
- **Rewrite `with_*` loops to `loop` (§4.5 of `02-playbook-language.md`)** per the documented migration table. Unknown `with_<plugin>` is kept verbatim and flagged.
- **Synthesize `[imports]` block (§22).** Short module names rewrite to FQCN; a `[imports]` block at the file head maps short → FQCN. `collections:` is dropped (with a comment).
- **Collapse `include_*` / `import_*` to `compose` (§25)** with `dynamic = true|false` derived from the original keyword.
- **Drop `gather_facts: true` (§12).** Replaced with `[plays.facts] required = [...]` populated from a static analysis of the playbook's templates and `when:` clauses; falls back to `["all"]` with a refinement TODO when inference fails.
- **Inventory: collapse `host_vars/` and `group_vars/` resolution.** Pass 2 emits inventories with explicit references; conflicts become explicit precedence annotations (per §3 of `03-inventory.md`).
- **License detection (mandatory when converting third-party content).** Pass 2 scans the input tree for `LICENSE` / `LICENSE.md` / `LICENSE.txt` / `COPYING` and for license fields in `meta/main.yml` / `galaxy.yml` / `MANIFEST.json`. The detected SPDX identifier is written into the output's `runsible.toml` `[package].license` and the original license file is copied verbatim into the output root. **No license found or contradictory declarations: Pass 2 aborts with exit code 9 and refuses to write any output file.** `--allow-missing-license` is the explicit override and tags the output with `license = "LicenseRef-Unknown"`. Conversion **never relicenses**: GPL input → GPL output; Apache-2.0 input → Apache-2.0 output (with NOTICE preserved verbatim); MIT input → MIT output (with attribution preserved). The user's own playbooks (no upstream license — the user owns the code) bypass this check via `--my-content` or auto-detection of "input has no LICENSE because it's a personal project," which prompts for a license to apply or defaults to the user-configured `[package.defaults].license` from `runsible.toml`.

User picks via `--pass=1` (mechanical), `--pass=2` (default), or `--passes=1,2` (write both for diff review).

### 3.3 Why two passes

Three reasons: **auditability** (the Pass 1/Pass 2 diff answers "did the tool change semantics, or just shape?"); **independent evolution** (Pass 2 will accrete normalizations as redesigns sharpen, Pass 1's contract is permanent); **round-trip testing** (Pass 1 round-trips losslessly under `toml2yaml`, Pass 2 doesn't by construction).

---

## 4. Quirk handling

Each quirk has a default behavior, a `--strict` behavior (typically: error and refuse), and a documented entry in the report.

### 4.1 Norway problem

YAML 1.1 coerces unquoted `no`, `yes`, `on`, `off`, `y`, `n`, country codes, etc. **Default:** bare bool-trap values used in known-bool contexts (`become`, `gather_facts`, `ignore_errors`, etc.) convert to TOML bool; same values in known-string contexts convert to TOML string `"NO"` etc.; ISO 3166-1 alpha-2 country codes are always strings regardless of context. Every conversion emits a warning in the report. `--strict` errors on any case where context could plausibly be either.

### 4.2 Leading-zero strings

`port: 022` parses to int 22 in modern YAML. **Default:** `0[0-9]+` in known-string contexts (`mode:` on file/copy/template, inventory hostname segments) becomes quoted `"022"`; in known-int contexts becomes int with a "did you mean a string?" warning; unknown context defaults to string. `--strict` errors.

### 4.3 Unquoted colons

`description: A 24/7: monitor` may parse as a nested mapping. **Default:** values that parse as nested mappings but are clearly intended as strings (heuristic: inner key has no value, parent context expects string) re-emit as TOML quoted strings with a warning. `--strict` errors and asks the user to disambiguate by quoting.

### 4.4 Anchors and aliases

YAML's `&`/`*`/`<<:` have no TOML equivalent. **Default:** expand inline. The report records every expansion site with the size cost. `--strict` errors and asks the user to inline anchors in YAML before converting. (We do not invent a TOML reference syntax for v1.)

### 4.5 Multi-document YAML

**Default:** parse all docs, emit a single TOML document with a `[[document]]` array of length N. Each entry carries the converted document under a key matching its YAML root type. `--multi-doc` splits into N separate `.toml` files (`base.0.toml`, `base.1.toml`, …).

### 4.6 Inline vs expanded tables

TOML has both. **Default heuristic:** ≤ 3 top-level keys, ≤ 60-char serialized width, no nested tables, no string values longer than 40 chars → inline. Anything else → expanded. Arrays of tables (`[[plays]]`, `[[plays.tasks]]`) are always expanded. `--inline-style=always|smart|never` overrides.

### 4.7 The `!vault` tag

```yaml
db_password: !vault |
     $ANSIBLE_VAULT;1.2;AES256;prod
     62313365...
```

becomes

```toml
db_password = { vault = "v1.2", cipher = "AES256", label = "prod", body = """
62313365...
""" }
```

The encrypted body is preserved byte-identical; runsible-vault decrypts at runtime, exactly as Ansible does today (lazy decryption per §7 of `04-vault.md`). Whole-file vaults are detected pre-parse and emit a single TOML document with the vault metadata; runsible-vault is the consumer. We do **not** re-encrypt under runsible's age recipients here — that is `runsible-vault migrate-from-ansible`'s job. The interface boundary between this crate and runsible-vault is the inline TOML form above; that contract is set here.

### 4.8 The `!unsafe` tag

`!unsafe "literal {{ braces }}"` becomes `{ unsafe = true, value = "literal {{ braces }}" }`. The runsible templating engine reads this shape and skips templating.

### 4.9 Block scalars

YAML's `|`, `>`, `|-`, `|+`, `>-`, `>+`. **Default:** literal styles map to TOML `"""..."""` multi-line basic strings (or `'''...'''` literal-multiline when content has no specials, to preserve "what you see is what you get"); folded styles get folded per YAML rules then emitted as single-line or multi-line as appropriate. Chomping indicators are honored.

### 4.10 Floats that look like versions

`version: 1.10` parses as float `1.1` (trailing zero lost). **Default:** `\d+\.\d+(\.\d+)?` in any version-context (key contains "version", "release", "tag") becomes a TOML string preserving the original textual representation. Other floats stay floats. `--strict` errors on ambiguous cases.

### 4.11 Null, ~, and empty values

TOML has no null. **Default:** drop the key entirely (with a warning); an explicitly null inventory host (`web1.example.com:`) becomes `[hosts.web1_example_com]` (an empty TOML table).

### 4.12 Range patterns in inventory

`web[01:50].example.com` patterns are left as TOML strings for the runsible-inventory parser to handle at runtime. `--expand-ranges` materializes the 50 hosts at convert time.

### 4.13 Vault-encrypted whole files

Detected pre-parse via the `$ANSIBLE_VAULT;1.x;AES256` magic. Emit a TOML wrapper with vault metadata; the body is preserved verbatim. The report notes "the decrypted contents would also need conversion; rerun on the decrypted copy." `--decrypt-with <password-file>` recurses (decrypts, converts, re-encrypts under the same key); off by default since the round-trip is fragile.

---

## 5. Comment preservation

Best-effort. YAML comments don't have a stable parse-tree position; we use a CST-aware parser (`saphyr`, with `serde_yaml` as fallback) and attach each comment to the nearest TOML key, preferring line-precedence over column-precedence.

**Heuristic.** For a comment at line L: (1) find the nearest YAML node whose tokens span line L or an adjacent line — attach as preceding comment to the corresponding TOML key; (2) same-line trailing comments attach as TOML inline trailing comments; (3) gap-region comments preserve as standalone with surrounding blank lines; (4) unattachable comments orphan to the file head with a marker `# (yaml2toml: orphaned comment from line L of source)`.

**Output rendering** uses `toml_edit` (round-trippable). `--no-comments` skips extraction; `--strict-comments` errors on any unattachable comment.

**Honesty.** The report includes a `comments_lost` count and an orphan list. We document that we do not guarantee 100% comment fidelity; we guarantee that comments are never silently dropped without being recorded.

---

## 6. CLI surface

`yaml2toml` is the binary (also wired as a runsible subcommand). `toml2yaml` is the sibling binary for the reverse direction.

```
yaml2toml [OPTIONS] [PATHS...]
```

`PATHS...` defaults to the current directory; the tool walks recursively, picking up `*.yml`, `*.yaml`, `requirements.yml`, `meta/main.yml`, `meta/runtime.yml`, `meta/argument_specs.yml`, `galaxy.yml` (or whatever `--glob` provides).

| Flag | Purpose |
|---|---|
| `--in-place` | Rewrite each file as `<file>.toml` and delete the original on success — default. |
| `--out-dir <dir>` | Write outputs into `<dir>` preserving the relative tree. |
| `--pass=1\|2` | Mechanical or opinionated. Default `2`. |
| `--passes=1,2` | Emit both passes. |
| `--dry-run` | Print what would be converted; don't write. |
| `--multi-doc` | Split multi-document YAML into N TOML files. |
| `--no-comments` | Skip comment extraction. |
| `--strict` | Error on any quirk that requires a heuristic. |
| `--strict-comments` | Error on any comment that can't be attached. |
| `--report <file\|->` | Emit JSON report to file or stdout. |
| `--diff` | Show side-by-side YAML→TOML diff. |
| `--reverse` | Run `toml2yaml` instead. |
| `--decrypt-with <file>` | Vault password file for recursing into encrypted YAML. |
| `--inline-style=always\|smart\|never` | Inline-table heuristic override. Default `smart`. |
| `--expand-ranges` | Expand inventory `web[01:50]` patterns at convert time. |
| `--include <glob>`, `--exclude <glob>` | Repeatable glob filters. |
| `--threads <N>` | Parallel conversion. Default: CPU count. |
| `--keep-original` | Preserve the YAML next to the new TOML. |
| `--print-supported-quirks` | List every quirk this version handles, with examples. |

**Exit codes.** `0` success, `1` conversion failure, `2` argument error, `3` `--strict` triggered, `4` `--strict-comments` triggered.

**The `--in-place` safety net.** Writes `<original>.toml` first, runs a `toml::from_str` round-trip on the new file, *then* deletes the original. Failure leaves the source untouched and removes the partial TOML.

---

## 7. The conversion report

The report is the source of truth for what happened. Every CI gate, every `runsible-lint --post-convert` rule, every "did this lose anything?" question reads it.

### 7.1 Schema (v1)

```json
{
  "report_version": 1,
  "yaml2toml_version": "1.0.0",
  "started_at": "2026-04-26T14:00:00Z",
  "ended_at":   "2026-04-26T14:00:18Z",
  "input_files": 42,
  "output_files": 42,
  "pass_run": [1, 2],

  "warnings": [
    { "file": "group_vars/all.yml", "line": 12, "kind": "norway-problem",
      "before": "no", "after": "\"NO\"",
      "context": "value of country in vars block" },
    { "file": "site.yml", "kind": "anchor-expansion",
      "anchor": "common_vars", "expanded_at": [12, 47],
      "size_bytes_added": 320 }
  ],

  "synthesized": [
    { "file": "handlers/main.yml", "kind": "handler-id",
      "name": "Restart nginx", "id": "restart_nginx" },
    { "file": "handlers/main.yml", "kind": "handler-id-collision",
      "name": "Restart Nginx", "id": "restart_nginx_2",
      "collided_with": "restart_nginx" }
  ],

  "manual_review_required": [
    { "file": "playbook.yml", "line": 88, "reason": "complex Jinja in when clause",
      "snippet": "when: result is failed and (lookup('env', 'X') | bool)" },
    { "file": "tasks/install.yml", "line": 32, "reason": "set_fact ambiguous: classified as set_fact!" }
  ],

  "comments": { "preserved": 142, "orphaned": 4, "lost": 0,
                "orphan_locations": [...] },

  "filters_used": [
    { "name": "default", "uses": 47, "supported": true },
    { "name": "json_query", "uses": 3, "supported": false,
      "note": "from community.general; not in runsible's filter catalog" }
  ],

  "modules_referenced": [
    { "fqcn": "ansible.builtin.copy", "uses": 12, "imported_as": "copy" }
  ],

  "errors": []
}
```

### 7.2 Use cases

CI gates fail on non-empty `errors` or above-threshold `manual_review_required`. Pre-commit hooks summarize warnings before staging. `runsible-lint --filter-audit` reads `filters_used` to decide whether the converted project will run under runsible's fixed filter catalog. `runsible-galaxy import-ansible-role` ships the report alongside the imported package so registry consumers see "this imported role had N manual-review items."

### 7.3 Versioning

`report_version: 1` is the v1 contract. Breaking changes bump the integer; tooling reads both. Additive changes (new optional fields) do not bump.

---

## 8. Test corpora

### 8.1 Corpus list

Maintained in `tests/corpus/manifest.toml` as a versioned list of `(repository_url, commit_sha, expected_outcome)` tuples. Reviewed quarterly.

- **Top 200 Galaxy roles by download count**, excluding plugin-heavy (cisco/arista/junipernetworks/amazon.aws/google.cloud/azure tracked separately as future work). Sources: `geerlingguy.*`, `bertvv.*`, `dev-sec.*` (security hardening — high-value for P3), `MichaelRigart.*`, `nephelaiio.*`, etc.
- **Top 50 collections by download count**, excluding plugin-heavy: `ansible.posix`, `community.general` (only the YAML-shaped roles, not the Python plugins), `community.docker`, `community.crypto`, `containers.podman`, `devsec.hardening`.
- **Handcrafted regression suite**: `tests/quirks/*.yml` — one file per documented quirk in §4, snapshot-asserted via `insta`.
- **Pathological corpus**: `tests/pathological/*.yml` — files known to break naive conversion (anchors with self-reference, multi-doc inventories with merge keys, vault-inside-vault).

### 8.2 The harness

`yaml2toml-corpus-test`, gated on `--features corpus`:

1. Clones each repo to `~/.cache/runsible/yaml2toml-corpus/` at the pinned commit.
2. Runs `yaml2toml --pass=1 --report` on every YAML file.
3. Asserts: all conversions succeed; Pass 1 round-trips (`toml2yaml` produces semantically equivalent YAML, deep-equal via the documented equivalence relation in §9.1); per-repo warning counts within tolerated thresholds.
4. Re-runs `--pass=2`; captures the report as a snapshot. PRs that change Pass 2 output must explicitly accept the diff.
5. Emits an HTML dashboard at `target/yaml2toml-corpus-report.html` uploaded as a CI artifact.

### 8.3 CI integration

- **Per-PR**: against any change in this crate or its shared deps. ~5 minute runtime.
- **Nightly** against the full corpus on `main`. Failures auto-open issues.
- **Quarterly** the corpus list regenerates; a maintainer reviews/accepts.

A regression in any corpus repo is a PR-blocking failure. We accept the cost: this is the project's primary correctness signal. A separate, project-internal "canary" suite of 5-10 redacted real customer playbooks gates production releases against shapes the public corpus doesn't cover.

---

## 9. Reverse direction (`toml2yaml`)

Sibling binary (or `yaml2toml --reverse`).

### 9.1 What "lossless" means

`toml2yaml` is lossless within the **runsible TOML schema** — any TOML that runsible-playbook can run, `toml2yaml` can convert to YAML, modulo Pass 2 redesigns (which don't have YAML expressions). Round-trip: `yaml2toml --pass=1 → toml2yaml` produces semantically equivalent YAML, where "semantically equivalent" means: parsed by the same YAML parser version, yielding identical Python data structures (booleans, ints, strings, lists, dicts) with identical scalar types. Whitespace, key order, and comments may differ. We do not promise byte-for-byte equivalence.

### 9.2 Pass 2 reverse direction

`toml2yaml` of Pass 2 output is **not** lossless to original Ansible YAML by construction — Pass 2 changed the schema. The tool warns "this YAML uses runsible-specific schema and will not run under Ansible." `--target=ansible` attempts to downgrade where possible (typed handlers re-emit as named handlers, etc.); `--target=runsible` (default) preserves the Pass 2 schema in YAML form for hybrid environments.

### 9.3 Why ship it

Three reasons: round-trip CI testing (the corpus harness depends on it); hybrid-environment users (a team migrating may keep CI on Ansible for a quarter); documentation (side-by-side examples). Engineering cost is ~30% of `yaml2toml` proper.

---

## 10. Redesigns vs Ansible

This crate is the *only* place in runsible that is not opinionated by default — Pass 1 explicitly mirrors Ansible's structure. Pass 2 applies the redesigns from `11-poor-decisions.md`. Cross-references for navigation: §1 (YAML→TOML — the entire crate's existence justification), §4 (`set_fact`), §12 (lazy facts), §13 (handler IDs), §16 (become), §17 (`meta:` actions), §19 (tags), §21 (rollout), §22 (`[imports]`), §24 (async), §25 (`compose`).

`yaml2toml` is opinionated by default (Pass 2); users wanting a mechanical translation request Pass 1.

---

## 11. Milestones

### M0 — Pass 1 conversion of core file types

Pass 1 working for: playbooks, vars files, inventories (YAML), `requirements.yml`, `galaxy.yml`, `meta/*.yml`. All quirks in §4 with documented defaults; `--strict` functional. CLI complete except `--diff` and `--decrypt-with`. Conversion report v1 schema. Comment preservation at 80%+ on the handcrafted suite. Demo target: convert `geerlingguy.nginx` end-to-end.

### M1 — Pass 2 normalization

All Pass 2 normalizations from §3.2 implemented. `--diff` working. `toml2yaml` works on Pass 1 outputs (round-trip CI green). Pass 2 output validates against `runsible-core`'s TOML schema. Milestone where `yaml2toml --pass=2` produces output that `runsible-playbook` can actually execute.

### M2 — Corpus harness in CI

`yaml2toml-corpus-test` against the top 50 Galaxy roles. HTML dashboard. `toml2yaml` works on Pass 2 output for both `--target=runsible` and (where downgradeable) `--target=ansible`. Manual-review reporter integrated into `runsible-galaxy import-ansible-role`'s output.

### M3 — Top-200 corpus, vault recursion, polish

Full top-200 in CI. `--decrypt-with` working with safety rails. Comment preservation at 95%+ on the corpus. Performance: 50-file role <500ms; full `geerlingguy.docker` <2s. Documentation: a "porting your first Ansible playbook" walkthrough.

### M4 — Hardening and v1 release

`cargo fuzz` of the YAML parser and TOML emitter. `proptest` round-trip tests. Long-tail corpus edge cases addressed. v1.0.0 semver-stable from this point.

---

## 12. Dependencies on other crates

`yaml2toml` is intentionally **standalone** at runtime — `cargo install yaml2toml` yields a working binary that does not pull in the rest of the workspace.

**Optional workspace dep (compile-time, gated):**
- `runsible-core` (under `--features schema-validate`) — provides the TOML schema so Pass 2 output is parseable. Recommended on for CI; off for the standalone binary.

**External crate deps:**
- `saphyr` (or `serde_yaml`) — YAML parser. Default `saphyr` for CST-aware comment preservation.
- `toml_edit` — TOML emitter that round-trips comments.
- `clap` v4 — CLI parsing.
- `serde`, `serde_json` — for the report.
- `similar` — for `--diff`.
- `walkdir`, `globset` — directory walking.
- `rayon` — parallel conversion across files.
- `tracing` — structured logs.
- `blake3` — for shuffle-seed derivation.
- `regex` — for quirk-detection patterns.

**Anti-deps:** no Python interop, no HTTP client at runtime (corpus harness uses git via subprocess), no database.

---

## 13. Tests

- **Quirk unit tests.** Each quirk in §4 has at least three tests: canonical case, edge case, `--strict` failure. Snapshot via `insta`.
- **Comment-attachment tests.** A grid of source positions × node types verifying §5.
- **Snapshot tests** on representative real playbooks: simple play with become/notify; role with defaults+vars+tasks+handlers+meta+deps; inventory with nested groups + range expansion + host vars + vault; `requirements.yml` with collections+roles+git+sigs; playbook full of `meta:` actions and `set_fact`s; vault-encrypted whole-file vars file.
- **Round-trip property tests** via `proptest`: generate arbitrary Ansible-shaped data → Pass 1 → `toml2yaml` → assert deep-equal.
- **Corpus tests** per §8.
- **Fuzz** targets via `cargo fuzz`: YAML parser surface, TOML emitter, vault detector, comment-attachment heuristic. Nightly with 30-min budget per target.
- **Performance benchmarks** via `cargo bench`: single-file, 1000-task playbook, whole-role (`geerlingguy.docker`), peak RSS. Regressions >20% are PR-blocking.

---

## 14. Risks

- **Real-world content is dirty.** Corner cases will surface forever. The corpus harness is the only honest defense; maintenance cost is permanent.
- **Comment preservation is fragile.** Users will complain when a comment moves to the "wrong" key. We document the heuristic, ship the orphan marker, provide `--strict-comments`, and do not promise 100%.
- **Jinja with embedded YAML quirks.** `{{ "no" | bool }}` should NOT be touched (it's inside Jinja; Norway doesn't apply). The quirk detector respects Jinja boundaries: scan only YAML *outside* `{{ ... }}` and `{% ... %}` regions. We test this explicitly.
- **Reputation: `geerlingguy.docker`.** The single most-downloaded role on Galaxy. If we produce broken output for it, screenshots spread and runsible is dismissed. Permanent zero-tolerance corpus target.
- **Vault interaction edge cases.** Whole-file vault containing YAML that itself contains `!vault` tagged variables under different keys. Conversion preserves both layers; runsible-vault decrypts at runtime.
- **Inventory-relative-vs-playbook-relative collapse.** Per §3 of `03-inventory.md`, runsible drops this distinction. Pass 2 emits explicit references; conflicts go in the report. Deeply-layered overrides require non-trivial review.
- **Multi-doc YAML in unexpected places.** Some users put `---` separators in old vars files. Default `[[document]]` array merge may surprise; the report flags every detection.
- **Pathological huge files.** A 100k-line corporate inventory. Parallel-across-files handles this; for single pathological files we ship `--max-file-size` (default 50 MB) with a clear error.

---

## 15. Open questions

1. **Should `yaml2toml` ALSO produce a `runsible.toml` manifest scaffold for converted roles?** Tentative: yes, with `--scaffold-manifest` (default off). Reduces "now I have to write a manifest from scratch" friction; ties yaml2toml more tightly to `runsible-galaxy`'s import flow.
2. **Should `--strict` be on by default for CI?** Tentative: ship `--mode=lenient` (default) and `--mode=strict` (alias for `--strict --strict-comments`); recommend strict for CI in docs but don't make it the default.
3. **Round-trip equivalence: semantic vs byte-for-byte.** Semantic is the right choice. Define the equivalence relation in a documented Rust trait (`YamlSemanticEqual`) used by round-trip tests; iterate as edge cases surface. No byte-for-byte guarantee.
4. **`yaml2toml --watch` mode** for incremental migration? Not in v1; revisit in v1.5 if user demand surfaces.
5. **Vault recursion: warning-only mode?** Tentative: yes, behind `--decrypt-with --emit-plaintext`; output gets a `.PLAINTEXT.toml` suffix as a screaming reminder; the report records paths so the user knows what to clean up.
6. **Public per-role compatibility scores?** Counter-argument: maintenance pressure plus perverse incentive to gamify warnings. Tentative: ship internally; do not publish until v1.5+ when conversion logic stabilizes.
7. **Should `toml2yaml` round-trip Ansible's vault as `!vault` from a runsible-native age entry?** Tentative: no — emit a clear error explaining the vault format is incompatible. One-way migration on the vault axis even when playbook-content is round-trippable.

---

End. The next plan likely tackles `runsible-vault`, which depends on `yaml2toml`'s `!vault` preservation contract (§4.7) being stable so it can decrypt the preserved bodies at runtime. The interface boundary is the inline TOML form `{ vault = "v1.x", cipher = "AES256", label = "...", body = "..." }` — that contract is set here and consumed there.
