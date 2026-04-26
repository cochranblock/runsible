# runsible — `runsible-lint`

## 1. Mission

`runsible-lint` is the first-party static analyzer for runsible packages, playbooks, and inventories. Per §14 of the redesign critique, it is structurally inseparable from the runtime: it imports `runsible-core`, parses the same TOML AST that `runsible-playbook` executes, and reports findings against that single source of truth. A rule cannot diverge from runtime semantics because both read the same bytes through the same grammar — there is no parallel parser, no separate schema, no version-skew matrix. This eliminates the Ansible failure mode where `ansible-lint 24.5` and `ansible-core 2.16` disagree on what "valid" means. The profile system (`min ⊂ basic ⊂ moderate ⊂ safety ⊂ shared ⊂ production`) is preserved so existing muscle memory carries over, but every rule is rewritten for the TOML world: typed handler IDs replace string-match notify, tag enums replace stringly-typed `--tags`, FQCNs replace the `collections:` keyword. It serves J6 (CI-friendly machine output) and J8 (one syntax to learn) from the user-story memo; without it the project ships a footgun catalog as documentation.

## 2. Scope

**In:** TOML playbook + package + inventory linting against the `runsible-core` AST. Rule catalog with the six ansible-lint profiles (`min`/`basic`/`moderate`/`safety`/`shared`/`production`) plus `opt-in` / `experimental` tags. Auto-fix (`--fix`) for safe-rewrite rules, preserving comments and key order via `toml_edit`. `.runsible-lint.toml` config (same key surface as `.ansible-lint`, retranslated). `.runsible-lint-baseline.toml` plus inline `# runsible-lint: noqa: <id>` directives. Output formats `text`, `json`, `sarif`, `github-actions`. CI integration: `GITHUB_ACTIONS=true` triggers workflow annotations; `--strict` makes warnings exit non-zero. ansible-lint config import for migrating teams. Schema linting of `runsible.toml`, `runsible.lock`, `meta/argument_specs.toml`. Vault hygiene (plaintext-secret patterns in non-vault files). `--explain <rule>` for in-terminal rule docs.

**Out:** YAML linting (yaml2toml is the bridge; the lint runs on converted TOML). Plugin-architecture rules (no Python plugin model in v1). `mock_modules`/`mock_roles`/`mock_filters` config — these exist in ansible-lint to paper over Ansible's late binding; the runsible parser knows every module at compile time, so a missing module is L018, not something to mock around. `pylint`/`pep8`/`mypy`/`pymarkdown` equivalents (no user Python). LSP server (deferred to a sibling `runsible-lsp` crate; §15). Custom Rust-trait rules (declarative TOML in v1; cdylib in v2). Galaxy-certification rules (`meta-no-dependencies`, `sanity`) — runsible has its own registry and does not court Red Hat's gate.

## 3. Rule catalog

Rule IDs use the `L###` namespace, grouped by category in 100-blocks. Severity legend: **E**=error, **W**=warning, **I**=info. Auto-fix marker: **[F]** if fixable. Citations point at `11-poor-decisions.md` (`§N`) or note the ansible-lint analogue when intent is preserved verbatim.

### 3.1 Schema validation (`L001`–`L020`)

Every key in a play/task/handler/block/manifest/inventory must validate against `runsible-core` types. Load-failure floor; cannot be `noqa`-suppressed.

- **L001 `parser-error`** / **L002 `load-failure`** / **L003 `internal-error`** (E, *min*) — TOML parse / file open / lint-crash failures.
- **L004 `schema-play`** / **L005 `schema-task`** (E, *min*) — unknown/missing key in play, or two module calls in one task.
- **L006 `schema-handler`** (E) — unknown key or missing `id` (§13). **L007 `schema-block`** (E) — unknown key or `loop`/`register`/`notify` on the container.
- **L008 `schema-inventory`** / **L009 `schema-vars`** / **L010 `schema-package`** / **L011 `schema-argspec`** (E) — out-of-grammar in inventory, vars, `runsible.toml`, or `meta/argument_specs.toml` (§5).
- **L012 `schema-meta-runtime`** / **L013 `schema-galaxy`** / **L014 `schema-execution`** (W) — out-of-grammar in `runtime.toml`, manifest Galaxy-style fields, or `[execution]` (replaces exec-env).
- **L015 `schema-requirements`** (E) — `[dependencies]` malformed.
- **L016 `key-order`** [F] (W) — canonical order: `id`, `name`, `tags`, `when`, `become`, …, module call last.
- **L017 `duplicate-key`** (E) — array-of-tables duplicate where uniqueness is required.
- **L018 `unknown-module`** (E, *min*) — module resolves to no installed package.
- **L019 `empty-playbook`** (E, *min*) — playbook has no plays.
- **L020 `missing-file`** (E) — referenced template/copy/include path absent.

### 3.2 Module reference (`L021`–`L030`)

Per §22, every reference is `package.module`. No shortening, no `collections:`, no implicit fallback; `[imports]` is lexical sugar.

- **L021 `module-name-fqcn`** [F] (E) — short name without `[imports]` alias (§22).
- **L022 `module-name-typo`** [F] (E) — fuzzy-suggest when name doesn't exist.
- **L023 `module-name-deprecated`** [F] (W) — successor is canonical.
- **L024 `imports-unused`** [F] (W) / **L025 `imports-shadowed`** (E) / **L026 `imports-circular`** (E) — alias hygiene.
- **L027 `command-instead-of-module`** (W) — `command` invokes `apt-get`/`systemctl` when a typed module exists.
- **L028 `command-instead-of-shell`** [F] (W) — `shell` without shell features.
- **L029 `only-builtins`** (I) — opt-in: forbid non-`runsible_builtin`.
- **L030 `module-name-legacy`** [F] (W) — `ansible.builtin.X` survived migration; rename.

### 3.3 Tag enum (`L031`–`L040`)

Per §19, tags are an enforced enum at the package level.

- **L031 `tag-undeclared`** (E) — tag not in `[tags]` (§19).
- **L032 `tag-unused`** [F] (W) — declared but never used.
- **L033 `tag-special-misuse`** (E) — collides with `always`/`never`/`untagged`/`all`/`tagged`.
- **L034 `tag-format`** [F] (W) — violates `^[a-z][a-z0-9_-]*$`.
- **L035 `tag-shadowed`** (W) — block + child-task collision.

### 3.4 Handler IDs (`L041`–`L050`)

Per §13, handlers carry typed IDs; `notify` references the ID.

- **L041 `handler-id-undeclared`** (E) — `notify` matches no `[[handlers]]` (§13).
- **L042 `handler-id-duplicate`** (E) / **L043 `handler-id-unused`** (W) / **L044 `handler-id-format`** [F] (W) — `id` hygiene.
- **L045 `handler-flush-unreachable`** (W) — `flush_handlers` is dead code.
- **L046 `handler-loop`** (E) — notify-cycle.
- **L047 `no-handler`** (W) — task uses `when = "<prev>.changed"` instead of `notify`.

### 3.5 Idempotence (`L051`–`L060`)

Per §9, idempotence rides on the `Module` trait's `plan/apply/verify`. `command`/`shell` bypass that.

- **L051 `command-no-idempotence`** / **L052 `shell-no-idempotence`** (W) — lacks `[idempotence]` guards (§9).
- **L053 `no-changed-when`** (W) / **L054 `read-only-changed-when`** [F] (W) — mutating tasks need `changed_when`; read-only tasks need `changed_when = false`.
- **L055 `verify-disabled`** (I) — module sets `verify_idempotent = false`.
- **L056 `latest-state`** (W) / **L057 `git-head`** (E) — non-deterministic `state = "latest"` / `version = "HEAD"`.
- **L058 `loop-mutates-shared`** (W) — iteration writes a var the next iteration reads.
- **L059 `set-fact-mutation`** [F] (W) — bare `set_fact` vs `set_fact!` (§4).
- **L060 `delegate-without-runonce`** (W) — `delegate_to` on per-host loop without `run_once` (§18).

### 3.6 Style (`L061`–`L080`)

- **L061** / **L062 `name-missing-task`/`-play`** (W) — task/play lacks `name`.
- **L063 `name-casing`** [F] (W) — must start uppercase.
- **L064 `name-template-mid`** [F] (W) — `{{ }}` must be tail-only.
- **L065 `name-unique`** (W) — name collision; `--start-at-task` ambiguous.
- **L066 `name-prefix`** [F] (I) — opt-in: included tasks prefixed `<stem> | `.
- **L067 `package-name-format`** [F] (W) — violates `^[a-z][a-z0-9_]*$`.
- **L068 `var-naming-pattern`** [F] (W) / **L069 `var-naming-keyword`** (E) / **L070 `var-naming-magic`** (E) — pattern, keyword collision, magic-var shadow.
- **L071 `var-naming-package-prefix`** [F] (I) / **L072 `loop-var-prefix`** [F] (I) — package vars + loop vars must be `<package>_`-prefixed.
- **L073 `block-depth`** (W) / **L074 `tasks-per-file`** (W) — exceed `max_block_depth` (default 8) or `max_tasks` (default 100).
- **L075 `partial-become`** [F] (W) — `become_user` without `become = true`.
- **L076 `risky-mode`** [F] (W) — `mode = 644` vs `mode = "0644"` (was `risky-octal`; renamed because TOML expresses ints exactly — the bug is wrong-type, not octal vs decimal).
- **L077 `risky-file-permissions`** (W) — file-touching module without explicit `mode`.
- **L078 `risky-shell-pipe`** (W) — `shell` pipe without `set -o pipefail`.
- **L079 `inline-env-var`** [F] (W) — env var inline in `command`; use `[environment]`.
- **L080 `no-relative-paths`** (W) — `copy.src`/`template.src` via `..` instead of `files/`.

### 3.7 Vault (`L081`–`L090`)

Plaintext-secret heuristics. Prevents most "committed the password" incidents.

- **L081 `secret-pattern-aws`** (E) — `AKIA[0-9A-Z]{16}` in non-vault file (§6).
- **L082 `secret-pattern-pem`** (E) — `-----BEGIN PRIVATE KEY-----` block.
- **L083 `secret-pattern-jwt`** (W) — `eyJ`-shaped string of plausible length.
- **L084 `secret-pattern-bearer`** (W) — `Bearer [A-Za-z0-9_-]{20,}`.
- **L085 `secret-pattern-entropy`** (W) — high-entropy string (>4.5 bits/char, length >32).
- **L086 `vault-no-recipients`** (E) / **L087 `vault-stale-recipient`** (W) — vault hygiene (§6).
- **L088 `no-log-password`** [F] (W) — loop over `*password*`/`*secret*`/`*token*` without `no_log`.
- **L089 `no-prompting`** (W) — `vars_prompt` used (CI hostility).
- **L090 `become-password-plaintext`** (E) — literal `become_password`; use `keyring:`/`vault:` (§16).

### 3.8 Variables (`L091`–`L100`)

- **L091 `var-undefined`** (E) — `{{ var }}` resolves to no layer (tractable via shared parser).
- **L092 `var-undeclared-required`** (W) — `mandatory` without fallback.
- **L093 `precedence-compat-removed`** (W) — references a layer only in `--precedence-compat ansible` (§3).
- **L094 `set-fact-cacheable`** [F] (W) — `cacheable = true` removed; use `runsible-fact-store` (§4).
- **L095 `var-shadow-precedence`** (W) / **L096 `extra-vars-shadowed`** (I) — multi-layer shadow vs always-overridden-by-`--extra-vars`.
- **L097 `omit-misuse`** (W) — `omit` outside `default()`.
- **L098 `register-overwrite`** (W) — two tasks register to the same name.
- **L099 `loop-var-shadow`** [F] (W) — nested loop reuses default `item`.
- **L100 `vars-file-missing`** (E) — `vars_file` path absent.

### 3.9 Compatibility / deprecation (`L101`–`L120`)

Fires on shapes produced by `yaml2toml`; nudges toward canonical runsible idioms.

- **L101 `set-fact-mutation-bare`** [F] (W) — `set_fact` lacks `!` (§4).
- **L102 `meta-legacy-form`** [F] (W) — `meta = "flush_handlers"` vs typed control task (§17).
- **L103 `with-loop-form`** [F] (W) — `with_*` vs `loop`.
- **L104 `local-action-form`** [F] (W) — `local_action` vs `delegate_to + connection`.
- **L105 `no-free-form-args`** [F] (W) — free-form `command = "chdir=/tmp …"` vs structured.
- **L106 `bare-when-jinja`** [F] (W) — `when = "{{ var }}"` vs `when = "var"`.
- **L107 `literal-compare`** [F] (W) / **L108 `empty-string-compare`** [F] (I) — redundant `== true` / `== ""` comparisons.
- **L109 `collections-keyword`** [F] (W) — `collections = [...]` vs `[imports]` (§22).
- **L110 `become-flat-keywords`** [F] (W) — flat siblings vs typed `[plays.become]` (§16).
- **L111 `serial-stringly`** [F] (W) — `serial = "20%"` vs `[plays.rollout]` (§21).
- **L112 `async-poll-naming`** [F] (W) — `async`/`poll` vs `[…async]` / `[…background]` (§24).
- **L113 `failed-when-double-negative`** [F] (W) — `failed_when` vs `success_predicate` (§25).
- **L114 `register-untyped`** (I) — register slot can't be typed against module `Outcome`.
- **L115 `include-vs-import`** [F] (W) — `include_tasks`/`import_tasks` vs `compose` (§25).
- **L116 `gather-everything`** [F] (W) — `gather_facts = true` without `facts.required` (§12).
- **L117 `hash-behaviour-merge`** (E) — global `hash_behaviour = merge` removed (§25).
- **L118 `vault-symmetric-only`** (W) — relies on `--vault-password-file` only (§6).
- **L119 `gathered-fact-unused`** [F] (I) / **L120 `gathered-fact-missing`** (W) — fact-subset / required-list mismatches (§12).

### 3.10 TOML format (`L121`–`L130`)

Replaces ansible-lint's `yaml[*]` family. Backed by `taplo`.

- **L121 `toml-format`** [F] (W) — whitespace/indentation per `taplo` config.
- **L122 `toml-line-length`** [F] (W) — >120 chars (configurable).
- **L123 `toml-trailing-spaces`** [F] (W) / **L124 `toml-eof-newline`** [F] (W) / **L126 `toml-empty-lines`** [F] (W) — whitespace hygiene.
- **L125 `toml-line-endings`** [F] (E) — CRLF detected.
- **L127 `toml-key-order-canonical`** [F] (I) — manifest keys out of order.
- **L128 `playbook-extension`** (W) / **L129 `template-extension`** [F] (W) — playbook must be `.toml`, template source `.tera`.
- **L130 `toml-comment-style`** [F] (I) — inconsistent `# ` vs `#`.

Total v1: **130 rules** across 10 categories. M0 ships ~20; M1 the remainder; M2 expands auto-fix (§5).

## 4. Profiles

Inheritance-only, per ansible-lint: `production ⊃ shared ⊃ safety ⊃ moderate ⊃ basic ⊃ min`. `opt-in` and `experimental` rules require both profile membership *and* `enable_list` to fire.

- **`min`** (cannot be skipped): L001, L002, L003, L004, L005, L018, L019.
- **`basic`** adds: L006–L011, L015, L016, L020–L022, L027, L028, L030, L031, L041, L042, L061–L063, L067–L069, L075, L076, L079, L101–L108, L121, L122, L124, L125, L128.
- **`moderate`** adds: L023, L024, L033, L034, L043, L044, L046, L053, L054, L064, L065, L073, L074, L078, L091, L097–L099, L109, L110, L113, L114, L123, L126, L127, L129.
- **`safety`** adds: L032, L045, L047, L051, L052, L055–L060, L070, L077, L081–L084, L086, L087, L090, L092, L093, L095, L100, L115, L117, L118.
- **`shared`** adds: L012, L013, L014, L025, L026, L071, L080, L085, L088, L089, L094, L116, L120, L130.
- **`production`** adds: L029, L066, L072, L089 (all opt-in by default), L111, L112, L119; implicitly raises warnings to errors (`--strict`).

## 5. Auto-fix

`--fix` rewrites files in place via `toml_edit`, preserving comments, blank lines, and key order. Without it, the diff prints to stdout (`text` mode) or appears in the JSON event (machine modes). `RUNSIBLE_LINT_WRITE_TMP=1` diverts fixes to a temp directory for the test harness.

**M1 set (10):** L016, L021, L059, L101, L102, L104, L105, L106, L121, L125. **M2 expansion** covers ~30 more (L022–L024, L028, L030, L032, L034, L044, L054, L063, L064, L067, L068, L075, L076, L079, L088, L094, L099, L103, L107–L113, L115, L116, L119, L122–L124, L127, L129, L130; L066, L071, L072 when opted-in).

### Representative before/after

**L021 `module-name-fqcn`** — short name → FQCN:
```toml
# before:  package                  = { name = "nginx", state = "present" }
# after:   "runsible_builtin.package" = { name = "nginx", state = "present" }
```

**L101 `set-fact-mutation-bare`** — yaml2toml emits bare `set_fact`; promoted to `set_fact!` so the migration stays visible:
```toml
# before:  set_fact   = { release_id = "{{ ansible_date_time.iso8601 }}" }
# after:  "set_fact!" = { release_id = "{{ ansible_date_time.iso8601 }}" }
# runsible-lint: TODO L059 — refactor to [plays.let] to drop the `!`
```

**L102 `meta-legacy-form`** — collapse to typed control task (§17):
```toml
# before:  meta = "flush_handlers"
# after:   type = "control"
#          action = "flush_handlers"
```

**L104 `local-action-form`** — rewrite to delegation:
```toml
# before:  local_action = { module = "runsible_builtin.shell", cmd = "echo hi" }
# after:   delegate_to = "localhost"
#          connection  = "local"
#          "runsible_builtin.shell" = { cmd = "echo hi" }
```

**L106 `bare-when-jinja`** — strip `{{ }}`:
```toml
# before:  when = "{{ ansible_facts.os_family == 'RedHat' }}"
# after:   when = "ansible_facts.os_family == 'RedHat'"
```

**L121 `toml-format`** — taplo whitespace normalization (`cargo fmt` for TOML).

## 6. CLI surface

```
runsible-lint [PATHS...]
```

Empty `PATHS` walks the project rooted at the discovered `.runsible-lint.toml`. Glob expansion via the shell.

| Flag                          | Default | Env                       | Purpose                                                          |
|-------------------------------|---------|---------------------------|------------------------------------------------------------------|
| `--profile <name>`            | `basic` | `RUNSIBLE_LINT_PROFILE`   | Override config-file profile.                                    |
| `--config <file>`             | search  | `RUNSIBLE_LINT_CONFIG`    | Override `.runsible-lint.toml` discovery.                        |
| `--format <fmt>`              | auto    | `RUNSIBLE_LINT_FORMAT`    | `text` / `json` / `sarif` / `github-actions` / `auto` (TTY→text).|
| `--sarif-file <path>`         | unset   | —                         | Also write SARIF to file.                                        |
| `--fix`                       | false   | —                         | Apply auto-fixes in place.                                       |
| `--explain <rule-id>`         | —       | —                         | Print rule's full description and exit.                          |
| `--list-rules` / `--list-profiles` / `--list-tags` | — | — | Catalog views and exit.                                  |
| `--severity <level>`          | unset   | —                         | Threshold filter (`error`/`warning`/`info`).                     |
| `--strict`                    | false   | `RUNSIBLE_LINT_STRICT`    | Warnings exit non-zero.                                          |
| `--baseline <file>` / `--generate-baseline` | unset | `RUNSIBLE_LINT_BASELINE` | Suppress matching findings; write a fresh baseline. |
| `--exclude <glob>` (repeat)   | unset   | —                         | Skip matching files.                                             |
| `--enable` / `--disable` / `--warn <rule>` (repeat) | unset | — | Profile overrides at the CLI.                            |
| `--offline`                   | false   | `RUNSIBLE_LINT_OFFLINE`   | Skip network-dependent checks (none in v1; reserved).            |
| `--project-dir <path>`        | cwd     | —                         | Override project-root discovery.                                 |
| `--show-relpath`              | false   | —                         | Paths relative to cwd.                                           |
| `--no-color`, `--quiet`, `-v`, `-h`, `--version` | — | `NO_COLOR`, `RUNSIBLE_LINT_VERBOSITY` | Standard.                          |

**Exit codes:** `0` clean, `2` findings present, `4` config error, `5` bad CLI options, `6` internal error.

**Removed vs `ansible-lint`:** `--parseable` (use `--format text:compact`); `--rules-dir`/`-R` (v2 with cdylib rules); `--yamllint-file` (no YAML); `--generate-ignore` (renamed `--generate-baseline`).

## 7. The `.runsible-lint.toml` schema

```toml
[lint]
profile = "production"
exclude = ["legacy/**", "vendor/**"]
parseable = true                # alias for format = "text:compact"
strict = false                  # warnings → non-zero exit
offline = false
use_default_rules = true        # false = only [lint.rules].enable fires

[lint.rules]
disable = ["L042", "L055"]
warn_only = ["L060", "L073"]
enable = ["L029", "L066"]       # opt-in rules
severity_overrides = { L010 = "warning", L091 = "info" }

[lint.naming]
var_pattern = "^[a-z_][a-z0-9_]*$"
loop_var_prefix = "^(__|{package}_)"
task_name_prefix = "{stem} | "
package_name_pattern = "^[a-z][a-z0-9_]*$"
tag_pattern = "^[a-z][a-z0-9_-]*$"

[lint.complexity]
max_block_depth = 8
max_tasks = 100

[lint.fix]
write_list = "all"              # or a list of rule IDs; or "none"

[lint.baseline]
file = ".runsible-lint-baseline.toml"

[lint.kinds]
playbook  = ["**/playbooks/*.toml"]
tasks     = ["**/tasks/*.toml"]
vars      = ["**/vars/*.toml", "**/defaults/*.toml"]
inventory = ["inventory/*.toml"]
package   = ["runsible.toml"]

[lint.import-ansible-lint]
enabled = true
config = ".ansible-lint"
strict_translation = false

[lint.vault.patterns]
custom = [{ id = "L081-extra-1", regex = "MYCO-[A-Z0-9]{24}", severity = "error" }]

[lint.toml-format]
config = "taplo.toml"
```

Discovery walks cwd up to the git root. `exclude` is relative to the config file; CLI `--exclude` is relative to cwd (matches ansible-lint per §2.4 of `10-test-and-lint.md`).

## 8. Inline directives

- **Per-line:** `# runsible-lint: noqa: L001 L002` — suppress on the next TOML statement.
- **Trailing:** `key = "v"  # runsible-lint: noqa: L076` — attach to the line.
- **Block:** `# runsible-lint: skip_block` before a `[table]` heading suppresses the entire block until the next same-depth heading.
- **File:** `# runsible-lint: skip_file: L091 L092` in the first 10 lines (bare `skip_file` rejected; too easy to abuse).
- **Re-enable:** `# runsible-lint: noqa-end` cancels `skip_block` early.

Dropped vs ansible-lint: the `tags = ["skip_ansible_lint"]` mechanism — tags are now an enforced enum (L033) and overloading them as lint directives breaks the type system.

The baseline file is a TOML map of `path → list of {rule_id, line_range_normalized, message_hash}` fingerprints. Exact-match findings are suppressed; new findings are reported; stale baseline entries are warnings (errors under `--strict`, encouraging pruning).

## 9. Custom rules

**v1 — declarative TOML.** Users add rules without Rust. Schema loaded from `<project>/.runsible-lint/rules/*.toml`:

```toml
[[rule]]
id = "ORG-001"
name = "no-bare-shell-in-prod"
severity = "error"
profile = "production"
tags = ["custom", "org"]
description = "Production playbooks may not use runsible_builtin.shell directly."

[rule.match]
path = "plays[*].tasks[*]"
where = [
  { field = "module", equals = "runsible_builtin.shell" },
  { field = "tags",   contains = "production" },
]

[rule.report]
message = "Use a typed module or wrap shell access in a vetted helper package."
fix = "none"   # or remove_field / rename_field / replace_field with kwargs
```

Predicates compile into a small interpreter at startup; v1 supports `equals`, `not_equals`, `contains`, `regex_match`, `defined`, `undefined`, plus `all_of` / `any_of` / `not` composition.

**v2 — Rust trait + cdylib (deferred).** Once the runsible cdylib ABI stabilizes (likely v1.5 plugin work), `runsible-lint` accepts `.so`/`.dylib`/`.dll` plugins exporting a `runsible_lint_rule` symbol implementing the `LintRule` trait from §4.5 of `10-test-and-lint.md`. Declarative rules cover ~90% of org-policy use cases without users shipping native code.

**Discovery (later overrides earlier on ID collision):** built-in catalog → `~/.config/runsible/lint/rules/*.toml` → `<project>/.runsible-lint/rules/*.toml`. Collision is an error unless the new rule declares `[rule.replaces]`.

## 10. Redesigns vs Ansible

Per §14, the headline change is **first-party + shared parser**. Downstream consequences:

- **No mock layer.** ansible-lint's `mock_modules`/`mock_roles`/`mock_filters` exist to paper over Ansible's late binding. The runsible parser knows everything at compile time; a missing module is L018 and the lint stops. Nothing to mock.
- **No Python plugin model.** v1 declarative rules are a strict subset of what ansible-lint allows; no "is this filter even loaded?" ambiguity.
- **No separate release cadence.** `runsible-lint`'s version equals `runsible-core`'s version, in lockstep. Eliminates the "ansible 2.16 + ansible-lint 24.5 disagree" failure mode.
- **No `skip_ansible_lint` tag.** Tags are an enforced enum (§19); overloading them as lint directives violates the type system. Inline `# runsible-lint: noqa` replaces it.
- **No `--parseable` flag.** Subsumed into `--format text:compact`.
- **No `--use-default-rules false` on the CLI.** Config-only; too easy to misconfigure into "all lints off" in CI.
- **No cosmetic Galaxy rules.** `meta-no-dependencies`, `sanity` (the rule), `meta-video-links`, `galaxy[*]` dropped — the runsible-native registry has its own validator.
- **`yaml` family → `toml` family.** Same intent, different tool; `taplo` provides the formatter.
- **`risky-octal` → L076 `risky-mode`.** TOML expresses ints exactly, so the rule is now about mode-as-string hygiene, not YAML's octal ambiguity.

## 11. Milestones

- **M0 — proof of life** (≈12 wk post-AST): 20 rules (all of `min` plus L001–L020 schema set); `text` + `json` output; `.runsible-lint.toml` discovery + `[lint.rules]`; inline `noqa`; `--profile` / `--format` / `--explain` / `--list-rules`; test harness + golden corpus. No auto-fix.
- **M1 — full catalog** (≈28 wk): all 130 rules; all six profiles; auto-fix for the M1 set of 10; baseline (`--baseline` / `--generate-baseline`); custom declarative TOML rules; `severity_overrides` / `naming` / `complexity` / `kinds`.
- **M2 — CI polish + ecosystem** (≈40 wk): SARIF + GitHub Actions output; ansible-lint config import; auto-fix M2 expansion (~30 more); custom-rule cdylib (only if the runsible plugin ABI is ready, else punt to M3); pre-commit hook + GitHub Action published.

## 12. Dependencies on other crates

**Imports:** `runsible-core` (parser, AST, module registry; pinned exact `= 0.x.y` workspace-wide so mismatch is a build error); `runsible-config` (`.runsible-lint.toml` precedence, env-var resolution); `runsible-vault` (L086/L087). Third-party: `taplo` (TOML formatter, backs L121 / `--fix`), `toml_edit` (comment + key-order preservation), `serde_json` plus a SARIF crate. `pubgrub` (already in workspace) reserved for future graph-analysis rules.

**Imported by:** nothing at runtime — `runsible-lint` is a leaf binary. The library half (`runsible-lint-core`, hosting the rule trait + catalog) can be re-exported into a future `runsible-lsp` without dragging CLI deps. **No dependency on `runsible-playbook`** — that would loop; the shared parser lives in `runsible-core`.

## 13. Tests

- **Per-rule golden corpus.** `tests/rules/L###/in/*.toml` + `expected.json`; the harness runs the rule alone and snapshots; `RUNSIBLE_LINT_BLESS=1` to update.
- **Auto-fix golden pairs.** `tests/fix/L###/in.toml` + `out.toml`.
- **Profile resolution.** Matrix test: each profile, exact rule-ID set, representative playbook.
- **Baseline suppression.** Generate baseline, mutate source, assert pre-existing suppressed and new surfaced.
- **Output format snapshots.** One project, one snapshot per format.
- **Inline directives.** `tests/noqa/` corpus for `noqa`, `skip_block`, `skip_file`, `noqa-end`.
- **Custom-rule loading.** Project-local declarative rule that fires, plus one that fails to load.
- **ansible-lint config import.** Fixture set of real `.ansible-lint` files; assert translated TOML is byte-stable.
- **Cross-crate version pinning.** CI check: `Cargo.lock`'s `runsible-core` equals the declared dep.
- **Real-corpus smoke.** Linter against runsible-converted `geerlingguy.docker`, selected `community.general`, in-tree examples; gate on no L001/L002/L003 regressions.

`RUNSIBLE_LINT_WRITE_TMP=1` diverts auto-fix output to temp paths so the source tree isn't mutated mid-run.

## 14. Risks

| # | Risk                                                          | P | I | Mitigation                                                                |
|---|---------------------------------------------------------------|---|---|---------------------------------------------------------------------------|
| 1 | Rule overreach — too pedantic, users disable wholesale.       | H | H | Profile gating; only `min` uncloaked by default; new rules ship `experimental`. |
| 2 | Schema drift from `runsible-playbook` despite shared parser.  | M | H | Exact-version workspace pin; CI check fails build on mismatch.            |
| 3 | Auto-fix breaks semantics on a real playbook.                 | M | H | Per-fix golden pairs; large fix corpus from yaml2toml output.             |
| 4 | Custom rules become a back-door for Python plugins.           | M | M | Declarative-only in v1; TOML schema spec-reviewed; no code execution.     |
| 5 | SARIF / GitHub Actions formats drift upstream.                | L | L | Snapshot-based; explicit schema bump on each upstream change.             |
| 6 | ansible-lint config import becomes a maintenance sink.        | M | M | M2 only; best-effort with `--strict-translation` opt-in.                  |
| 7 | Catalog bloats past 200 rules.                                | M | L | Hard cap of 200 in v1; a 201st must displace an existing rule via PR.     |
| 8 | Vault-pattern false positives on test fixtures.               | H | L | Documented `# runsible-lint: noqa: L08*` first-line pattern.              |

## 15. Open questions

- **LSP.** Bundle into the lint binary (`runsible-lint --lsp`) or split into `runsible-lsp` sharing the catalog? Incremental parsing differs from batch lint and the LSP needs a cancel-able rule runner. Default: separate crate.
- **Comment preservation on auto-fix.** `toml_edit` keeps trailing comments per key, but reorganizing siblings for L016 `key-order` can lose line-attached comments. Acceptable for v1; a `--fix --preserve-comments=strict` flag could refuse a fix that would drop comments.
- **Markdown / docs linting.** `runsible-doc lint` handles `.doc.toml`; project-level `README.md` is out of scope. `pymarkdown` does not port.
- **`--strict` semantics.** (a) raise warnings to errors (ansible-lint default) vs (b) exit non-zero on warnings without re-classifying (friendlier to JSON consumers). Default: (b), with `--strict-classify` for (a).
- **In-place vs preview `--fix`.** Default in-place (ansible-lint behavior); ship `--fix-dry-run` for diff preview.
- **`runsible-lint init` subcommand?** Scaffolds `.runsible-lint.toml` from project contents. Default: yes, gated behind `init --profile basic`.
- **Whole-project rules** (e.g., "no two packages declare the same module name"). Don't fit the per-file model; a separate phase. Defer to M2 with a `[[whole-project-rule]]` schema if demand surfaces.
