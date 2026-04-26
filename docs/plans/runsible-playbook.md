# runsible — `runsible-playbook`

> **Document weight.** This is the plan for the central crate of the
> project. Every other crate either feeds this one or is subordinated to
> it. When in doubt about a design choice elsewhere in runsible, the
> tie-breaker is "what makes `runsible-playbook` correct, fast, and
> auditable?"
>
> **Source documents** (read first):
> - `docs/research/00-user-story-analysis.md` (the personas this engine serves)
> - `docs/research/01-cli-surface.md` §2 (the `ansible-playbook` flag matrix we mirror)
> - `docs/research/02-playbook-language.md` (the entire YAML semantics we re-implement in TOML)
> - `docs/research/03-inventory.md` (where targets come from)
> - `docs/research/06-configuration-reference.md` (settings the engine reads)
> - `docs/research/08-builtin-modules.md` (the kit this engine drives)
> - `docs/research/09-connection-templating-facts.md` (templating, become, facts)
> - `docs/research/11-poor-decisions.md` (THE redesign bible — applied below by §-number)

---

## 1. Mission

`runsible-playbook` is the runsible **engine**: parse a TOML playbook and
its imports, type-check the whole project against a known module catalog,
resolve variables and roles into a per-host execution plan, dispatch that
plan through `runsible-connection`, gather only the facts the plan
actually depends on, run handlers at flush points, emit a fully
structured NDJSON event stream, and exit with a code that summarises the
outcome across hosts. The engine is a Rust binary that boots in under
100 ms cold, refuses to touch a host until the entire project has
parsed and type-checked, and treats `plan() → apply() → verify()` as
the canonical task lifecycle for every module. Everything else — the
ad-hoc CLI, the lint, the doc browser, the pull-mode daemon — is a
re-skinning of this same engine. If `runsible-playbook` is wrong,
runsible is wrong.

---

## 2. Scope

### In scope

- Parsing TOML playbooks (and the imports they pull in) into a fully
  typed AST.
- Type-checking the AST against the module catalog (every `task.action`
  resolves to a module; every arg matches the module's declared schema;
  every var reference is reachable; every tag is declared in the project's
  `[tags]` enum; every handler `notify` resolves to a handler ID; every
  conditional is well-formed).
- Resolving roles (packages declared via the runsible package model — see
  poor-decisions §5), including their `defaults`, `vars`, `tasks`,
  `handlers`, and `argument_specs`.
- Resolving the `[imports]` block (poor-decisions §22) into lexical
  module aliases, file-scoped, with no implicit collection-search behavior.
- Computing per-host execution plans honoring `[plays.rollout]`
  (poor-decisions §21): typed batches, deterministic ordering with seeded
  shuffle, and per-batch `max_fail_percentage`.
- Running plays in parallel batches per the chosen strategy (`linear`,
  `free`, `host_pinned`).
- Lazy fact gathering (poor-decisions §12): only the subsets the plan
  references, with static inference and `facts.required = [...]` overrides.
- Templating via a single chosen Rust engine (see §10) with a fixed
  filter/test catalog (poor-decisions §2).
- The shadowing-and-`let`-block model for variable scope (poor-decisions §4).
- Conditional `when`, looping `loop`, retry `until`, error-handling
  `block`/`rescue`/`always`, async, run_once, delegate_to, throttle,
  serial batching, check mode, diff mode.
- Privilege escalation as a typed `[plays.become]` sub-document
  (poor-decisions §16) backed by system keyring secrets.
- First-class `register` with typed `Outcome`s (poor-decisions §25).
- Handler firing at explicit flush points; handler IDs (not name-string
  matching), so typos are parse-time errors (poor-decisions §13).
- NDJSON event stream (`runsible.event.v1`) on stdout when not a TTY,
  pretty rendering when on a TTY (poor-decisions §10).
- `--check`, `--diff`, `--list-tasks`, `--list-tags`, `--list-hosts`,
  `--syntax-check`, `--start-at-task`, `--step`, plus a runsible-native
  `--plan-only` mode that prints the planned diff per host without applying.
- Lockfile-backed reproducible runs (M3): `runsible.lock` records the
  resolved plan hashes for each host so re-runs can detect drift between
  plan and apply.

### Out of scope

- **Plugin loading.** No dynamic discovery of out-of-tree connection,
  callback, lookup, filter, or strategy plugins in v1. Connections are an
  enum. Filters are a fixed catalog. Strategies are an enum. Stable
  Rust trait for out-of-tree authors is a v1.5 problem.
- **YAML parsing.** runsible-playbook only consumes TOML. Conversion
  from YAML lives entirely in the separate `yaml2toml` crate; the engine
  has no awareness of YAML quirks.
- **ansible-galaxy integration.** Package install/resolve lives in
  `runsible-galaxy`. The engine consumes already-installed packages via
  `runsible-core::PackageStore`.
- **Per-collection module reference.** Each builtin module is its own
  Rust crate or workspace member that implements `runsible_core::Module`.
  This crate drives them; it does not house their docs, tests, or wire
  formats.
- **Vault decryption mechanics.** runsible-playbook calls into
  `runsible-vault` to decrypt vars files and inline `!vault` strings
  (well, the runsible-native equivalent) lazily; key resolution and
  recipient management happen there.
- **Inventory parsing.** `runsible-inventory` returns a typed
  `Inventory` value; this crate consumes it.
- **Configuration loading.** `runsible-config` returns a typed
  `Settings` value; this crate consumes it.
- **The `runsible` ad-hoc CLI.** That binary lives in its own crate but
  uses `runsible-playbook` as a library (it builds a one-task synthetic
  playbook and runs it). The engine itself is library-first; the
  `runsible-playbook` binary is a thin shell.

---

## 3. The TOML playbook schema

This is the canonical, opinionated, post-redesign schema. Every Ansible
keyword has a runsible analogue; the analogue is named for clarity, not
for compatibility. The yaml2toml converter handles the renames.

### 3.1 Project root layout

A runsible project is a directory containing:

```
.
├── runsible.toml             # project manifest (name, version, dependencies, [tags] enum)
├── runsible.lock             # resolved package versions + plan hashes (M3)
├── inventory/
│   ├── hosts.toml            # the canonical inventory
│   ├── group_vars/
│   │   └── webservers.toml
│   └── host_vars/
│       └── web01.toml
├── playbooks/
│   ├── site.toml             # the entry-point playbook
│   └── deploy.toml
├── packages/                 # local packages (runsible's "roles")
│   └── nginx/
│       ├── runsible.toml     # package manifest
│       ├── tasks/main.toml
│       ├── handlers/main.toml
│       ├── defaults/main.toml
│       ├── vars/main.toml
│       ├── files/
│       └── templates/
└── secrets/
    └── prod.runvault         # age-recipient encrypted file (runsible-vault)
```

A playbook is a TOML file with `[[plays]]` arrays. Each play is a single
`[[plays]]` table with sub-tables for `tasks`, `handlers`, `pre_tasks`,
`post_tasks`, `roles`, etc.

### 3.2 The complete play

```toml
# playbooks/site.toml
schema = "runsible.playbook.v1"

[imports]
# Lexical aliases for module FQ names. Scoped to this file. (poor-decisions §22)
copy     = "runsible_builtin.copy"
template = "runsible_builtin.template"
service  = "runsible_builtin.service"
apt      = "runsible_builtin.apt"
file     = "runsible_builtin.file"
debug    = "runsible_builtin.debug"
assert   = "runsible_builtin.assert"
command  = "runsible_builtin.command"
shell    = "runsible_builtin.shell"

# ----- Play -----
[[plays]]
name = "Update web tier"
hosts = ["webservers", "&production"]   # list, not ":"-joined string
remote_user = "deploy"
connection = "ssh"
port = 22
strategy = "linear"            # "linear" | "free" | "host_pinned"
throttle = 4                   # task-level concurrency cap
force_handlers = true
ignore_unreachable = false
no_log = false
debugger = "on_failed"         # "always" | "never" | "on_failed" | "on_unreachable" | "on_skipped" | "on_ok"

# Mode forcing (independent of CLI --check / --diff)
check_mode = false
diff = true

# Tags applied to every task in this play
tags = ["release", "web"]

# Project-wide-declared tags only; using one not in [tags] is a parse error.

# --- Lazy facts (poor-decisions §12) ---
[plays.facts]
required = ["distribution", "kernel", "network"]
timeout_seconds = 30
fact_path = "/etc/runsible/facts.d"      # plus fallback to /etc/ansible/facts.d for migration

# --- Typed rollout (poor-decisions §21) ---
[plays.rollout]
batches = [1, 5, 10, "100%"]   # int | "N%"
order = "inventory"            # "inventory" | "reverse_inventory" | "sorted" | "reverse_sorted" | { shuffle = { seed = 42 } }
max_fail_percentage = 10       # 0..=100; abort batch when exceeded

# --- run_once is a play-level concern (poor-decisions §18) ---
[plays.run_once]
# When set, this play runs exactly once on the named delegate; the result is
# broadcast as a fact to every host in plays.hosts. Omit to run per-host.
host = "lb01.corp"

# --- Privilege escalation (poor-decisions §16) ---
[plays.become]
enabled = true
user = "root"
method = "sudo"                # "sudo" | "su" | "doas" | "machinectl" | "runas" | "enable"

[plays.become.sudo]
flags = ["-H", "-S", "-n"]
preserve_env = ["HOME", "USER"]
password = { from_keyring = "runsible:sudo:prod" }   # never plaintext

# --- Static let-bindings (poor-decisions §4) ---
[plays.let]
release_id = "{{ now('%Y%m%d_%H%M%S') }}"
artifact_url = "https://artifacts/{{ app_version }}.tar.gz"

# --- Vars (lowest of the in-play tiers; see §6 of poor-decisions for the 5-level model) ---
[plays.vars]
app_version = "1.4.2"
corp_proxy = "http://proxy:3128"

[plays.vars_files]
# A list; each entry is a path or a "first-found" sublist.
files = [
  "vars/secrets.runvault",
  ["vars/{{ ansible_facts['os_family'] }}.toml", "vars/os_defaults.toml"],
]

# --- Module defaults (poor-decisions §15 still applies) ---
[plays.module_defaults."runsible_builtin.file"]
owner = "app"
group = "app"
mode = "0640"

# --- Environment (per-task remote env) ---
[plays.environment]
HTTPS_PROXY = "{{ corp_proxy }}"

# --- Pre-tasks ---
[[plays.pre_tasks]]
name = "Drain from LB"
delegate_to = "lb01.corp"
command = { argv = ["drain", "{{ inventory_hostname }}"] }

# --- Roles (runsible "packages") ---
[[plays.roles]]
name = "common"

[[plays.roles]]
name = "webserver"
tags = ["web"]
[plays.roles.vars]
port = 8080

# --- Tasks ---
[[plays.tasks]]
name = "Pull artifact"
register = "dl"
until = { expr = "dl is succeeded" }
retries = 5
delay_seconds = 10
async = { timeout = "10m" }    # poor-decisions §24: `async` is a typed sub-table, not a magic int
get_url = { url = "{{ artifact_url }}", dest = "/tmp/app.tgz", checksum = "sha256:{{ app_sha256 }}" }

[[plays.tasks]]
name = "Apply config (block)"
[[plays.tasks.block]]
template = { src = "app.conf.j2", dest = "/etc/app/app.conf" }
notify = ["restart_app"]
[[plays.tasks.block]]
shell = { cmd = "validate-config" }
changed_when = { expr = "false" }
[[plays.tasks.rescue]]
debug = { msg = "rolling back {{ ansible_failed_task.name }}" }
[[plays.tasks.rescue]]
copy = { src = "/etc/app/app.conf.bak", dest = "/etc/app/app.conf", remote_src = true }
[[plays.tasks.always]]
control = { action = "flush_handlers" }   # poor-decisions §17: typed control flow

[[plays.tasks]]
name = "Restart only when version changed"
when = { expr = "dl is changed and 'OK' in dl.stdout" }
loop = ["nginx", "php-fpm"]
loop_control = { loop_var = "svc", label = "{{ svc }}" }
service = { name = "{{ svc }}", state = "restarted" }

# --- Post-tasks ---
[[plays.post_tasks]]
name = "Add back to LB"
delegate_to = "lb01.corp"
command = { argv = ["enable", "{{ inventory_hostname }}"] }

# --- Handlers (typed IDs, not strings; poor-decisions §13) ---
[plays.handlers.restart_app]
listen = ["app_reload"]
service = { name = "myapp", state = "restarted" }

[plays.handlers.reload_nginx]
listen = ["app_reload"]
service = { name = "nginx", state = "reloaded" }
```

Key shape decisions visible above:

- **Module call is a single inline TOML table on the task.** A task has
  exactly one module key (`copy = {...}`, `service = {...}`, etc.); the
  schema validator enforces this. Aliases come from `[imports]`; the
  alias `copy` resolves lexically to `runsible_builtin.copy`.
- **`when`, `until`, `failed_when`, `changed_when`** are `{ expr = "..." }`
  tables — never bare strings. This sidesteps the "is this a Jinja
  expression or a literal" ambiguity Ansible suffers (poor-decisions §25).
- **`async`** is a typed sub-table, never overloaded with `poll: 0`
  meaning "fire and forget"; for fire-and-forget use `background = true`
  (see §3.5).
- **Tags are an enum.** They must appear in the project-level `[tags]`
  block (`runsible.toml`); CLI `--tag` for an undeclared tag is a hard
  error (poor-decisions §19).
- **Handlers are tables keyed by ID**, so `notify = ["restart_app"]`
  cross-references a real symbol. Renaming a handler is a refactor, not
  a string-search.
- **Control-flow tasks** (`flush_handlers`, `end_play`, `end_host`,
  `end_batch`, `end_role`, `reset_connection`, `clear_facts`,
  `clear_host_errors`, `refresh_inventory`) live under a typed `control`
  key — never the dumping-ground `meta:` (poor-decisions §17).
- **`set_fact`** in TOML defaults to **shadowing**; `set_fact!` is the
  mutation form (poor-decisions §4). The exclamation in a TOML key is
  legal as a quoted string: `"set_fact!" = { ... }`. The lint warns on
  every use.

### 3.3 Task — every supported keyword

```toml
[[plays.tasks]]
# --- Identity ---
name = "Drop a file"
id   = "drop_file"             # optional stable ID; defaults to a stable hash of name + path
tags = ["files"]
register = "result"            # the registered variable name (typed Outcome of the module)

# --- Module call (exactly one) ---
copy = { src = "...", dest = "..." }
# OR (alternative form for dynamic action selection):
# action = { module = "runsible_builtin.copy", args = { src = "...", dest = "..." } }

# --- Conditional / loop / retry ---
when         = { expr = "ansible_facts.os_family == 'Debian'" }
loop         = ["a", "b", "c"]                                   # list expression
loop_control = { loop_var = "item", index_var = "i", label = "{{ item }}", pause_seconds = 0, extended = true, break_when = ["item == 'b'"] }
until        = { expr = "result.rc == 0" }
retries      = 5
delay_seconds = 10
failed_when  = { expr = "result.rc != 0" }                       # AND-joined when given a list of {expr=...}
changed_when = { expr = "result.stdout.contains('UPDATED')" }
# Renamed from Ansible — see poor-decisions §25 ("rename failed_when→success_predicate"). We keep
# `failed_when` for migration ergonomics but the lint nudges to `success_predicate`.

# --- Async / background (poor-decisions §24) ---
async      = { timeout = "5m" }    # foreground async; engine waits, with retries above
# OR
background = { id = "deploy_{{ inventory_hostname }}" }   # fire-and-forget; engine returns a job ref

# --- Execution context ---
connection = "ssh"
port = 22
remote_user = "deploy"
timeout_seconds = 30
delegate_to = "lb01.corp"
delegate_facts = false       # poor-decisions §18: explicit. Default false.
throttle = 1
no_log = false
# `become` here is the same typed sub-document as plays.become.

[plays.tasks.become]
enabled = true
user = "root"
method = "sudo"

# --- Notify (handler IDs only) ---
notify = ["restart_app", "reload_nginx"]

# --- Local task vars (highest in-play tier; see precedence §5) ---
[plays.tasks.vars]
local_only = "x"

# --- Per-task module defaults override ---
[plays.tasks.module_defaults."runsible_builtin.file"]
mode = "0600"

# --- Environment (remote) ---
[plays.tasks.environment]
RUST_LOG = "debug"
```

### 3.4 Handler

```toml
# Handlers live as tables keyed by ID. The ID is what `notify =` references.
[plays.handlers.restart_app]
listen = ["app_reload"]                # optional: topic group
when   = { expr = "ansible_check_mode == false" }   # handlers may have when
service = { name = "myapp", state = "restarted" }

# Handlers DO NOT support: loop, register, notify (chained handlers must be explicit).
```

### 3.5 Block

```toml
[[plays.tasks]]
name = "Apply config"
# Block-level keywords inherited by children: when, become, tags, environment, vars,
# delegate_to, throttle, run_once_in_batch, no_log, debugger, ignore_errors, ignore_unreachable.
when = { expr = "deploy_enabled | bool" }

[[plays.tasks.block]]                  # children
template = { src = "app.conf.j2", dest = "/etc/app/app.conf" }
notify = ["restart_app"]

[[plays.tasks.block]]
shell = { cmd = "validate-config" }
changed_when = { expr = "false" }

[[plays.tasks.rescue]]                 # runs only on first failure inside block
debug = { msg = "rolling back {{ ansible_failed_task.name }}" }

[[plays.tasks.always]]                 # always runs regardless
control = { action = "flush_handlers" }
```

### 3.6 Roles structure

```toml
# packages/nginx/runsible.toml
[package]
name = "nginx"
version = "0.3.1"

[[entry_points]]
# A role/package may expose multiple entry points (Ansible's tasks_from / handlers_from
# / vars_from collapse into "named entry points").
name = "main"
tasks = "tasks/main.toml"
handlers = "handlers/main.toml"
defaults = "defaults/main.toml"
vars = "vars/main.toml"

[[entry_points]]
name = "uninstall"
tasks = "tasks/uninstall.toml"

[argument_specs.main]
short_description = "Install and configure nginx"
[argument_specs.main.options.foo_port]
type = "int"
required = false
default = 80
choices = [80, 443]
description = "Port to bind"
[argument_specs.main.options.foo_host]
type = "str"
required = true

[dependencies]
common = "^1"        # SAT-resolved by runsible-galaxy at install time; locked in runsible.lock
```

When the playbook references this role:

```toml
[[plays.roles]]
name = "nginx"
entry_point = "main"     # default "main"; explicit when you need "uninstall" etc.
tags = ["web"]
[plays.roles.vars]
foo_port = 443
```

### 3.7 The `[imports]` block (poor-decisions §22)

```toml
# Top of every playbook file (and every role's tasks file).
[imports]
copy = "runsible_builtin.copy"
template = "runsible_builtin.template"
my_thing = "mycorp_platform.my_thing"
# Aliases are LEXICAL — they apply only to the file declaring them.
# No "collections:" implicit search list exists. Every reference to a module by short name
# must be in [imports] or it is a parse error.
```

### 3.8 The `[tags]` enum block (poor-decisions §19)

```toml
# runsible.toml (project root)
[project]
name = "ops"
version = "0.1.0"

[tags]
release = { description = "code rollouts" }
hotfix  = { description = "out-of-band emergency change" }
cleanup = { description = "post-deploy cleanup" }
audit   = { description = "compliance scans" }
# Plus the four built-ins: always, never, untagged, all (always declared, cannot be redefined).

[settings]
forks = 20
templating = "minijinja"           # see §10
output = { mode = "auto", schema_version = "v1" }   # "auto" | "ndjson" | "pretty"
```

Using `--tag whatever` for an undeclared tag exits non-zero with
`error: tag 'whatever' is not declared in [tags]; valid tags: ...`.

### 3.9 The `[plays.rollout]` typed sub-document (poor-decisions §21)

```toml
[plays.rollout]
batches = [1, 5, "20%"]
order = { shuffle = { seed = 42 } }     # "inventory" | "reverse_inventory" | "sorted" | "reverse_sorted" | { shuffle = { seed = N } }
max_fail_percentage = 10
```

Behaviorally:

- `batches = [1, 5, "20%"]` runs the play on 1 host, then 5 hosts, then
  20% of remaining hosts per batch until all targeted hosts are done.
- `order = "inventory"` (default) walks hosts in inventory definition
  order. `shuffle` requires a seed — there is **no unseeded shuffle**;
  this is a deliberate break with Ansible to make incident reproduction
  feasible.
- `max_fail_percentage` evaluates per batch. Once exceeded, the engine
  finishes the in-flight task across the batch, then halts the play.

### 3.10 The `[plays.become]` typed sub-document (poor-decisions §16)

```toml
[plays.become]
enabled = true
user = "root"
method = "sudo"            # enum

[plays.become.sudo]
flags = ["-H", "-S", "-n"]
preserve_env = ["HOME", "USER"]
password = { from_keyring = "runsible:sudo:prod" }    # NEVER plaintext
# Alternatives:
#   password = { from_env = "RUNSIBLE_SUDO_PASSWORD" }   # for CI
#   password = { from_vault = "secrets/prod.runvault#sudo_password" }

[plays.become.su]
flags = ["-l"]
prompt_pattern = "^Password: "

[plays.become.runas]
logon_type = "interactive"
logon_flags = ["with_profile"]
```

A pre-flight check confirms become works on every targeted host before
any task runs. If `enabled = true` and the connection user is already
the target user, the engine short-circuits — no needless `sudo -u root`
call (poor-decisions §16, §2.7 of -09).

### 3.11 `set_fact` shadowing vs `set_fact!` mutation (poor-decisions §4)

```toml
# Shadowing — introduces a new value visible in subsequent tasks of the
# current scope (block, then play). Does not mutate prior bindings.
[[plays.tasks]]
name = "Compute build id"
set_fact = { build_id = "{{ now('%s') }}" }     # the new build_id shadows any prior one

# Mutation — explicit `!`. The lint warns on every use.
[[plays.tasks]]
name = "Mutate counter"
"set_fact!" = { counter = "{{ counter | int + 1 }}" }
```

There is no `cacheable: true`. Persistent fact storage is opt-in via
`runsible-fact-store` (a separate subcommand) — the engine itself keeps
fact state in-process for the duration of the run.

### 3.12 The `[plays.let]` block

```toml
# Computed once per host before tasks start. Read-only for the rest of the play.
# Replaces the "first task is a set_fact to compute X" pattern.
[plays.let]
release_id   = "{{ now('%Y%m%d_%H%M%S') }}"
artifact_url = "https://artifacts/{{ app_version }}.tar.gz"
needs_reboot = "{{ ansible_facts.kernel != cached_kernel | default('') }}"
```

Resolution order: `let` bindings see play `vars`, `vars_files`, gathered
facts, and inventory vars. They run after fact gathering but before the
first task. Other `let` bindings cannot cross-reference each other —
that is a parse-time cycle check.

### 3.13 An ~80-line example playbook exercising every major feature

```toml
# playbooks/site.toml
schema = "runsible.playbook.v1"

[imports]
copy     = "runsible_builtin.copy"
template = "runsible_builtin.template"
service  = "runsible_builtin.service"
apt      = "runsible_builtin.apt"
get_url  = "runsible_builtin.get_url"
unarchive = "runsible_builtin.unarchive"
debug    = "runsible_builtin.debug"
assert   = "runsible_builtin.assert"
shell    = "runsible_builtin.shell"

# ----- The play -----
[[plays]]
name = "Update web tier"
hosts = ["webservers", "&production"]
remote_user = "deploy"
strategy = "linear"
throttle = 4
force_handlers = true
tags = ["release", "web"]

[plays.facts]
required = ["distribution", "kernel", "network"]

[plays.rollout]
batches = [1, "20%"]
order = "inventory"
max_fail_percentage = 10

[plays.become]
enabled = true
user = "root"
method = "sudo"
[plays.become.sudo]
flags = ["-H", "-S", "-n"]
password = { from_keyring = "runsible:sudo:prod" }

[plays.let]
release_id = "{{ now('%Y%m%d_%H%M%S') }}"
artifact_url = "https://artifacts/{{ app_version }}.tar.gz"

[plays.vars]
app_version = "1.4.2"

[plays.vars_files]
files = ["vars/secrets.runvault"]

[plays.module_defaults."runsible_builtin.file"]
owner = "app"
group = "app"
mode = "0640"

[plays.environment]
HTTPS_PROXY = "{{ corp_proxy | default('') }}"

# Pre-tasks
[[plays.pre_tasks]]
name = "Drain from LB"
delegate_to = "lb01.corp"
shell = { cmd = "lb-drain {{ inventory_hostname }}" }

# Roles
[[plays.roles]]
name = "common"
[[plays.roles]]
name = "webserver"
tags = ["web"]
[plays.roles.vars]
port = 8080

# Tasks
[[plays.tasks]]
name = "Validate inputs"
assert = { that = ["app_version is defined", "app_version is match('^[0-9]+\\.[0-9]+\\.[0-9]+$')"] }

[[plays.tasks]]
name = "Pull artifact"
register = "dl"
until = { expr = "dl is succeeded" }
retries = 5
delay_seconds = 10
get_url = { url = "{{ artifact_url }}", dest = "/tmp/app.tgz", checksum = "sha256:{{ app_sha256 }}" }

[[plays.tasks]]
name = "Apply config (block + rescue + always)"
[[plays.tasks.block]]
template = { src = "app.conf.j2", dest = "/etc/app/app.conf", validate = "/usr/sbin/nginx -t -c %s" }
notify = ["restart_app"]
[[plays.tasks.block]]
shell = { cmd = "validate-config" }
changed_when = { expr = "false" }
[[plays.tasks.rescue]]
debug = { msg = "rolling back {{ ansible_failed_task.name }}" }
[[plays.tasks.rescue]]
copy = { src = "/etc/app/app.conf.bak", dest = "/etc/app/app.conf", remote_src = true }
[[plays.tasks.always]]
control = { action = "flush_handlers" }

[[plays.tasks]]
name = "Per-service restart loop"
loop = ["nginx", "php-fpm"]
loop_control = { loop_var = "svc", label = "{{ svc }}" }
service = { name = "{{ svc }}", state = "restarted" }

# Post-tasks
[[plays.post_tasks]]
name = "Add back to LB"
delegate_to = "lb01.corp"
shell = { cmd = "lb-enable {{ inventory_hostname }}" }

# Handlers (typed IDs)
[plays.handlers.restart_app]
listen = ["app_reload"]
service = { name = "myapp", state = "restarted" }

[plays.handlers.reload_nginx]
listen = ["app_reload"]
service = { name = "nginx", state = "reloaded" }
```

This single play exercises: imports, lazy facts, typed rollout, typed
become with keyring secrets, let bindings, vars + vars_files, module
defaults, environment, pre/post tasks, roles, assert, register + until +
retries, block + rescue + always, control flow (`flush_handlers`),
loops with loop_control, typed handlers with `listen`. Deliberately
omitted to keep readable: async, delegate_facts, run_once block,
explicit `set_fact!`, `[plays.run_once.host]`. All of those are covered
in §3.2–3.12 above.

---

## 4. Data model (Rust types)

The crate exposes its types from `runsible_playbook::ast` (parsed
form), `runsible_playbook::plan` (planned form), and
`runsible_playbook::events` (NDJSON event types). The shared `Module`
trait lives in `runsible_core` (see §9).

### 4.1 The AST

```rust
// runsible_playbook::ast

/// Top-level: one TOML file.
pub struct Playbook {
    pub schema: String,                    // "runsible.playbook.v1"
    pub source_path: PathBuf,
    pub imports: BTreeMap<String, ModuleId>,   // alias -> FQ module id
    pub plays: Vec<Play>,
}

pub struct Play {
    pub name: Option<String>,
    pub hosts: HostPattern,                // typed pattern, see §6 of -03
    pub remote_user: Option<String>,
    pub connection: Option<ConnectionKind>,
    pub port: Option<u16>,
    pub strategy: Strategy,
    pub throttle: Option<u32>,
    pub force_handlers: bool,
    pub tags: Vec<TagId>,                  // resolved against [tags] enum
    pub facts: FactSpec,
    pub rollout: Rollout,
    pub run_once: Option<RunOnce>,
    pub become_: Option<BecomeSpec>,
    pub let_bindings: BTreeMap<String, Expression>,   // names cannot cross-ref each other
    pub vars: BTreeMap<String, Value>,
    pub vars_files: Vec<VarsFileSpec>,
    pub module_defaults: ModuleDefaults,
    pub environment: BTreeMap<String, Expression>,
    pub pre_tasks: Vec<TaskNode>,
    pub roles: Vec<RoleInvocation>,
    pub tasks: Vec<TaskNode>,
    pub post_tasks: Vec<TaskNode>,
    pub handlers: BTreeMap<HandlerId, Handler>,
    pub debugger: DebuggerMode,
    pub no_log: bool,
    pub check_mode_force: Option<bool>,
    pub diff_force: Option<bool>,
    pub ignore_unreachable: bool,
}

/// A `TaskNode` is either a leaf task or a block.
pub enum TaskNode {
    Task(Task),
    Block(Block),
}

pub struct Task {
    pub id: TaskId,                        // stable hash if not user-specified
    pub name: Option<String>,
    pub action: ResolvedAction,            // module + args
    pub when: Option<Predicate>,
    pub loop_: Option<LoopSpec>,
    pub until: Option<Predicate>,
    pub retries: u32,
    pub delay_seconds: f32,
    pub failed_when: Option<Predicate>,
    pub changed_when: Option<Predicate>,
    pub register: Option<String>,
    pub notify: Vec<HandlerId>,            // resolved at parse time; typo = parse error
    pub tags: Vec<TagId>,
    pub timeout_seconds: Option<u32>,
    pub async_: Option<AsyncSpec>,
    pub background: Option<BackgroundSpec>,
    pub no_log: bool,
    pub run_once: bool,                    // task-level still permitted but lint discourages it
    pub throttle: Option<u32>,
    pub delegate_to: Option<HostPattern>,
    pub delegate_facts: bool,
    pub become_: Option<BecomeSpec>,
    pub connection: Option<ConnectionKind>,
    pub port: Option<u16>,
    pub remote_user: Option<String>,
    pub vars: BTreeMap<String, Value>,
    pub module_defaults: ModuleDefaults,
    pub environment: BTreeMap<String, Expression>,
    pub check_mode_force: Option<bool>,
    pub diff_force: Option<bool>,
}

pub struct Block {
    pub block: Vec<TaskNode>,
    pub rescue: Vec<TaskNode>,
    pub always: Vec<TaskNode>,
    // Inherited keywords (not the action-bearing ones):
    pub when: Option<Predicate>,
    pub tags: Vec<TagId>,
    pub become_: Option<BecomeSpec>,
    pub environment: BTreeMap<String, Expression>,
    pub vars: BTreeMap<String, Value>,
    pub module_defaults: ModuleDefaults,
    pub connection: Option<ConnectionKind>,
    pub remote_user: Option<String>,
    pub no_log: bool,
    pub ignore_errors: bool,
    pub ignore_unreachable: bool,
    pub delegate_to: Option<HostPattern>,
    pub throttle: Option<u32>,
    pub debugger: DebuggerMode,
}

pub struct Handler {
    pub id: HandlerId,
    pub name: Option<String>,
    pub listen: Vec<String>,
    pub when: Option<Predicate>,
    pub action: ResolvedAction,
    pub no_log: bool,
}

/// A resolved (module, args) pair.
pub struct ResolvedAction {
    pub module_id: ModuleId,
    pub args: ModuleArgs,                  // typed against the module's input schema
}

pub struct Predicate {
    pub conjuncts: Vec<Expression>,        // implicitly AND-joined
}

pub enum Strategy { Linear, Free, HostPinned }

pub enum ConnectionKind { Ssh, Local, Docker, Kubectl, Podman, Russh }

pub enum DebuggerMode { Always, Never, OnFailed, OnUnreachable, OnSkipped, OnOk }
```

### 4.2 The Plan

```rust
// runsible_playbook::plan

pub struct Plan {
    pub project_hash: ContentHash,
    pub host_plans: Vec<HostPlan>,
    pub tag_resolution: TagResolution,    // what tags actually got selected
}

pub struct HostPlan {
    pub host: HostId,
    pub batches: Vec<BatchPlan>,
}

pub struct BatchPlan {
    pub batch_index: usize,
    pub plays: Vec<PlayPlan>,
}

pub struct PlayPlan {
    pub play_index: usize,
    pub tasks: Vec<TaskPlan>,
    pub handlers: Vec<HandlerPlan>,
    pub max_fail_percentage: u8,
}

pub struct TaskPlan {
    pub task_id: TaskId,
    pub action: ResolvedAction,
    pub vars_resolved: BTreeMap<String, Value>,
    pub when: Option<Predicate>,
    pub loop_iterations: Option<Vec<Value>>,    // None = single-shot, Some = list
    pub retry_policy: RetryPolicy,
    pub timeout: Duration,
    pub run_once: bool,
    pub delegate: Option<DelegateSpec>,
    pub become_: Option<BecomeSpec>,
    pub connection_target: ConnectionTarget,
    pub module_plan: Option<ModuleSpecificPlan>,   // populated by the module's plan() call
}
```

### 4.3 Events (NDJSON wire format)

```rust
// runsible_playbook::events

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    RunStarted    { schema: &'static str, run_id: Uuid, project_hash: ContentHash, started_at: DateTime<Utc> },
    PlanComputed  { run_id: Uuid, host_count: usize, batches: usize, tag_resolution: TagResolution },
    BatchStarted  { run_id: Uuid, batch_index: usize, hosts: Vec<HostId> },
    PlayStarted   { run_id: Uuid, play_index: usize, name: Option<String> },
    FactsGathered { run_id: Uuid, host: HostId, subsets: Vec<String>, fact_count: usize, duration_ms: u64 },
    TaskStarted   { run_id: Uuid, task_id: TaskId, host: HostId, name: Option<String>, module: ModuleId },
    TaskPlanned   { run_id: Uuid, task_id: TaskId, host: HostId, plan_summary: serde_json::Value },
    TaskApplied   { run_id: Uuid, task_id: TaskId, host: HostId, outcome: TaskOutcome, duration_ms: u64 },
    TaskVerified  { run_id: Uuid, task_id: TaskId, host: HostId, idempotent: bool },
    TaskSkipped   { run_id: Uuid, task_id: TaskId, host: HostId, reason: SkipReason },
    HandlerFired  { run_id: Uuid, handler_id: HandlerId, host: HostId, outcome: TaskOutcome },
    HostFailed    { run_id: Uuid, host: HostId, task_id: TaskId, reason: FailureReason },
    HostUnreachable { run_id: Uuid, host: HostId, task_id: TaskId, reason: String },
    BatchEnded    { run_id: Uuid, batch_index: usize, summary: BatchSummary },
    PlayEnded     { run_id: Uuid, play_index: usize, summary: PlaySummary },
    RunEnded      { run_id: Uuid, summary: RunSummary, exit_code: i32, ended_at: DateTime<Utc> },
    Diagnostic    { run_id: Uuid, level: Level, message: String, source: Option<String> },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutcome { Ok, Changed, Failed, Skipped, Unreachable }
```

The NDJSON schema is **versioned** (`schema = "runsible.event.v1"`).
v2 will be additive (new variants); v1 events keep the same field
shape for the lifetime of v1.

### 4.4 The Module trait (lives in `runsible_core`, used by every module crate and by this engine)

```rust
// runsible_core::module

#[async_trait]
pub trait Module: Send + Sync + 'static {
    type Input: serde::de::DeserializeOwned + serde::Serialize + JsonSchema;
    type Plan:  serde::Serialize + serde::de::DeserializeOwned + Diff + Send + Sync;
    type Outcome: serde::Serialize + serde::de::DeserializeOwned + Send + Sync;

    /// Stable identifier (`"runsible_builtin.copy"`).
    fn id() -> ModuleId where Self: Sized;

    /// Compute what would change. Empty plan => idempotent no-op; `apply` is skipped.
    async fn plan(
        &self,
        input: &Self::Input,
        ctx: &PlanContext<'_>,
    ) -> Result<Self::Plan, ModuleError>;

    /// Apply the plan. Mutates the host. Engine calls only if plan is non-empty.
    async fn apply(
        &self,
        plan: &Self::Plan,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Self::Outcome, ModuleError>;

    /// Re-plan against the post-apply state. Engine asserts the result is empty
    /// for modules that opt in to verification (idempotency proof, poor-decisions §9).
    async fn verify(
        &self,
        post: &Self::Plan,
        ctx: &PlanContext<'_>,
    ) -> Result<(), ModuleError> {
        // Default impl: re-plan and check for emptiness.
        if post.is_empty() { Ok(()) } else { Err(ModuleError::NotIdempotent) }
    }

    /// Whether `verify()` should be skipped (true for `command`, `shell`, `raw`, `script`).
    fn verify_idempotent() -> bool where Self: Sized { true }

    /// Whether the module supports check mode.
    fn supports_check_mode() -> bool where Self: Sized { true }

    /// Whether the module supports diff output.
    fn supports_diff() -> bool where Self: Sized { false }

    /// Args that should be redacted in event output (poor-decisions §25).
    fn sensitive_args() -> &'static [&'static str] where Self: Sized { &[] }
}
```

### 4.5 How `plan() / apply() / verify()` connects

Per task, per host, per loop iteration, the engine performs:

```
plan_ctx = PlanContext { vars, facts, host_state, settings };
plan = module.plan(&input, &plan_ctx).await?;
emit(TaskPlanned { plan_summary: plan.summary() });

if plan.is_empty() {
    emit(TaskSkipped { reason: NoChangeRequired });
    register_outcome = Outcome::ok_no_change();
} else if check_mode {
    emit(TaskApplied { outcome: Changed, duration_ms: 0 });   // simulated
    register_outcome = Outcome::simulated_change(plan);
} else {
    let outcome = module.apply(&plan, &mut apply_ctx).await?;
    emit(TaskApplied { outcome, duration_ms });

    if module.verify_idempotent() && verify_enabled {
        let after_plan = module.plan(&input, &plan_ctx).await?;
        if after_plan.is_empty() {
            emit(TaskVerified { idempotent: true });
        } else {
            emit(TaskVerified { idempotent: false });
            // record violation; if --strict-idempotence, fail the host
        }
    }
}
```

This gives us cheap dry-run (just `plan()`), real idempotence proof
(verify), and a uniform diff surface (the `Diff` trait on every
`Plan`).

---

## 5. Execution model

End-to-end, what happens when a user runs:

```
runsible-playbook playbooks/site.toml -i inventory/ -e env=prod
```

### 5.1 Boot (target: under 100 ms before any I/O)

1. Static linker, no Python. Single binary.
2. Parse argv with `clap`. `--version`, `--help`, `--syntax-check`,
   `--list-tags`, etc., short-circuit before anything else.
3. Load `runsible-config` (`runsible.toml` + env vars + CLI overrides).
   This is fast; it's TOML.
4. Open the inventory via `runsible-inventory::Inventory::load(sources)`.
5. Open the package store via `runsible-core::PackageStore::open()`
   (mmap'd, lazy).

### 5.2 Parse + type-check

6. Read the playbook file(s) on the command line. For each, pull in
   imports, role tasks, role handlers, role defaults — all TOML, all
   parsed eagerly.
7. Build the AST (`Playbook`). Resolve imports, handler IDs, tag
   references, role references, module references.
8. **Type-check**:
   - Every `task.action` resolves to a known module (consulting the
     module catalog, which is compiled in for builtins and discovered
     via `PackageStore` for installed packages).
   - Every module's `Input` schema is satisfied by the provided args.
   - Every var reference (`{{ foo.bar }}`) is reachable in the static
     var scope (with limited inference for facts that the play declares
     it requires).
   - Every tag is in the project's `[tags]` enum.
   - Every `notify = [...]` references a defined handler in the play
     (or a `listen` topic that resolves to at least one handler).
   - Every `delegate_to` references either a host in inventory or a
     name that `add_host` could plausibly create (with a lint warning
     if not in inventory).
   - Every `let` binding is acyclic.
9. If any type-check fails: print all errors with line/column and exit
   `4` (parser error). **No host is touched.**

### 5.3 Resolve roles + imports

10. Roles are flattened in dependency order (DFS) into the play's task
    list. Each role contributes its tasks, handlers, and var bindings.
    Handler IDs from a role are namespaced as `<package>.<handler_id>`
    to avoid collisions (poor-decisions §13).
11. `vars_files` are loaded; vault-encrypted files are decrypted via
    `runsible-vault` lazily — i.e., only if the play actually references
    a key from them.

### 5.4 Compute per-host work

12. Resolve the play's `hosts` pattern against inventory. The result
    is a `Vec<HostId>` (the play's host set).
13. Apply `--limit` from the CLI (also a typed `HostPattern`).
14. Compute batches from `[plays.rollout]`. The result is
    `Vec<Vec<HostId>>` (batches in execution order).
15. For each host in each batch, compute a `HostPlan` containing
    `BatchPlan -> PlayPlan -> TaskPlan` entries. Variables are resolved
    per host (because facts and let bindings are per-host).

### 5.5 Run the plan

16. **Outer loop:** for each batch (sequential — batches do not overlap).
17. **Inner loop:** for each host in the batch (parallel up to
    `forks` and `throttle`). The strategy decides whether tasks
    barrier-sync between hosts (`linear`) or run free (`free`).
18. **Per host:**
    - Open / reuse the connection (poor-decisions §8: defaults to
      system OpenSSH with ControlPersist; per-host pool keyed by
      `(plugin, user, host, port)`).
    - Pre-flight become check (one-time per host per run).
    - Gather only the required fact subsets (poor-decisions §12).
      Subsets are computed by static analysis of the playbook — the
      lint warns if `facts.required` lists subsets the play does not
      reference, or omits subsets the play does reference.
    - Walk the task list. For each task:
      - Render variables.
      - Evaluate `when`. If false, emit `TaskSkipped` and move on.
      - For each loop iteration (or once if no loop):
        - Call `module.plan()`. Emit `TaskPlanned`.
        - If `--check`: emit a simulated `TaskApplied` with the plan
          summary; do not call `apply()`.
        - Else: call `module.apply()`. Emit `TaskApplied`.
        - If verify is enabled and the module opts in: call
          `module.plan()` again; emit `TaskVerified`.
        - Update `register` and host state (vars, facts, etc.).
        - On failure: walk to `rescue` if inside a block, else mark
          host failed, emit `HostFailed`, and skip the rest of the play
          on this host (unless `ignore_errors`).
        - On notify: queue handler IDs in the per-host notify set.
    - At explicit flush points (`flush_handlers`, end-of-block,
      end-of-play, end of `pre_tasks`/`roles`/`tasks`/`post_tasks`):
      run notified handlers in **definition order**, deduplicated by ID.
      Emit `HandlerFired`.

### 5.6 Wrap-up

19. On batch completion: emit `BatchEnded` with per-host status counts.
    Check `max_fail_percentage`; if exceeded, mark the play aborted and
    skip subsequent batches.
20. On play completion: emit `PlayEnded`.
21. On run completion: emit `RunEnded` with the final summary and the
    exit code:
    - `0` — every targeted host succeeded.
    - `2` — at least one task failure.
    - `3` — at least one unreachable host (no failures).
    - `4` — parser/type error before any execution.
    - `5` — bad CLI option.
    - `8` — interrupted (Ctrl-C) during a play.
    - `99` — user-interrupted (Ctrl-C during interactive prompt).

### 5.7 Lockfile-backed reproducible mode (M3)

When run with `--locked`, the engine:
- Verifies that the project hash matches `runsible.lock.project_hash`.
- Verifies that resolved package versions match `runsible.lock.packages`.
- After planning, computes per-host plan hashes and checks they match
  `runsible.lock.host_plans[host]` (for hosts whose plan is recorded —
  drift hosts are reported via `Diagnostic`).
- After applying, writes a signed run record (Ed25519 by default;
  hardware key via PKCS#11 optional) under
  `.runsible/run-records/<run_id>.json`.

This is the P3 (compliance) artifact (user-stories §2 P3).

---

## 6. CLI surface

`runsible-playbook` is the runsible counterpart of `ansible-playbook`.
We mirror the muscle memory wherever sane (user-stories §10 close), and
diverge surgically where the redesigns require it.

### 6.1 Synopsis

```
runsible-playbook [GLOBAL OPTIONS] [-i INVENTORY] [-l SUBSET] [-e EXTRA_VARS]
                  [--vault-id VAULT_IDS] [-f FORKS] [--check] [--diff]
                  [--syntax-check] [--list-tasks] [--list-tags] [--list-hosts]
                  [--start-at-task NAME] [--step]
                  [--tags TAGS] [--skip-tags TAGS] [--force-handlers]
                  [--strategy NAME] [--connection NAME] [--remote-user USER]
                  [--port PORT] [--timeout SECS] [--task-timeout SECS]
                  [--private-key PATH] [--become] [--become-method M]
                  [--become-user U]
                  [--plan-only] [--locked] [--strict-idempotence]
                  [--output {auto,ndjson,pretty}] [--output-file FILE]
                  [-v]...
                  PLAYBOOK [PLAYBOOK ...]
```

### 6.2 Flag table (with type / default / env / origin)

| Flag | Type | Default | Env | Origin / notes |
|---|---|---|---|---|
| `-i, --inventory PATH` | repeat | `inventory/` (project default), `/etc/runsible/hosts` (system fallback) | `RUNSIBLE_INVENTORY` | Mirrors `ansible-playbook -i`. Comma-list also accepted. |
| `-l, --limit SUBSET` | str | none | `RUNSIBLE_LIMIT` | Same syntax as `ansible-playbook --limit`. |
| `--list-hosts` | bool | false | — | Prints the hosts the playbook would target, then exits. |
| `--flush-cache` | bool | false | — | Drops the fact cache for every targeted host. |
| `-c, --connection NAME` | enum | from inventory/play | `RUNSIBLE_CONNECTION` | `ssh\|local\|docker\|kubectl\|podman\|russh`. |
| `-u, --remote-user USER` | str | from inventory/play | `RUNSIBLE_REMOTE_USER` | |
| `-T, --timeout SECS` | int | 30 | `RUNSIBLE_TIMEOUT` | TCP connect timeout. (renamed from Ansible's 10s default — too aggressive for cloud) |
| `--task-timeout SECS` | int | 0 (unlimited) | `RUNSIBLE_TASK_TIMEOUT` | Per-task wall-clock cap. |
| `--port PORT` | int | per-plugin default | `RUNSIBLE_PORT` | |
| `--private-key PATH` | path | `~/.ssh/id_*` (ssh-agent first) | `RUNSIBLE_PRIVATE_KEY_FILE` | |
| `--ssh-common-args STR` | str | "" | `RUNSIBLE_SSH_COMMON_ARGS` | Pass-through to system ssh. |
| `--ssh-extra-args STR` | str | "" | — | |
| `--scp-extra-args STR` | str | "" | — | |
| `--sftp-extra-args STR` | str | "" | — | |
| `-k, --connection-password-file PATH` | path | none | `RUNSIBLE_CONNECTION_PASSWORD_FILE` | Mutually exclusive with `--connection-password-keyring`. **`-k` is renamed: it no longer prompts.** Prompting in 2026 is a CI footgun; we replace `-k` with the file form, and add `--connection-password-keyring` for the modern path. |
| `--connection-password-keyring KEY` | str | none | `RUNSIBLE_CONNECTION_PASSWORD_KEYRING` | NEW. Read connection password from system keyring. |
| `-b, --become` | bool | from play | `RUNSIBLE_BECOME` | |
| `--become-method NAME` | enum | `sudo` | `RUNSIBLE_BECOME_METHOD` | `sudo\|su\|doas\|machinectl\|runas\|enable`. |
| `--become-user USER` | str | `root` | `RUNSIBLE_BECOME_USER` | |
| `--become-password-file PATH` | path | none | `RUNSIBLE_BECOME_PASSWORD_FILE` | Mutually exclusive with `--become-password-keyring`. |
| `--become-password-keyring KEY` | str | none | `RUNSIBLE_BECOME_PASSWORD_KEYRING` | NEW. Default channel for v1. |
| `-K, --ask-become-pass` | bool | false | — | KEPT for muscle memory but **deprecation-warned**: prompts only if stdin is a TTY; CI fails. |
| `-e, --var KEY=VAL` | repeat | none | — | `key=value`, JSON, or `@file.toml`. (Renamed from `--extra-vars` to `--var` — extras-vs-locals distinction obsolete in our 5-tier precedence model.) |
| `--vars-file PATH` | repeat | none | — | NEW. Load a TOML file of vars at the runtime layer. |
| `--vault-id ID` | repeat | from project | `RUNSIBLE_VAULT_IDS` | runsible-vault identity, format `label@source`. |
| `--vault-password-file PATH` | path | from project | `RUNSIBLE_VAULT_PASSWORD_FILE` | Migration-only; `--vault-recipient` is the v1 native. |
| `-J, --ask-vault-pass` | bool | false | — | KEPT, deprecation-warned. |
| `-f, --forks N` | int | 20 (was 5 in Ansible) | `RUNSIBLE_FORKS` | We default higher; modern hardware can do 20 SSH sessions trivially. |
| `--strategy NAME` | enum | `linear` | `RUNSIBLE_STRATEGY` | `linear\|free\|host_pinned`. |
| `-t, --tags T1,T2` | repeat | none | `RUNSIBLE_TAGS` | Tags must be declared in `[tags]`. |
| `--skip-tags T1,T2` | repeat | none | `RUNSIBLE_SKIP_TAGS` | |
| `--list-tasks` | bool | false | — | Print tasks per host (post-resolution); exit. |
| `--list-tags` | bool | false | — | Print declared tags + which the playbook uses; exit. |
| `--syntax-check` | bool | false | — | Parse + type-check; exit. |
| `--start-at-task NAME` | str | none | — | Skip tasks until one whose name matches. Static imports only; with our model "compose" is always static, so this works for everything. |
| `--step` | bool | false | — | Interactive: confirm each task. Refuses on non-TTY. |
| `--force-handlers` | bool | from play | `RUNSIBLE_FORCE_HANDLERS` | |
| `-C, --check` | bool | false | `RUNSIBLE_CHECK` | Run `plan()` only; never `apply()`. |
| `-D, --diff` | bool | false | `RUNSIBLE_DIFF` | Render the `Diff` of each module's plan. |
| `--plan-only` | bool | false | — | NEW. Like `--check` but emits a single consolidated `Plan` artifact (TOML) per host instead of per-task events. The artifact is the input to `runsible-plan apply`. |
| `--locked` | bool | false | — | NEW. Refuse to run if `runsible.lock` is missing or stale. |
| `--strict-idempotence` | bool | false | — | NEW. Fail the host on any `verify()` violation. |
| `--output MODE` | enum | `auto` | `RUNSIBLE_OUTPUT` | `auto\|ndjson\|pretty`. `auto` picks `ndjson` if stdout is not a TTY, `pretty` otherwise. |
| `--output-file PATH` | path | stdout | `RUNSIBLE_OUTPUT_FILE` | NEW. Write events to a file in addition to stdout. |
| `-v, -vv, -vvv` | counter | 0 | `RUNSIBLE_VERBOSITY` | Up to `-vvvv`. |
| `-h, --help`, `--version` | — | — | — | |

### 6.3 Dropped vs Ansible

- `-M, --module-path` — we have no plugin search path; modules live in
  packages resolved by `runsible-galaxy`. (Drop)
- `-B, --background SECONDS`, `-P, --poll INTERVAL` — applicable only
  to the ad-hoc binary; `runsible-playbook` uses the typed `async` /
  `background` task fields instead. (Drop)
- `-o, --one-line`, `-t TREE` — output formatting handled by
  `--output` and `--output-file`. (Drop)
- `--collections-path` — there are no Galaxy collections in our model;
  packages live in `runsible-galaxy`'s store. (Drop)
- `--playbook-dir` — runsible-playbook always knows its playbook dir
  from the positional argument. (Drop)

### 6.4 Renamed vs Ansible

- `--extra-vars` → `--var`. Extras vs locals collapse in our 5-tier model.
- `--list_files` (`ansible-doc` style underscore) — we use `--list-files`
  consistently (this is for `runsible-doc` not us, but worth flagging).
- `-K`/`-k`/`-J` (interactive password prompts) — KEPT names, but they
  are deprecation-warned and refuse on non-TTY.

### 6.5 Added vs Ansible

- `--connection-password-keyring`, `--become-password-keyring`,
  `--vars-file`, `--plan-only`, `--locked`, `--strict-idempotence`,
  `--output`, `--output-file`. Each one is justified by a redesign in
  poor-decisions or a JTBD in the user-stories memo.

---

## 7. Redesigns vs Ansible (apply by §-number)

This is where most of the redesigns from `11-poor-decisions.md` actually
land — `runsible-playbook` is the engine that enforces them. Each
subsection cites the source.

### 7.1 §1 YAML → TOML

The engine consumes only TOML. There is no fallback YAML path. The
`yaml2toml` crate is a separate, one-shot translator; if a user has
YAML, they convert it once and review the diff. The engine never sees
YAML.

### 7.2 §2 Jinja2 → MiniJinja with a fixed catalog

See §10 below for the full filter/test catalog.

### 7.3 §3 22-level precedence → 5 levels

The engine implements exactly 5 ordered tiers (lowest → highest):

1. **Project defaults** — `runsible.toml [defaults]` plus role
   `defaults/main.toml`.
2. **Inventory** — host vars, group vars, with explicit parent → child
   inheritance.
3. **Playbook** — play `vars`, play `vars_files`, block `vars`, task
   `vars`, role `vars/main.toml`.
4. **Runtime** — `--var key=val`, `--vars-file path.toml`.
5. **Set-facts** — `set_fact` / `set_fact!`, registered vars.

Within tier 3, declaration order matters (block beats play, task beats
block, role `vars/` beats role `defaults/`). The CLI ships
`runsible explain-var <name> --host H --task T` to print which tier
won.

A `--precedence-compat ansible` mode exists for transition (one major
version), implementing the original 22 layers behind a feature flag.

### 7.4 §4 set_fact mutation → shadowing + `let`

Implemented in §3.11 above. The engine treats `set_fact` as creating a
new scope; the value is visible to all subsequent tasks in the
enclosing scope but does not mutate prior bindings. `set_fact!`
performs in-place mutation; the lint warns. `[plays.let]` provides
read-only computed bindings (poor-decisions §4).

### 7.5 §9 Idempotency by convention → typed module trait

Implemented in §4.4. Modules implement `plan() / apply() / verify()`.
`apply()` is skipped when `plan()` is empty. `verify()` re-plans and
asserts emptiness. `command`, `shell`, `raw`, `script` opt out of
verify with `verify_idempotent = false`; the lint warns on every use.

### 7.6 §10 Output → NDJSON default

Implemented in §4.3 above. `runsible.event.v1` is the schema; pretty
printer is a separate consumer of the same NDJSON stream.

### 7.7 §12 Lazy facts

Implemented in §5.5 step 4. The engine runs the `setup` module with
exactly the subsets the play declares (`[plays.facts] required = [...]`).
The lint warns on (a) facts referenced but not in `required`, and (b)
facts in `required` but never referenced.

A static analyzer (`runsible-lint` reusing the same parser) infers the
required subsets from `when:`, `loop:`, and template var references.
The user can override.

### 7.8 §13 Handler typos → typed handler IDs

Implemented in §3.4 and §4.1. Handlers are TOML tables keyed by ID.
`notify = ["restart_app"]` cross-references a real symbol. Renaming a
handler is a refactor. `listen = [...]` is still supported but is a
**list of typed topic IDs** declared at the project level (just like
tags are an enum).

Handlers run at explicit flush points (`flush_handlers`,
end-of-block, end-of-play, between `pre_tasks`/`roles`/`tasks`/`post_tasks`).
Default behavior on partial failure: handlers notified before the
failure run; opt out with `[strategy] handlers_on_failure = "skip"`.

### 7.9 §16 `become` ladder → typed sub-document

Implemented in §3.10. `[plays.become]` is a typed table with
method-specific sub-tables (`[plays.become.sudo]`, `[plays.become.su]`,
`[plays.become.runas]`). Passwords default to a system keyring; never
plaintext. A pre-flight check confirms become works on every targeted
host before any task runs.

### 7.10 §17 `meta:` → typed control flow

Implemented in §3.5 (the `control = { action = "..." }` form). Each
former `meta:` action is a documented enum variant with its own
preconditions and effects.

### 7.11 §18 `run_once` + `delegate_to` → cleanly separated

Implemented in §3.2 and §4.1.

- `[plays.run_once]` is a play-level table with an optional `host`
  field naming the delegate. The play runs exactly once on that host;
  the result is broadcast as a fact to every host in `plays.hosts`.
- `delegate_to` is a task-level field that swaps the connection
  target. Variable scoping stays on the original host. There is no
  other interaction.

The engine warns when both are set on a single task (the legacy
combination); the lint forbids it.

### 7.12 §19 Tags → enum

Implemented in §3.8. `[tags]` declares the project's tag enum. CLI
`--tag whatever` for an undeclared tag exits non-zero.

The engine emits an `EvaluatedTagSet` event before any task runs:

```json
{"type":"plan_computed","tag_resolution":{"include":["release"],"exclude":["never"],"effective_task_count":42}}
```

### 7.13 §21 `serial:` → typed `[plays.rollout]`

Implemented in §3.9. Shuffle requires a seed.

### 7.14 §22 `collections:` → lexical `[imports]`

Implemented in §3.7. Module references must use either the FQN
(`runsible_builtin.copy`) or an alias declared in `[imports]`. There is
no implicit collection search.

### 7.15 §24 async + poll → `async` vs `background`

Implemented in §3.3. `async = { timeout = "5m" }` is foreground async
(engine waits up to 5 m). `background = { id = "..." }` is fire-and-forget;
the engine emits a `BackgroundJobStarted` event, returns the job ID,
and `runsible-job` is a sister CLI to inspect / wait for it.

### 7.16 §25 (smaller items)

- `changed_when` / `failed_when`: typed predicates with documented
  field access on the registered Outcome (§4.4).
- `no_log`: declared at the module level via `Module::sensitive_args()`;
  the engine redacts those fields in events automatically. Per-task
  `no_log = true` is still honored for ad-hoc cases.
- `vars_prompt`: works only when stdin is a TTY; non-TTY runs error at
  parse time unless `default` is provided and `required = false`.
- `hash_behaviour = merge`: removed. Per-merge-site explicit syntax via
  `combine` filter / `merge_strategy` argument on relevant operations.
- `ANSIBLE_*` env vars: explicit per-key in our config schema; no
  blanket pattern. Each key has at most one `RUNSIBLE_*` env var.
- `register` writes to a typed Outcome slot.
- `include_*` vs `import_*`: collapsed to `compose` (always static,
  parsed at load time). Dynamic composition is opt-in via
  `compose_dynamic` and limited to handler dispatch — the engine
  refuses dynamic composition of a task list at runtime (it makes
  type-checking impossible).

### 7.17 §15 module_defaults — KEEP

Module defaults are useful and well-designed in Ansible. Implemented
in §3.2 as `[plays.module_defaults."runsible_builtin.file"] = { ... }`.
Action groups (`group/aws`) become typed module groups declared in the
`runsible.toml` `[module_groups]` table.

---

## 8. Milestones

### M0 — TOML parser + type checker + a single-task happy-path runner (~2 weeks)

- TOML schema parser: full AST emission for a *minimum* playbook (one
  play, one task, no roles, no handlers, no blocks).
- Type checker: module resolution against a hardcoded catalog of one
  module (`debug`).
- Module trait + `runsible_core` skeleton.
- One-shot `apply()` against `localhost` via the `local` connection.
- NDJSON event emission for `RunStarted`, `TaskStarted`, `TaskApplied`,
  `RunEnded`.
- Exit codes wired up.
- Smoke test: `runsible-playbook examples/hello.toml -i localhost,`
  emits an event stream and exits `0`.

### M1 — Plays + handlers + blocks + tags + loops + when + roles + 12-module library (~6 weeks)

- Full play + task + block + handler + role parser.
- 5-tier var precedence resolver.
- MiniJinja templating with the §10 filter/test catalog.
- Tags enum, `[tags]` block, `--tags` / `--skip-tags`.
- Loops (`loop`, `loop_control`), conditionals (`when`),
  retries (`until`/`retries`/`delay_seconds`), error handling
  (`block`/`rescue`/`always`).
- Lazy facts: `gather` task with `min`, `network`, `hardware`,
  `distribution` subsets.
- 12-module kit:
  `command, shell, copy, template, file, package (apt + dnf dispatch),
  service, systemd_service, debug, set_fact, assert, get_url`.
- yaml2toml golden-file harness consuming and round-tripping
  `geerlingguy.docker` and ten other top Galaxy roles.
- Acceptance: take an Ansible playbook, convert it via yaml2toml,
  run it via runsible-playbook, demonstrate a wall-clock improvement
  on a 50-host fleet.

### M2 — Strategies + delegation + async + run_once + checkmode + diff + plan-only mode (~6 weeks)

- `linear`, `free`, `host_pinned` strategies.
- `delegate_to`, `delegate_facts`, `[plays.run_once]`.
- `async = { timeout = "..." }` foreground async.
- `background = { id = "..." }` + sister `runsible-job` CLI.
- `--check` / `--diff` working full-fidelity through every module.
- `--plan-only` writing a per-host `Plan` artifact (TOML).
- `runsible-plan apply <plan.toml>` consuming the artifact.
- Check-mode tests: every module either fully supports check or is
  documented as `partial` in its module manifest.

### M3 — Lockfile + structured plan diffing + signed run records (~8 weeks)

- `runsible.lock` schema and writer (resolved package versions +
  per-host plan hashes).
- `--locked` mode: refuse to run on stale lock; warn on host plan
  drift.
- Plan diff format: a typed `PlanDiff` with file-level / property-level
  changes; renderable as text, JSON, and HTML.
- Signed run records: Ed25519 by default, PKCS#11 hardware key
  optional, written to `.runsible/run-records/<run_id>.json`.
- Verifier CLI (`runsible-record verify <run_id>`).
- This is the P3 / compliance segment artifact; ties to the
  user-stories §2 P3 ("DoD subcontractor" beachhead).

---

## 9. Dependencies on other crates

`runsible-playbook` is the consumer at the top of the dependency
graph. Concretely:

### 9.1 `runsible-core` (proposed new workspace member)

The shared `Module` trait, the `Outcome` type, the `Diff` trait, the
`HostId` / `TagId` / `HandlerId` / `ModuleId` newtypes, the
`PackageStore` (a mmap'd index into installed packages), the `Settings`
struct shape (filled by `runsible-config`), and the `Inventory` trait
shape (filled by `runsible-inventory`). Without `runsible-core`, the
engine and the modules can't even agree on a `register` shape.

This crate **does not yet exist** in the workspace. It is the first
thing M0 should land. Every module crate depends on `runsible-core`;
the engine depends on `runsible-core` plus the modules' implementations
(via the package store at runtime, plus a static catalog of builtins
linked at compile time).

### 9.2 `runsible-config`

Provides `Settings::load(args, env, files) -> Settings`. The engine
consumes `Settings` (forks, default strategy, output mode, fact-gather
defaults, become method default, templating engine choice). The engine
does not parse TOML config itself; that's `runsible-config`'s job.

### 9.3 `runsible-inventory`

Provides `Inventory::load(sources) -> Inventory` and
`Inventory::resolve_pattern(&HostPattern) -> Vec<HostId>`. The engine
calls these to materialize `Vec<HostId>` for each play and
`HostVars` for each host. The engine never touches inventory file
formats directly.

### 9.4 `runsible-vault`

Provides `Vault::decrypt(path, key_resolver) -> serde_json::Value`
(or TOML equivalent). The engine calls this lazily on `vars_files` that
are encrypted, and on inline `!runvault:...` strings. Key resolution
(file fallback / age recipients / SSH keys) is the vault's problem.

### 9.5 `runsible-connection`

Provides the `Connection` trait and concrete impls for each transport
(see §1.7 of `09-connection-templating-facts.md`). The engine treats
connections as opaque: open, exec, put, fetch, close. ControlMaster /
russh multiplexing details belong inside `runsible-connection`.

### 9.6 The module crates

Each module ships as a Rust crate (or workspace member) implementing
`runsible_core::Module`. The 12-module M1 kit lives in
`runsible-builtin-modules` (a workspace member with one sub-module
per module). External modules (post-v1) load via a stable C ABI
through a `Module`-trait dynamic loader; this is deferred.

### 9.7 Visualization

```
                       runsible-playbook (this crate)
                              |
        +---------------------+----------------------+
        |          |          |          |           |
runsible-config  runsible-  runsible-  runsible-  runsible-
                inventory   vault     connection   builtin-
                                                   modules
                              |
                         runsible-core
```

---

## 10. Templating decisions

### 10.1 Engine choice: MiniJinja

We pick **MiniJinja**, not Tera. Rationale:

- **Closer to Jinja2 surface than Tera.** MiniJinja explicitly aims for
  Jinja2 compatibility; Tera diverges (different `default` semantics,
  different test syntax, no `is`-test in some versions, etc.).
  yaml2toml output has the best chance of running on MiniJinja
  unmodified.
- **Custom filters / tests are easy to register.** The §3.4-§3.7
  catalog ports cleanly. Both engines support this, but MiniJinja's
  trait surface is simpler.
- **No custom loader gymnastics required.** runsible doesn't need
  template includes from the playbook itself — the only template
  files are the `template:` module's `.j2` files, which MiniJinja
  loads from disk per call.
- **Native Jinja-style `is`-test syntax**, which our `when:` predicates
  and `until:` predicates use heavily. Tera's analog is awkward.
- **Active maintenance, single Rust author whose code we trust.**

We do not bet on Tera. The cost of being wrong here is large (a
mid-project switch would cascade into every module's template-handling
code), so we pin MiniJinja at a major version and budget time per year
to track it.

### 10.2 The fixed filter catalog (~40, per poor-decisions §2)

Implemented as MiniJinja extensions registered at engine startup:

- **Defaults / control:** `default(value, [boolean=false])`, `mandatory`,
  `mandatory(msg)`, `omit` (sentinel, see §10.4), `ternary(true, false, [null])`.
- **Type:** `bool`, `int`, `float`, `string`, `list`, `dict`, `type_debug`.
- **Encoding / hashing:** `b64encode`, `b64decode`, `urlencode`,
  `quote`, `comment(style)`, `hash(type)`, `password_hash(scheme, [salt], [rounds])`.
- **Format:** `to_json`, `to_nice_json([indent])`, `from_json`, `to_yaml`,
  `to_nice_yaml`, `from_yaml`, `to_toml`, `from_toml`.
  (We add the TOML pair; YAML pair is for migration.)
- **String / regex:** `split(sep)`, `join(sep)`, `splitlines`, `replace(old, new)`,
  `regex_search(pat, [ignorecase, multiline])`,
  `regex_findall(pat, [ignorecase, multiline])`,
  `regex_replace(pat, repl, [ignorecase, multiline, count])`,
  `regex_escape`.
- **Lists:** `unique`, `union`, `intersect`, `difference`,
  `symmetric_difference`, `flatten([levels])`, `min`, `max`, `sum`,
  `length`, `random([seed])`, `shuffle([seed])`,
  `map(attribute=)`, `select(test)`, `reject(test)`, `selectattr(attr, test)`,
  `rejectattr(attr, test)`, `groupby(key)`, `sort([reverse, attribute])`,
  `batch(n)`, `slice(n)`, `zip(other, ...)`, `zip_longest(other, [fillvalue])`.
- **Dict:** `combine(other, [recursive, list_merge])`, `dict2items`,
  `items2dict([key_name, value_name])`.
- **Path:** `basename`, `dirname`, `realpath`, `relpath(start)`, `expanduser`,
  `splitext`, `path_join`.
- **Date:** `to_datetime([fmt])`, `strftime(fmt, [utc])`.

That's ~45 filters. The Ansible filters we explicitly **drop** (and
the migration story):

- **Network filters** (`ipaddr`, `ipv4`, `ipv6`, `ipsubnet`, `ipmath`,
  `cidr_merge`, …) → out-of-tree crate `runsible-net-filters` (post-v1).
- **JMESPath** (`json_query`) → out-of-tree crate (post-v1).
- **Network device parsers** (`parse_cli`, `parse_xml`, `vlan_parser`)
  → never; networking is out of v1 scope per user-stories.

### 10.3 The fixed tests catalog

Same model, registered as MiniJinja tests:

- **Type:** `string`, `mapping`, `sequence`, `iterable`, `number`,
  `integer`, `float`, `boolean`, `defined`, `undefined`, `none`,
  `callable`.
- **Truth:** `truthy([convert_bool])`, `falsy([convert_bool])`.
- **Comparison:** `equalto`, `eq`, `ne`, `gt`, `ge`, `lt`, `le`,
  `divisibleby`, `even`, `odd`.
- **String:** `match(pat)`, `search(pat)`, `regex(pat, [ignorecase, multiline, match_type])`.
- **Version:** `version(other, op, [type])` — `loose`/`strict`/`semver`/`pep440`.
- **Set:** `subset`, `superset`, `contains(value)`.
- **Path:** `file`, `directory`, `link`, `exists`, `mount`, `same_file`,
  `abs`.
- **Task result:** `failed`, `succeeded`, `success`, `changed`, `change`,
  `skipped`, `skip`, `finished`, `started`, `reachable`, `unreachable`.
- **Vault:** `vault_encrypted`, `vaulted_file`.

### 10.4 The `omit` sentinel

Implementation: `omit` is a `MiniJinja Value` representing a sentinel
type `RunsibleOmit`. Filters and the templating layer return it as a
normal value. After rendering a module's argument map, the dispatcher
walks the dict and drops every key whose value is `RunsibleOmit`. This
mirrors Ansible's behavior (poor-decisions §3.2 of -09).

```rust
fn scrub_omits(args: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = args {
        map.retain(|_, v| !is_omit_sentinel(v));
        for v in map.values_mut() { scrub_omits(v); }
    }
}
```

### 10.5 Implicit-coercion policy

The engine **refuses** implicit string ↔ int ↔ bool coercion. Explicit
filters (`| int`, `| bool`, `| string`) are required. TOML's strict
typing makes the surface small; the holdouts are computed strings like
`"{{ port + 1 }}"` where the user must write `"{{ port + 1 | int }}"`.
The lint enforces this at parse time when it can prove a type
mismatch.

---

## 11. Tests

### 11.1 Unit tests

Per file (`#[cfg(test)] mod tests`):

- **AST construction:** every node type has at least one happy-path
  parse and three error-path parses (missing required field, wrong
  type, unknown field).
- **Type checker:** every rule (module exists, args match schema, var
  reachable, tag declared, handler ID resolves, no `let` cycle) has a
  pass test and a fail test.
- **Var precedence resolver:** a parametrised test that walks the
  5-tier merge order with every reasonable combination.
- **Templating filters/tests:** one input/output table per filter;
  borrowed from MiniJinja's test suite where applicable.
- **Loop expansion:** edge cases on empty lists, nested loops, label
  rendering, `loop_control.break_when`.
- **Plan computation:** for each strategy, batch computation against a
  synthetic 100-host inventory.
- **Event serialization:** every `Event` variant round-trips through
  serde with stable field names.

Target: 80% line coverage on `runsible-playbook` itself, 100% on the
type-checker rules and the var-precedence resolver. CI fails on
regressions in those two.

### 11.2 Integration tests

A `tests/` directory with end-to-end runs:

- **Local-only happy path** — every M1 module exercised against
  `connection = "local"` in a Vagrant-less Linux job (uses the host
  filesystem; runs in a container under CI).
- **SSH path against a containerized fleet** — `docker compose` brings
  up 5 Ubuntu containers with ssh + sudo configured; the test uses
  them as `webservers`. Runs the §3.13 example playbook end-to-end and
  asserts the NDJSON stream matches a golden file (after redacting
  timestamps and run IDs).
- **Vagrant fleet** for OS coverage (CentOS, Debian, Ubuntu,
  Alpine). Optional / nightly; not a per-PR gate.
- **Strategy correctness** — a deterministic test asserting `linear`
  barriers between hosts and `free` does not.
- **Failure-handling matrix** — block/rescue/always paths,
  `max_fail_percentage`, `any_errors_fatal`, `force_handlers` all have
  one test each.
- **Check-mode parity** — for each module that supports check mode, a
  test that asserts `--check` produces the same plan that `apply` would
  have produced.
- **Verify (idempotence) sweep** — every M1 module is run twice and
  the second run must report `verify_idempotent: true` for every task.

### 11.3 yaml2toml golden-file regression suite

The corpus harness — the bridge into Ansible's user base
(user-stories §7 spend bullet, §8 R2 risk).

- The corpus is the **top 200 Galaxy roles** (excluding plugin-heavy
  ones — networking, Windows, anything that depends on a Python module
  not in our M1 module list). Plus the top 50 published collections,
  same exclusions.
- For each: yaml2toml converts the role; the output is committed under
  `tests/corpus/<role>/`; a runsible-playbook integration job runs the
  converted playbook against a per-role test inventory (containerized).
- A **golden file** records the NDJSON event stream of a successful
  apply on a fresh container.
- CI fails if (a) yaml2toml regresses and emits different TOML, (b) the
  converted playbook fails to type-check, (c) the converted playbook
  produces a different event stream after redaction.
- Cadence: full suite nightly; per-PR runs a smoke set of 20 roles.

This is the M1 acceptance bar. If `geerlingguy.docker` does not
round-trip and run, M1 does not ship (user-stories §7).

### 11.4 Performance benchmarks

`cargo bench` targets gated by Criterion:

- **Cold start.** From `runsible-playbook --version` invocation to
  process exit, target < 50 ms (head-room under the < 100 ms budget).
- **Parse + type-check throughput.** A 1000-task synthetic playbook
  parses in < 100 ms; type-checks in < 50 ms.
- **Plan computation.** 100-host × 100-task plan computes in < 200 ms.
- **End-to-end happy path.** A real 50-host playbook (10 tasks each)
  via SSH against containers completes in < the wall-clock of the same
  playbook under `ansible-playbook` (target: 3-5× faster on cold start,
  1.5-2× faster on warm start with ControlMaster).

Cold start regression > 20% in a single PR is a release blocker.

---

## 12. Risks

### 12.1 Module trait stability

The `runsible_core::Module` trait is the most load-bearing API in the
project. Every breaking change cascades into every module crate (12 in
M1, dozens by M3, hundreds long-term). Mitigations:

- Freeze the trait shape at M1; document a deprecation cycle for any
  changes.
- Use trait-default methods aggressively for additions, so existing
  modules don't need to recompile.
- Version the trait (`Module1`, `Module2`) if a true break is needed,
  with the engine accepting both.
- Pin `runsible-core` major-version in every module crate's Cargo.toml.

### 12.2 Performance regressions

Cold-start budget is 100 ms. Each new feature is a temptation to add a
proc-macro, a heavy serde deserialization, a global init. Mitigations:

- CI runs the cold-start benchmark on every PR; > 20% regression
  fails the build.
- `cargo bloat` gate on the binary size (target: under 30 MB stripped).
- Dependency budget: every new top-level crate dependency requires a
  written justification in the PR description.

### 12.3 Compatibility with non-trivial existing playbooks

The corpus harness is our defense. But the harness is only as good as
its corpus. Risks:

- A popular role uses a filter we dropped (§10.2 networking filters,
  json_query, etc.). Mitigation: the lint surfaces every dropped filter
  reference; the README and migration doc list them prominently.
- A popular role uses a `meta:` action we don't support. Mitigation:
  the §3.5 / §7.10 `control` enum implements every Ansible meta action.
- A popular role expects fact-cache persistence between runs.
  Mitigation: `runsible-fact-store` ships in M2 with a JSON-file
  backend.

### 12.4 Templating engine choice locking us in

If MiniJinja is deprecated, abandoned, or relicensed, we are stuck.
Mitigations:

- Wrap MiniJinja behind our own thin `Template` trait — every module
  and every internal templating call goes through this trait. A swap
  is a pure refactor in `runsible-playbook`; module crates don't see
  the change.
- Build a small `runsible-template` crate that owns the trait and the
  current MiniJinja impl. If we ever swap, only this crate moves.

### 12.5 The `let`-block + var-precedence redesign breaks playbooks

The 5-tier collapse is a real semantic break. Mitigations:

- `--precedence-compat ansible` for one major version (poor-decisions
  §3 migration path).
- `runsible explain-var` is a first-class debugging tool from M1.
- yaml2toml emits explicit `precedence = "..."` annotations on
  ambiguous bindings, so the conversion is auditable.

### 12.6 Single-maintainer bus factor (project-wide, but acute here)

`runsible-playbook` is the most complex crate. If one person owns it,
it dies if they leave. Mitigations:

- This document and its sister per-crate plans are the architecture
  record (user-stories §8 R7).
- Every top-level module is independently testable; the engine is not
  a monolith from a code-organization standpoint.
- The Module trait provides a clean interface for new contributors:
  "implement this trait, write your tests" is a small, well-scoped
  contribution surface.

---

## 13. Open questions

These are decisions the document cannot resolve unilaterally; they
require either user research, a design meeting, or both.

### 13.1 Keep or drop `meta:` as a category vs splitting into typed control-flow tasks?

The plan above (§3.5 / §7.10) drops the `meta:` keyword and replaces
it with `control = { action = "..." }`. This is opinionated and breaks
muscle memory.

**Alternative:** keep `meta = "flush_handlers"` as a sugar for
`control = { action = "flush_handlers" }`. It's a one-line parser
addition. yaml2toml could emit `meta = "..."` directly, avoiding the
verbose form.

**My current take:** drop. The verbose form is honest about what's
happening (it's a control-flow task, not a no-op), and the sugar
exists in our heads if anyone wants to write a small parser plugin.
But this is the kind of choice that wants validation on a real
playbook corpus before locking in.

### 13.2 Synchronous-by-default + opt-in parallelism, or the reverse?

Ansible's default is parallel-by-default with `forks = 5`. We default
`forks = 20`, which is more aggressive.

**Argument for sync default:** debugging is easier, cold start is
faster, surprises are fewer.

**Argument for parallel default:** the entire reason runsible exists
is performance, and that includes wall-clock on multi-host runs.
Synchronous-by-default is a regression vs Ansible.

**My current take:** parallel default with `forks = 20`. The
performance win is the wedge (user-stories §5). But it means we have
to be ruthless about preserving determinism in our event stream and
making `--forks 1` (synchronous) trivial.

### 13.3 Does `delegate_to` swap variable scope, or keep it on the original host?

Ansible's behavior is to keep variable scope on the original host but
swap connection vars onto the delegate. This is right but subtle.

**Question:** should we honor that, or simplify to "delegate_to swaps
everything"?

**My current take:** honor Ansible's semantics. The split between
"scope owner" and "exec target" is real and useful (a load-balancer
delegate template references the *deployed* host, not the LB). But
this is the most-confused part of Ansible's model and warrants a
design review with users.

### 13.4 Should the engine support any form of dynamic task list?

Our model says: every task list is parsed at load time. `compose
file = "..."` is static; the file path can be templated only with
extra-vars and inventory vars (available pre-run). This is poor-decisions §22
applied to includes/imports.

**Counter-argument:** there are real use cases where the task list
genuinely depends on a fact only known at runtime (e.g., "for each
role in `ansible_facts.installed_roles`, run cleanup"). Without
dynamic compose, users can't do this without a `command:` shell-out.

**My current take:** keep dynamic compose deferred (post-v1). The use
case above can be re-expressed via `loop` over a static task with a
parametric module call, in 95% of cases. The remaining 5% can wait
for v2.

### 13.5 How do we handle a task-level `become_password` that has to be different per host?

`[plays.become.sudo].password.from_keyring` is per-play. But hosts in
the same play might need different sudo passwords (different tenants,
different VMs).

**Option A:** `become.sudo.password.from_keyring` accepts a template:
`from_keyring = "runsible:sudo:{{ inventory_hostname }}"`. Cheap,
flexible. The keyring lookup happens at task-bind time.

**Option B:** require `[plays.become]` to be hostvar-overridden via
the inventory's `ansible_become_password_keyring`. More aligned with
Ansible's model.

**My current take:** Option A. Templating in keyring keys is the
clearest expression of intent and matches the surrounding system
(everything else in TOML accepts templated strings).

### 13.6 What is the canonical way to express "this play has no fact requirements"?

Three choices:
- `[plays.facts] required = []` (empty list).
- Omit `[plays.facts]` entirely (engine defaults to no gather).
- `[plays.facts] required = "none"` (explicit string).

**My current take:** omit means "no gather"; explicit empty list also
means no gather; the `"none"` form is rejected. This makes the lint
cleaner and the schema less ambiguous.

### 13.7 Does `--strict-idempotence` apply to handlers too?

Handlers run at flush points; they fire only on `notify`. If a handler
re-runs on a subsequent `notify` and reports `changed`, that's
expected behavior — not an idempotence violation.

**Question:** do we run `verify()` on handler outcomes at all?

**My current take:** no. Handlers are explicitly side-effecting; the
verify contract is about idempotence of the playbook's *steady-state
intent*, which handlers are not part of. The lint can warn if a
handler's underlying module has `verify_idempotent = true` (most do)
but the engine does not gate on it.

### 13.8 Should we expose a stable Rust API for embedding the engine?

The engine is a library; the binary is a thin shell. That implies a
stable embedding API (`runsible_playbook::run(playbook, inventory,
settings) -> ExitCode`).

**Question:** do we commit to this API stability in v1, or keep it
internal?

**My current take:** commit to it. It is the natural integration point
for `runsible` (ad-hoc), `runsible-pull`, and any future tower-style
controller. Documenting it from M1 forces clean module boundaries
inside the crate.

---

*End of plan. Next steps: land `runsible-core` (the Module trait) as
the M0-prerequisite workspace member, then start M0 on the parser +
type-checker.*
