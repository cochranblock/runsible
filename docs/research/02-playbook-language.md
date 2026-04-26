# Ansible Playbook Language — Exhaustive Reference

> Source: `https://docs.ansible.com/ansible/latest/playbook_guide/` (entire tree).
> Purpose: full distillation of Ansible's playbook language semantics for re-implementation in Rust+TOML (`runsible`). Module catalog and plugin internals are out of scope here — they live in separate research notes.

---

## 0. Mental model

A **playbook** is an ordered list of **plays** (a YAML sequence at the top level of a `.yml` file).

A **play** binds a host pattern to a sequence of **tasks** (with optional `pre_tasks`, `roles`, `post_tasks`, `handlers`).

A **task** is one invocation of one Ansible **module** against the matched hosts.

A **handler** is a task that runs only when notified, deduplicated, after the play body finishes (or at an explicit flush).

A **block** is a logical group of tasks plus optional `rescue:` and `always:` sub-lists, providing exception-style error handling.

Execution order in a play:

```
gather_facts (implicit setup task)
  -> pre_tasks
  -> handlers (flushed if pre_tasks notify)
  -> roles (each role's tasks; dependencies first)
  -> tasks
  -> handlers (flushed if tasks notify)
  -> post_tasks
  -> handlers (flushed if post_tasks notify)
```

Within each batch (controlled by `serial`/strategy), Ansible runs **task N on every host** before moving to task N+1 (the *linear* strategy). Failed hosts drop out for the remainder of the play unless rescued or `clear_host_errors` is invoked.

Templating (Jinja2) happens **on the control node** before the task is shipped to the target. There is no remote interpretation of `{{ ... }}`.

---

## 1. Top-level structure

### 1.1 Play

A play is a mapping that **must** contain `hosts` (or `import_playbook`) and at least one of `tasks`, `roles`, `pre_tasks`, `post_tasks`, `handlers`.

#### Full play-level keyword list

Connection / targeting:
- `hosts` — host pattern (string or list); supports `:`, `&`, `!`, regex (`~`), wildcards.
- `connection` — connection plugin (`ssh`, `local`, `paramiko`, `winrm`, `psrp`, `docker`, `community.general.lxc`, `network_cli`, `httpapi`, `netconf`).
- `port` — connection port override.
- `remote_user` — login user on the target.
- `timeout` — connection timeout (seconds).

Identity / structure:
- `name` — human-readable label for the play.
- `tasks` — list of tasks (executed after `roles`).
- `pre_tasks` — list run before `roles`; notifies flushed before roles run.
- `post_tasks` — list run after `tasks`; final handler flush after.
- `handlers` — list of named tasks that run only on `notify`.
- `roles` — static list of roles (treated as imports; pre-parsed).
- `vars` — inline variable map.
- `vars_files` — list of YAML files (or list-of-lists for first-found semantics) loaded into play vars.
- `vars_prompt` — interactive prompts (see §2.5).
- `module_defaults` — default args per module / action group (see §15).
- `collections` — search path for short-name modules/roles (FQCN preferred since 2.10).
- `environment` — env-var dict applied to every task in the play.

Privilege escalation (see §13):
- `become`, `become_user`, `become_method`, `become_flags`, `become_exe`.

Fact gathering (see §2.6):
- `gather_facts` (bool; default `true`).
- `gather_subset` — subset list (e.g. `["!all", "!min", "network"]`).
- `gather_timeout` — seconds.
- `fact_path` — directory of `.fact` files for `ansible_local`.

Execution control (see §9):
- `strategy` — `linear` (default), `free`, `host_pinned`, `debug`.
- `serial` — int, "20%", or list `[1, 5, "20%"]` for ramping batches.
- `max_fail_percentage` — abort the play when this percent of hosts in a batch fail (must be **exceeded**, not equaled).
- `any_errors_fatal` — bool; first failure ends the play on all hosts (after the failing task completes batch-wide).
- `throttle` — cap concurrent workers per task.
- `order` — `inventory` (default), `reverse_inventory`, `sorted`, `reverse_sorted`, `shuffle`.
- `force_handlers` — run notified handlers even if the play has failures.
- `ignore_errors` / `ignore_unreachable` — inherited by all tasks.
- `run_once` — run on the first host, propagate result to the batch.

Modes (see §14):
- `check_mode` — bool; force dry-run regardless of CLI.
- `diff` — bool; force diff output.

Misc:
- `tags` — applied to every task in the play.
- `no_log` — suppress task logging (inherited).
- `debugger` — `always` / `never` / `on_failed` / `on_unreachable` / `on_skipped` / `on_ok`.
- `validate_argspec` — internal; controls argument-spec validation when role params are passed.

### 1.2 Task

A task is a mapping with **exactly one module invocation** (either as `module_name: { args }`, or `action:` / `local_action:`), optionally plus task-level keywords.

#### Full task-level keyword list

Module invocation:
- `<module_name>` — FQCN (e.g. `ansible.builtin.copy`) or short name (resolved via `collections`).
- `action` — alternate form: `action: ansible.builtin.copy src=... dest=...`.
- `args` — secondary argument map (merged with the module call).
- `local_action` — shorthand for `delegate_to: 127.0.0.1` + `connection: local`.

Identity:
- `name` — task label (templated).
- `tags` — list of tags.
- `notify` — list of handler names or `listen` topics to fire on `changed`.
- `register` — variable name capturing the task result (per-host).

Conditional / retry:
- `when` — Jinja2 expression (evaluated *without* `{{ }}`).
- `failed_when` — override failure condition (string or list, AND-joined).
- `changed_when` — override changed status (string or list).
- `ignore_errors` — bool.
- `ignore_unreachable` — bool.
- `loop` — list to iterate; exposes `item` (and `ansible_loop` if `loop_control.extended`).
- `with_<plugin>` — legacy lookup-driven loop (mutually exclusive with `loop`).
- `loop_control` — `loop_var`, `index_var`, `label`, `pause`, `extended`, `extended_allitems`, `break_when` (2.18+).
- `until` — retry expression (no braces).
- `retries` — retry count for `until` (default 3).
- `delay` — seconds between `until` retries (default 5).

Async:
- `async` — max runtime in seconds.
- `poll` — polling interval in seconds; `0` = fire-and-forget.

Execution context (all also inheritable from block/play):
- `become`, `become_user`, `become_method`, `become_flags`, `become_exe`.
- `connection`, `remote_user`, `port`, `timeout`.
- `environment`.
- `delegate_to`, `delegate_facts`.
- `run_once`.
- `throttle`.
- `check_mode`, `diff`.
- `module_defaults`.
- `collections`.
- `no_log`.
- `vars` — task-only variables (highest override below extra-vars / role params).
- `debugger`.
- `any_errors_fatal`.

### 1.3 Block

Blocks group tasks for shared keywords and `rescue`/`always` error handling.

#### Block-only keywords
- `block` — list of child tasks (required).
- `rescue` — list run on first failure inside `block`.
- `always` — list run regardless of `block`/`rescue` outcome.

#### Inheritable block keywords
Same set as task-level except: **no `loop`, no `with_*`, no `register`, no `notify`** on the block itself (but child tasks of course can use them). All other task keywords (`when`, `become*`, `tags`, `ignore_errors`, `delegate_to`, `environment`, `vars`, `module_defaults`, `connection`, `port`, `remote_user`, `run_once`, `throttle`, `check_mode`, `diff`, `no_log`, `debugger`, `collections`, `any_errors_fatal`) propagate to every child task. The directives **do not affect the block container itself** — they are inherited by enclosed tasks.

### 1.4 Handler

A handler is identical to a task in structure, defined under `handlers:`. It runs only when notified, exactly once per play (deduplicated by name), at a flush point.

Handler-specific behavior:
- `listen` — string (or list of strings) of topic names; multiple handlers can `listen` to the same topic and all fire on a single `notify: <topic>`.
- Handlers **ignore tags** (you cannot select for or against them via `--tags`/`--skip-tags`).
- Handlers cannot use `import_role` or `include_role`.
- `meta: flush_handlers` is the only meta action that cannot be used as a handler itself.
- A dynamic include used as a handler runs **all** of its contained tasks; you cannot notify the inner tasks individually.

---

## 2. Variables

### 2.1 Naming rules

Letters, digits, underscores. Cannot start with a digit. Cannot collide with Python keywords or playbook keywords. Leading underscores have **no privacy meaning**.

### 2.2 Types

- **Scalar**: string / int / float / bool / null. YAML coercion applies.
- **List**: ordered sequence; index with `var[0]` or `var.0` (bracket form is safer).
- **Dict**: mapping; access via `var['key']` (preferred when key matches Python attrs) or `var.key`.

### 2.3 Sources and complete precedence (low → high)

The 22-level canonical order, lowest first (highest wins):

1. command-line values (e.g. `-u`, `-c`, `--connection` — note these are *connection* values, not vars; included here for completeness)
2. role defaults (`roles/<r>/defaults/main.yml`)
3. inventory file or script group vars
4. inventory `group_vars/all`
5. playbook `group_vars/all`
6. inventory `group_vars/*`
7. playbook `group_vars/*`
8. inventory file or script host vars
9. inventory `host_vars/*`
10. playbook `host_vars/*`
11. host facts / cached `set_fact` (with `cacheable: true`)
12. play `vars`
13. play `vars_prompt`
14. play `vars_files`
15. role `vars` (`roles/<r>/vars/main.yml`) and `include_vars`
16. block `vars` (in scope of that block only)
17. task `vars` (in scope of that task only)
18. `include_vars`
19. `set_facts` / registered vars (per-host, current run)
20. role (and `include_role`) params
21. `include` params
22. extra vars (`-e`, **always wins**)

Mnemonic: more specific scope, more recent definition, more explicit invocation → higher precedence. Extra vars always trump everything.

### 2.4 Scopes

- **Global**: config, env, CLI.
- **Play**: visible to every task in the play (subject to inheritance).
- **Host**: facts, inventory vars, registered vars, cacheable `set_fact` — sticky to the host across plays in the same run (and cross-run if a fact cache is enabled).
- **Block** / **task**: confined to that container.
- **Role**: `defaults/` and `vars/` are visible to the role and (because Ansible flattens to play scope by default) to subsequent tasks too. Since 2.15 the `public:` flag on `include_role`/`import_role` controls whether vars and handlers leak.

### 2.5 Defining vars

- `vars:` — inline map at play / role / block / task level.
- `vars_files:` — list of YAML paths; nested list = first-found:
  ```yaml
  vars_files:
    - vars/common.yml
    - [ "vars/{{ ansible_facts['os_family'] }}.yml", "vars/os_defaults.yml" ]
  ```
- `vars_prompt:` — interactive input (skipped under `--extra-vars`, AWX, cron):
  - `name`, `prompt`, `default`, `private` (default `true`), `confirm`, `unsafe`,
  - `encrypt` (e.g. `sha512_crypt`, `bcrypt`, `md5_crypt`, `sha256_crypt`; with Passlib also `des_crypt`, `bsdi_crypt`, `bigcrypt`, etc.),
  - `salt`, `salt_size` (default 8; mutually exclusive with `salt`).
- `set_fact:` — task-time assignment, host-scoped. `cacheable: true` writes through to the fact cache and elevates to fact precedence (level 11).
- `register:` — captures the task return into a per-host variable (always populated even on skip/fail, with `failed`/`skipped`/`changed` flags).
- `include_vars` — runtime load of vars from a file or directory.
- `--extra-vars` / `-e` — `key=val`, JSON, or `@file.json`/`@file.yaml`.

### 2.6 Facts and `gather_facts`

`gather_facts: true` (default) inserts an implicit `ansible.builtin.setup` call at the start of every play. Outputs land in `ansible_facts['<key>']` and (for backwards compat) as top-level `ansible_<key>` variables.

Knobs:
- `gather_subset` — list with `!`-negation (`["!all", "!min", "network"]`).
- `gather_timeout` — seconds.
- `fact_path` — directory of `.fact` files (JSON, INI, or executable) read into `ansible_local.<file>.<section>.<key>`. Note: INI keys are lowercased by ConfigParser.
- `INJECT_FACTS_AS_VARS=False` (config) suppresses the legacy top-level `ansible_*` aliases.
- Fact caching plugins: `jsonfile`, `redis`, `memcached`, `mongodb`, `pickle`, `yaml`, `memory` (the no-op default).

### 2.7 Magic variables (full set)

Identity / inventory:
- `inventory_hostname` — name as it appears in inventory.
- `inventory_hostname_short` — first segment of FQDN.
- `inventory_dir`, `inventory_file` — source directory / file of the host's first definition.
- `inventory_sources` (a.k.a. `ansible_inventory_sources`) — full list of inventory sources used.
- `groups` — `dict[group_name -> list[hostname]]` for the entire inventory.
- `group_names` — list of groups the current host belongs to.
- `hostvars` — `dict[hostname -> vars]`; also gives access to other hosts' facts after they've been gathered.
- `omit` — sentinel that removes an option from a module call (used as `value: "{{ var | default(omit) }}"`).

Play state:
- `ansible_play_name` — current play's `name` (2.8+).
- `ansible_play_hosts` — hosts in the current play **not** limited by `serial`.
- `ansible_play_hosts_all` — every host originally targeted by the play.
- `ansible_play_batch` — hosts in the **current** serial batch.
- `play_hosts` — deprecated alias of `ansible_play_batch`.
- `ansible_play_role_names` — roles used in this play (excluding implicit deps).
- `ansible_dependent_role_names` — implicit role dependencies in this play.
- `ansible_role_names` — union of the above two.
- `ansible_role_name` — FQCN of the currently executing role (`ns.col.role`).
- `ansible_parent_role_names` / `ansible_parent_role_paths` — call chain when invoked through `include_role`/`import_role`.
- `role_name` — short name of the current role.
- `role_names` — deprecated alias of `ansible_play_role_names`.
- `role_path` — filesystem path of the current role.

Run-time meta:
- `ansible_check_mode` — bool, true under `--check`.
- `ansible_diff_mode` — bool, true under `--diff`.
- `ansible_forks` — fork limit for this run.
- `ansible_verbosity` — `-v` count.
- `ansible_run_tags` — value of `--tags`.
- `ansible_skip_tags` — value of `--skip-tags`.
- `ansible_limit` — value of `--limit`.
- `ansible_search_path` — current action-plugin / lookup search path.
- `ansible_config_file` — path to the ansible.cfg in use.
- `ansible_playbook_python` — Python interpreter that started ansible-playbook on the control node.
- `ansible_version` — dict `{full, major, minor, revision, string}`.
- `ansible_collection_name` — current collection of the running task.
- `playbook_dir` — directory of the playbook currently executing.

Loop:
- `ansible_loop_var` — name in use for the loop variable (default `item`).
- `ansible_index_var` — name of the index var.
- `ansible_loop` — extended loop info (when `loop_control.extended: true`): `index`, `index0`, `revindex`, `revindex0`, `first`, `last`, `length`, `previtem`, `nextitem`, `allitems`.

Facts:
- `ansible_facts` — all gathered facts for the current host.
- `ansible_local` — custom facts from `fact_path`.

These names are reserved — assigning to them is undefined behavior.

### 2.8 Lazy evaluation rules

- Variable values are stored as Jinja2-expression-bearing strings (or unsafe-marked) and **rendered on use**, not on definition.
- A `vars:` reference can refer to another variable that gets defined later in the precedence chain — final value depends on what's resolvable at render time.
- `register` results, by contrast, are *concrete dicts* — they freeze the task return.
- `set_fact` evaluates the RHS at the moment the task runs.
- Imports are pre-parsed: variables used to template `import_*: file_name` must be available before play execution starts (e.g. extra-vars, group_vars). Dynamic `include_*` templates at runtime.

### 2.9 Combining and merging

- Lists: `list1 + list2`, `list1 | union(list2)`, `list1 | unique`.
- Dicts: `combine` filter — `dict1 | combine(dict2, recursive=True, list_merge='append')`.
  - `list_merge` options: `replace` (default), `keep`, `append`, `prepend`, `append_rp`, `prepend_rp`.
- Global config: `hash_behaviour = merge` (NOT recommended; default `replace`) makes Ansible deep-merge same-named hashes across precedence levels instead of overwriting.

---

## 3. Templating (Jinja2)

### 3.1 Where it runs

All templating is performed on the **control node**, before the task is dispatched. Targets see only resolved values. Therefore: filters, lookups, tests, and `now()`/`undef()` cannot reflect remote state, only what's available locally at render time.

Files passed to the `template` module are also rendered locally before transfer; they must be UTF-8.

### 3.2 Syntax

- `{{ expression }}` — substitute value.
- `{% statement %}` — control flow (`if`, `for`, `set`, `block`, `extends`, `include`, `macro`, `raw`).
- `{# comment #}` — comment.
- `{% raw %}` … `{% endraw %}` — disable templating inside.
- Whitespace control: `{%- ... -%}`, `{{- ... -}}`. `trim_blocks` and `lstrip_blocks` controllable per template via the `template` module (`trim_blocks: yes`, `lstrip_blocks: no` — note Ansible defaults differ from upstream Jinja).
- Custom delimiters: settable via the `template` module's `variable_start_string`, `variable_end_string`, `block_start_string`, `block_end_string`, `comment_start_string`, `comment_end_string`.

### 3.3 Special functions

- `now(utc=False, fmt=None)` — current datetime (added 2.8).
- `undef(hint='...')` — explicit undefined value with diagnostic hint.
- `lookup(name, args, errors='strict|warn|ignore', wantlist=False)` and `query(name, args)` — alias `q()` — runs lookup plugins; `query` always returns a list.

### 3.4 Filters (Ansible-shipped, complete-as-documented)

Defaults / control:
- `default(value, boolean=False)` — fallback for undefined; with `boolean=True`, also falls back on falsy.
- `mandatory` / `mandatory(msg)` — raise if undefined.
- `omit` (special var, behaves filter-like in `default(omit)`).
- `ternary(true_val, false_val[, null_val])`.

Type discovery / coercion:
- `type_debug` — Python type name.
- `bool`, `int`, `float`, `string`, `list`, `dict`.

Encoding / hashing:
- `b64encode`, `b64decode` (optional `encoding=`).
- `urlencode`.
- `quote` (shell-safe).
- `comment(style='plain'|'c'|'cblock'|'erlang'|'xml'|...)`.
- `hash(type)` — `sha1`, `sha256`, `sha512`, `md5`, `blowfish`, …
- `checksum` — default sha1 of string.
- `password_hash(scheme[, salt][, rounds][, ident])`.
- `vault(passphrase[, salt])` / `unvault(passphrase)`.

Format conversion:
- `to_json`, `to_nice_json(indent=N)`, `from_json`.
- `to_yaml`, `to_nice_yaml(indent=N, sort_keys=True)`, `from_yaml`, `from_yaml_all` (multi-doc).

String / regex:
- `split(sep)`, `join(sep)`, `splitlines`, `replace(old, new)`.
- `regex_search(pat[, ignorecase, multiline])`.
- `regex_findall(pat[, ignorecase, multiline])`.
- `regex_replace(pat, replacement[, ignorecase, multiline])`.
- `regex_escape([, re_type='python'|'posix_basic'])`.

Lists / sets:
- `unique`, `union`, `intersect`, `difference`, `symmetric_difference`.
- `flatten([levels=N, skip_nulls=True])`.
- `min`, `max` (optional `attribute=`).
- `extract(container[, default])`.
- `permutations([k])`, `combinations(k)`, `product(*lists)`.
- `zip`, `zip_longest([fillvalue=...])`.
- `subelements(path[, skip_missing=False])`.
- `random([seed=...])`, `shuffle([seed=...])`.
- `random_mac(prefix[, seed=...])`.
- `map`, `select`, `reject`, `selectattr`, `rejectattr`, `groupby`, `sort`, `batch(n)`, `slice(n)`.

Dict:
- `combine(other[, recursive=False, list_merge='replace'|'keep'|'append'|'prepend'|'append_rp'|'prepend_rp'])`.
- `dict2items([key_name=, value_name=])`.
- `items2dict([key_name=, value_name=])`.

Path / file:
- `basename`, `dirname`, `expanduser`, `expandvars`, `realpath`, `relpath(start)`.
- `splitext`, `path_join`.
- `win_basename`, `win_dirname`, `win_splitdrive`.
- `urlsplit('hostname'|'netloc'|'scheme'|'path'|'query'|'fragment'|'port'|'username'|'password')`.

Date / UUID:
- `to_datetime([fmt])`, `strftime(fmt[, utc=False])`.
- `to_uuid([namespace=...])` — UUIDv5.
- `human_readable`, `human_to_bytes`.

Network (collection-shipped, included by default in `community.general` / `ansible.utils`):
- `ipaddr`, `ipv4`, `ipv6`, `hwaddr`, `macaddr`.
- `parse_cli`, `parse_cli_textfsm`, `parse_xml`, `vlan_parser`.

JMESPath:
- `json_query(expr)` (from `community.general`).

Kubernetes:
- `k8s_config_resource_name` (from `kubernetes.core`).

### 3.5 Tests (Ansible + inherited Jinja)

Built-in Jinja:
- `defined`, `undefined`, `none`, `sameas`, `in`, `callable`.
- Comparison: `equalto`, `eq`, `ne`, `gt`, `ge`, `lt`, `le`.
- Numeric: `divisibleby`, `even`, `odd`.
- Strings: `escaped`, `lower`, `upper`.

Truthiness:
- `truthy([convert_bool=False])`, `falsy([convert_bool=False])`.

Version:
- `version(other, op[, version_type='loose'|'strict'|'semver'|'semantic'|'pep440'])`. Operators: `<`, `<=`, `>`, `>=`, `==`, `!=`. Old name `version_compare`.

Set theory:
- `subset`, `superset`, `contains`, `all`, `any`.

Path:
- `file`, `directory`, `link`, `exists`, `mount`, `abs`, `same_file`.

Task result:
- `failed`, `succeeded` / `success`, `changed` / `change`, `skipped` / `skip`, `finished`, `started`.
- `reachable`, `unreachable` (for batch-level checks).

Type:
- `string`, `number`, `integer`, `float`, `boolean`, `sequence`, `mapping`, `iterable`.

Vault:
- `vault_encrypted` (inline single-vaulted value), `vaulted_file`.

Regex:
- `match` (pattern at start), `search` (anywhere), `regex` (configurable via `match_type=`).
- All accept `ignorecase=`, `multiline=`.

URI:
- `uri`, `url`, `urn` (collection-provided).

### 3.6 Lookups (`ansible.builtin`, complete)

| Plugin | Purpose |
|---|---|
| `config` | resolved Ansible config option values |
| `csvfile` | look up cell in TSV/CSV by key |
| `dict` | iterate dict as `{key, value}` items |
| `env` | environment variable on the **control** node |
| `file` | read file contents |
| `fileglob` | list files matching a shell glob |
| `first_found` | first existing path from a list |
| `indexed_items` | return `(idx, item)` pairs |
| `ini` | read value from INI file |
| `inventory_hostnames` | hosts matching an inventory pattern |
| `items` | flatten a list of lists |
| `lines` | one-per-line stdout from a shell command |
| `list` | identity (returns input) |
| `nested` | cartesian-product of nested lists |
| `password` | get-or-generate a password, persisted in a file |
| `pipe` | stdout of a shell command (no shell features) |
| `random_choice` | random item from a list |
| `sequence` | generate `start..end` sequence with stride/format |
| `subelements` | flatten a list of dicts on a sub-key |
| `template` | read file then render through Jinja2 |
| `together` | zip lists into synced rows |
| `unvault` | read vault-encrypted file |
| `url` | HTTP GET |
| `varnames` | variable names matching a regex |
| `vars` | resolve a variable by templated name |

Common third-party / collection lookups: `dig`, `dnstxt`, `etcd`, `redis`, `mongodb`, `hashi_vault`, `k8s`, `cyberarkpassword`, `lastpass`, `passwordstore`, `keyring`, `aws_account_attribute`, `aws_secret`, `aws_ssm`. (Out of scope for the Rust port at first.)

`lookup()` returns a string by default (CSV-joins lists). Pass `wantlist=True` or use `query()`/`q()` for list output. `errors='ignore'|'warn'|'strict'` controls failure behavior.

---

## 4. Loops

### 4.1 Forms

- `loop:` — preferred since 2.5; expects a list.
- `with_<plugin>:` — runs a lookup plugin and iterates the result.
- `until:` — retry loop (different concept: while-condition, not for-each).

`loop` and `with_*` are mutually exclusive on the same task.

### 4.2 `loop_control`

```yaml
loop_control:
  loop_var: <name>          # default 'item'; needed for nested loops to avoid clash
  index_var: <name>         # 0-based index variable
  label: "{{ item.name }}"  # what to print per-iteration
  pause: 3                  # seconds between iterations
  extended: true            # populate ansible_loop with rich metadata
  extended_allitems: false  # drop allitems to save memory
  break_when:               # 2.18+: list of expressions; first true ends the loop
    - some_condition
```

`ansible_loop` keys (when `extended: true`):
- `index` (1-based), `index0`.
- `revindex` (1-based remaining), `revindex0`.
- `first`, `last` (booleans).
- `length` (total).
- `previtem`, `nextitem`.
- `allitems` (full list; suppressible).

### 4.3 Retry: `until` / `retries` / `delay`

```yaml
- shell: /usr/bin/foo
  register: result
  until: result.stdout.find("ok") != -1
  retries: 5         # default 3
  delay: 10          # default 5 seconds
```

Result has `attempts` key.

When combined with `loop`, `until` applies **per item** independently.

### 4.4 `register` with loops

Result becomes `{ results: [ {...per item...} ], changed, failed, skipped, msg }`. Each `results[i]` carries `item` and `_ansible_item_label` (if `label` set) and standard return keys.

### 4.5 `with_*` → `loop` migration table

| `with_*` | `loop` equivalent |
|---|---|
| `with_list: x` | `loop: x` |
| `with_items: x` | `loop: "{{ x | flatten(levels=1) }}"` (note: `with_items` flattens one level implicitly) |
| `with_indexed_items: x` | `loop: "{{ x | flatten(levels=1) }}"` + `index_var: index` |
| `with_dict: x` | `loop: "{{ x | dict2items }}"` |
| `with_together: [a, b]` | `loop: "{{ a | zip(b) | list }}"` |
| `with_subelements: [list, key]` | `loop: "{{ list | subelements('key') }}"` |
| `with_nested: [a, b]` / `with_cartesian` | `loop: "{{ a | product(b) | list }}"` |
| `with_sequence: start=0 end=4 stride=2 format=...` | `loop: "{{ range(0, 5, 2) | list }}"` |
| `with_flattened: x` | `loop: "{{ x | flatten }}"` |
| `with_random_choice: x` | `msg: "{{ x | random }}"` (no loop needed) |
| `with_fileglob: pat` | `loop: "{{ query('fileglob', pat) }}"` |
| `with_first_found: list` | `loop: "{{ query('first_found', list) }}"` |
| `with_lines: cmd` | `loop: "{{ query('lines', cmd) }}"` |
| `with_url: url` | `loop: "{{ query('url', url) }}"` |

### 4.6 Looping over inventory / hosts

```yaml
loop: "{{ groups['all'] }}"
loop: "{{ ansible_play_batch }}"
loop: "{{ query('inventory_hostnames', 'webservers:!staging') }}"
```

### 4.7 Bulk-arg vs loop

For modules that natively accept a list (`yum`, `apt`, `package`, `user` with `users:` etc.), passing the list directly is dramatically faster than looping (one transaction vs. N transactions).

### 4.8 Loops with `include_tasks`

Loops over `include_tasks` execute the included task list once per iteration; use `loop_control.loop_var` to rename `item` so inner loops don't shadow outer.

---

## 5. Conditionals

### 5.1 `when`

Raw Jinja2 expression (no `{{ }}`). Evaluated per-host, per-iteration of any loop.

A list of expressions is implicitly AND'd:
```yaml
when:
  - ansible_facts['distribution'] == 'CentOS'
  - ansible_facts['distribution_major_version'] | int >= 8
```

### 5.2 Type pitfalls

- Strings like `"yes"`, `"true"`, `"on"`, `"1"` are not booleans by default; use `| bool`.
- Numeric strings need `| int` for arithmetic comparison (`"127" >= 6` evaluates as string-lexicographic).
- Comparing facts that may be missing: `ansible_facts['x'] is defined and ansible_facts['x'] == ...`.

### 5.3 Defined / undefined

`is defined`, `is undefined`, `is none`, `is not none`, `is mapping`, `is sequence`, etc.

### 5.4 Conditionals on registered output

```yaml
when: result.rc != 0
when: result is failed
when: result is changed
when: result is skipped
when: result.stdout.find('ok') != -1
when: result.stdout_lines | length > 0
```

A registered var is **always** present, even on skip/fail — with `failed`/`skipped`/`changed` flags set.

### 5.5 With loops

`when` is evaluated **per item** — selectively skip iterations.

Use `| default([])` for loops over potentially-undefined collections to avoid error.

### 5.6 Conditionals on includes / imports

- `when` on `import_*` propagates to **every imported task** (each task re-evaluates the same expression at runtime).
- `when` on `include_*` gates only the include statement itself; the included tasks still run normally once entered.

### 5.7 `failed_when` / `changed_when`

Override module's own definition of failed/changed.

```yaml
- shell: do_thing
  register: r
  failed_when: r.rc != 0 and 'expected_warning' not in r.stderr
  changed_when: r.rc == 0 and 'no-op' not in r.stdout
```

Multiple expressions in a list are AND-joined for `failed_when` (an explicit `or` inside one string is needed for OR semantics).

### 5.8 `ignore_errors`, `ignore_unreachable`

- `ignore_errors: true` — keep going past `failed`. Does NOT mask: undefined-variable errors, connection failures, missing modules, syntax errors, host-unreachable.
- `ignore_unreachable: true` — keep the host alive even if Ansible can't connect.

---

## 6. Error handling

### 6.1 `block` / `rescue` / `always`

```yaml
- block:
    - cmd: do_thing
    - cmd: do_other_thing
  rescue:
    - debug: msg="recovering"
    - cmd: rollback
  always:
    - debug: msg="cleanup"
```

Semantics:
- Tasks under `block` run in order. First failure → jump to `rescue`.
- `rescue` runs only on `failed` state. Unreachable hosts and parser/syntax errors do **not** trigger `rescue` or `always`.
- If `rescue` succeeds, the host's failure status is cleared and the play continues normally (so `max_fail_percentage`/`any_errors_fatal` are not tripped).
- `always` runs unconditionally, regardless of block or rescue outcome.

In `rescue`, two magic vars are populated:
- `ansible_failed_task` — task object (`name`, `action`, `args`, …).
- `ansible_failed_result` — full return dict from the failed task (same as `register`).

### 6.2 Play-wide

- `any_errors_fatal: true` — first task failure on any host causes Ansible to finish that task batch-wide and then halt the play. Recovery via `rescue` still possible.
- `max_fail_percentage: N` — abort batch when failed-host count **exceeds** N% (not equal).
- `force_handlers: true` (or `--force-handlers` / config) — flush notified handlers even when later tasks fail or the play aborts.

### 6.3 Meta tasks for flow

`meta:` accepts a single free-form action:

| Action | Scope | Effect |
|---|---|---|
| `flush_handlers` | play | run all pending handlers immediately |
| `refresh_inventory` | play | re-run dynamic inventory sources |
| `clear_facts` | host | drop cached / `set_fact cacheable` data |
| `clear_host_errors` | host | revive failed/unreachable hosts |
| `end_play` | play | end the play cleanly (no fail) |
| `end_host` | host | end the play for one host (no fail) |
| `end_batch` | batch | finish current `serial` batch (acts like `end_play` if no `serial`) |
| `end_role` | role | skip remaining tasks in the current role (2.18+) |
| `reset_connection` | host | drop persistent connection (e.g. SSH ControlPersist) |
| `noop` | global | do nothing (internal) |
| `role_complete` | role | mark role as fully run (used by include_role internally) |

Meta tasks themselves **cannot have a `when:` evaluated on the host side for some actions**; check per-action docs (`flush_handlers` cannot use `when` historically — fixed in newer versions).

---

## 7. Roles

### 7.1 Standard layout

```
roles/<role_name>/
  tasks/main.yml
  handlers/main.yml
  defaults/main.yml          # lowest-precedence vars (level 2)
  vars/main.yml              # high-precedence role vars (level 15)
  files/                     # static files for `copy`, `script`, etc.
  templates/                 # .j2 files for `template`
  meta/main.yml              # dependencies, galaxy info, allow_duplicates
  meta/argument_specs.yml    # role-arg validation (2.11+)
  library/                   # role-bundled modules
  module_utils/              # role-bundled module utilities
  filter_plugins/            # role-bundled filters
  lookup_plugins/            # role-bundled lookups (any plugin type)
  tests/                     # role test infra
  README.md
```

`defaults/` and `vars/` may be sub-directories — Ansible loads alphabetically.

### 7.2 Search path

Resolved in this order:
1. Inside collections (when role is referenced as `ns.col.role`).
2. `roles/` adjacent to the playbook.
3. `roles_path` config (default `~/.ansible/roles:/usr/share/ansible/roles:/etc/ansible/roles`).
4. The directory of the playbook.
5. Parent role's `roles/` when nested.

### 7.3 Three invocation styles

- **`roles:`** at play level — pre-parsed (effectively static import), runs *between* `pre_tasks` and `tasks`. Dependencies execute before the role.
- **`import_role:`** — static; evaluated at parse time; tags & `when:` propagate to every imported task.
- **`include_role:`** — dynamic; evaluated at runtime; tags & `when:` apply only to the include itself unless `apply:` is used.

Common keys:
```yaml
- include_role:                # or import_role
    name: foo
    tasks_from: extra.yml      # custom entry point (defaults to main.yml)
    vars_from: env.yml         # custom vars file under vars/
    defaults_from: env.yml     # custom defaults file
    handlers_from: extra.yml
    public: true               # 2.15+: control whether vars/handlers leak to play scope
    apply:
      tags: [...]
      become: true
      when: condition
```

### 7.4 Variable scope and precedence

- `defaults/` is precedence 2 — overridable by basically anything.
- `vars/` is precedence 15 — high; only block/task vars, set_fact, role params, include params, and extra-vars beat it.
- Role params (passed via `roles: - role: foo`, `vars:`, or as kwargs to `include_role`/`import_role`) are precedence 20.
- Role-defined handlers and (default behavior) vars become play-global once the role is loaded. `public: false` keeps `include_role` private.
- Pre-2.15: vars always leaked. Post-2.15: dynamic include defaults to *not* leaking.

### 7.5 Dependencies

`meta/main.yml`:
```yaml
dependencies:
  - role: common
    vars:
      app: web
  - role: apache
    when: ansible_facts['os_family'] == 'RedHat'
```

Dependencies run **before** the dependent role (DFS order). Each unique (role, params, tags, when) combination runs once. To force re-run, set `allow_duplicates: true` in the role's `meta/main.yml`, or invoke with different params.

### 7.6 Argument validation (`meta/argument_specs.yml`, 2.11+)

```yaml
argument_specs:
  main:
    short_description: Entry point
    options:
      foo_port:
        type: int
        required: false
        default: 80
        description: Port to bind
        choices: [80, 443]
      foo_host:
        type: str
        required: true
```

Auto-injected as a task tagged `always`; failure aborts the role.

### 7.7 Handlers in roles

Handlers from a role are loaded into the play's global handler namespace. Reference them as `<role_name> : <handler_name>` to disambiguate. Handlers from `roles:` flush automatically at the end of the `tasks:` section (and at section boundaries).

Loading order:
1. Handlers from `roles:` (and their deps).
2. Handlers from inline `handlers:`.
3. Handlers from `import_role` tasks.
4. Handlers from `include_role` tasks.

---

## 8. Includes and imports

### 8.1 The two families

| | `import_*` (static) | `include_*` (dynamic) |
|---|---|---|
| When resolved | parse time | runtime |
| Loop allowed | NO | YES |
| `when` semantics | applied to each imported task individually | gates the include itself |
| Tags inherited by inner tasks | YES | NO (use `apply:`) |
| `--list-tasks` shows inner | YES | NO |
| `--list-tags` shows inner | YES | NO |
| `--start-at-task` works on inner | YES | NO |
| Handler notify by inner-task-name | YES | NO (notify the include name; runs all) |
| Filename templating | requires the var to be available pre-run (extra-vars / inventory) | full runtime templating |
| Inventory / host vars in filename | NO | YES |
| Speed / overhead | leaner | heavier |

### 8.2 The keywords

- `import_playbook: other.yml` — top-level only; brings in another playbook's plays.
- `import_tasks: file.yml` — inserts tasks at parse time.
- `import_role: name=foo` — pre-parsed role inclusion.
- `include_tasks: file.yml` — runtime task inclusion (loop-friendly).
- `include_role: name=foo` — runtime role inclusion (loop-friendly).
- `include_vars: file_or_dir` — runtime variable load. Supports recursion, file-extension filtering (`extensions: [yaml, yml, json]`), `name:` to namespace into a sub-key, `depth:`, `files_matching:`, `ignore_files:`.

### 8.3 Best practice

The docs explicitly warn: "*it is best to select one approach per playbook. Mixing static and dynamic reuse can introduce difficult-to-diagnose bugs.*"

---

## 9. Strategies

### 9.1 The shipped strategies

| Strategy | Behavior |
|---|---|
| `linear` (default) | Run task N on every host (within the batch) before moving to task N+1. Synchronization point at every task. |
| `free` | Each host races through tasks as fast as it can; no per-task barrier. Hosts finish at their own pace. |
| `host_pinned` | Each fork is pinned to a host until that host's play finishes; new hosts are only started once a fork frees up. Useful when target-side resource locking matters. |
| `debug` | Run linearly, but invoke the interactive debugger automatically on the configured trigger conditions. |

All strategies are plugins; `STRATEGY_PLUGIN_PATH` is configurable.

### 9.2 Concurrency knobs

- `forks` (config / `-f`) — global parallel worker cap.
- `serial` — batch size; can be int, percent, or list (ramp). `serial: [1, 5, "20%"]` runs 1 host, then 5, then 20% of total per remaining batch.
- `throttle` — task/block-level cap on concurrent workers. Cannot **increase** parallelism — only restrict it.
- `order` — host iteration order: `inventory` (default), `reverse_inventory`, `sorted` (alphabetical), `reverse_sorted`, `shuffle`.
- `run_once: true` — task runs on the first host of the batch; result is broadcast to the entire batch (same `register` value on every host). Common with `delegate_to`.

### 9.3 `serial` failure-scope note

When `serial` is set, `max_fail_percentage` and `any_errors_fatal` apply **per batch**, not to the play as a whole.

---

## 10. Delegation

### 10.1 Keywords

- `delegate_to: <host_or_ip>` — execute this task against another machine.
- `delegate_facts: true` — apply gathered/registered facts to the delegated-to host instead of `inventory_hostname`.
- `local_action: <module> [args]` — sugar for `delegate_to: 127.0.0.1` + `connection: local`.
- `run_once: true` — pair with delegate_to to do something exactly once on a control plane (e.g. update a load balancer).

### 10.2 Connection swap behavior

When delegated:
- `ansible_host`, `ansible_user`, `ansible_port`, `ansible_connection`, `ansible_python_interpreter`, `become_*`, `ansible_ssh_*` reflect the **delegate target's** connection vars, not the inventory host's.
- `inventory_hostname` and other "subject" vars remain the **original** host (the one the task is logically *for*).
- To read original-host facts/vars while delegated: `hostvars[inventory_hostname]['ansible_facts']['...']`.
- The delegate target need not be in the play's `hosts:` pattern; it can be any inventory host or even an unmanaged hostname (though delegating to non-inventory hosts is fragile — use `add_host` to register them properly).

### 10.3 Concurrency caveats

Delegation does not serialize; multiple original hosts can hammer the same delegate concurrently. To prevent races on the delegate:
- `run_once: true`, or
- `serial: 1` (slow), or
- `throttle: 1` (per-task).

### 10.4 Cannot be delegated

`include`, `import_*`, `add_host`, `debug` (the last because there's nothing to send remotely; it just renders locally).

---

## 11. Async

### 11.1 Keywords

- `async: <seconds>` — maximum runtime; module is launched in the background on the target.
- `poll: <seconds>` — control-node polling cadence:
  - `poll > 0` — control node blocks, polling every N seconds, until completion or `async` timeout.
  - `poll: 0` — fire-and-forget; control node moves on immediately. The job ID is returned for later checking.

### 11.2 Checking status

```yaml
- name: Kick off
  shell: long_running
  async: 3600
  poll: 0
  register: job

- name: Wait
  async_status:
    jid: "{{ job.ansible_job_id }}"
  register: status
  until: status.finished
  retries: 100
  delay: 30
```

### 11.3 Cleanup

`poll: 0` jobs leave job-cache files behind; clear with `async_status: mode=cleanup`.

### 11.4 Limitations

- `async` does not support check mode (will fail).
- Some action plugins (`copy`, `template`, `fetch`) cannot do background data transfer — async wraps the *module* call, not the file-shipping.
- Async tasks always report `changed: true` initially; downstream `async_status` callers should override with `changed_when:` as needed.
- Don't combine `poll: 0` with operations that take exclusive system locks (e.g. `yum`/`apt`) if subsequent tasks will use the same lock.

### 11.5 Ad hoc

`ansible all -B 3600 -P 0 -a "/usr/bin/long_op"` — `-B` async, `-P` poll.

---

## 12. Tags

### 12.1 Application sites

`tags:` accepted on tasks, blocks, plays, roles, and the `include_*` / `import_*` keywords.

### 12.2 Inheritance

Tags applied at higher levels (play, block, role, import) propagate to every contained task. Dynamic includes are an exception:
- `import_*` — tags inherited by all inner tasks.
- `include_*` — tags only on the include statement; use `apply: { tags: [...] }` (or wrap in a tagged block) to push tags inward.

### 12.3 Special tags

- `always` — runs unless `--skip-tags always` is used. Fact gathering is implicitly `always`. `meta/argument_specs.yml`-driven validation is also `always`.
- `never` — skipped unless `--tags never` (or any other tag on that task) is requested.
- `all` — pseudo-tag matching everything except `never`.
- `tagged` — only tasks with at least one tag.
- `untagged` — only tasks with no tags.

### 12.4 CLI

- `--tags t1,t2` — run only tasks matching these (or `always`).
- `--skip-tags t3,t4` — drop these (skip wins on conflict).
- `--list-tags`, `--list-tasks` — preview; cannot reach into dynamic includes.
- Config: `TAGS_RUN`, `TAGS_SKIP` for global defaults.

### 12.5 Block-tag interaction with rescue/always

If the block's tag is not selected, **its rescue and always sub-lists are also skipped** — tag selection trumps error-handling structure.

### 12.6 Handlers ignore tags

Handlers cannot be filtered by tags. They always run (or always skip) based purely on whether they were notified.

---

## 13. Privilege escalation

### 13.1 Keywords

| Key | Purpose |
|---|---|
| `become` | bool, enable escalation |
| `become_user` | target user (default `root`) |
| `become_method` | escalation tool plugin |
| `become_flags` | flags appended to the escalation command |
| `become_exe` | full path to the escalation binary |
| `become_password` (var only) | password (use Vault) |

Connection-var equivalents: `ansible_become`, `ansible_become_method`, `ansible_become_user`, `ansible_become_password`, `ansible_common_remote_group`, `ansible_become_exe`, `ansible_become_flags`.

CLI: `-b`/`--become`, `-K`/`--ask-become-pass`, `--become-method=...`, `--become-user=...`.

### 13.2 Methods (built-in plugins)

`sudo`, `su`, `pbrun`, `pfexec`, `dzdo`, `ksu`, `runas` (Windows), `doas`, `machinectl`, `enable` (network device privileged mode), `sesu` (community), plus collection-shipped plugins.

Only one method per task; cannot chain (no "sudo then su").

### 13.3 Unprivileged-user fallback

When neither connection user nor become user can write to `/tmp/.ansible-tmp-*`, Ansible tries (in order):
1. POSIX ACLs via `setfacl`.
2. `chown` (if connection user can).
3. macOS-style `chmod +a` ACL (2.11+).
4. Common group via `chgrp` if `ansible_common_remote_group` set.
5. World-readable temp file (only if `allow_world_readable_tmpfiles=True`).

Pipelining sidesteps this: module source piped to stdin instead of dropped to disk. Disabled by default; incompatible with file-transfer modules and non-Python modules.

### 13.4 Network devices

`become_method: enable` for network CLI elevation. Requires `connection: ansible.netcommon.network_cli` or `httpapi`.

### 13.5 Windows specifics

`runas` requires the connection user to be Administrator and the Secondary Logon service running. Local service accounts (`System`, `NetworkService`, `LocalService`) need no password (2.5+). Passwordless `runas` for normal users requires `SeDebugPrivilege` and `SeBatchLogonRight` / `SeNetworkLogonRight` (2.8+). `become_flags` accepts `logon_type` (`interactive`/`batch`/`new_credentials`/`network`/`network_cleartext`) and `logon_flags` (`with_profile`/`netcredentials_only`).

### 13.6 Limitations

- One escalation step (no nesting).
- Cannot whitelist specific commands in sudoers; module names are dynamic.
- systemd-using distros: default `sudo` doesn't open a new session, so `XDG_RUNTIME_DIR`-tied operations need `become_method: machinectl`.

---

## 14. Check mode + diff

### 14.1 Check mode

`--check` (or `ANSIBLE_CHECK=True`) runs everything as a simulation. Per-module behavior:
- Modules that declare `supports_check_mode = True` skip side-effects and return what they *would* have done.
- Modules without check-mode support **return `skipped`**; they don't make changes.

Task-level overrides:
- `check_mode: true` — always treat this task as check mode (good for read-only ops you want to keep simulating).
- `check_mode: false` — always run for real, even under `--check` (use carefully — formerly `always_run: true` pre-2.2).

Magic var: `ansible_check_mode` (bool). Use it for branching:
```yaml
when: not ansible_check_mode
ignore_errors: "{{ ansible_check_mode }}"
```

Caveat: tasks gated by registered vars set in earlier tasks may produce no output in check mode if those earlier tasks didn't run.

### 14.2 Diff mode

`--diff` (or `diff: true` keyword) prints before/after for file-mutating modules (`template`, `lineinfile`, `blockinfile`, `copy`, `user`, `git`). Combine with `--check` for safe preview.

Task-level: `diff: false` to suppress for sensitive content.

Magic var: `ansible_diff_mode`.

Module support flag: `supports_diff = True` in module's `AnsibleModule(...)` constructor.

---

## 15. Module defaults

### 15.1 Per-module

Set defaults that every invocation of a module inherits, unless overridden by the task itself:

```yaml
- hosts: all
  module_defaults:
    ansible.builtin.file:
      owner: root
      group: root
      mode: "0755"
    ansible.builtin.uri:
      validate_certs: false
      timeout: 30
  tasks:
    - file: { path: /etc/foo, state: directory }   # inherits owner/group/mode
```

Empty-dict resets: `ansible.builtin.file: {}`.

### 15.2 Action groups

Apply defaults to **all modules in a logical group**:

```yaml
module_defaults:
  group/aws:
    region: us-east-1
    profile: prod
  group/k8s:
    host: https://...
    api_key: "{{ kube_token }}"
```

Built-in groups: `group/aws`, `group/azure`, `group/gcp`, `group/k8s`, `group/docker`, `group/vmware`, `group/os` (OpenStack), and any group declared in a collection's `meta/runtime.yml` `action_groups`.

### 15.3 Levels and merging

Settable at play, role, block, and task. Inner overrides outer (per module name). Module-specific defaults override group defaults. Task arguments override everything.

### 15.4 Caveat with roles

Play-level `module_defaults` **propagate into roles**. This may surprise you when a role assumes upstream defaults — set role-internal `module_defaults` defensively if needed.

---

## 16. Reserved / special keywords

### 16.1 `collections`

Specifies search namespaces for short-name modules / roles / lookups, in priority order:

```yaml
- hosts: all
  collections:
    - my_namespace.my_collection
    - community.general
  tasks:
    - my_module:    # resolved against the listed collections in order
        ...
```

`ansible.builtin` and `ansible.legacy` are always implicitly searched (legacy after the explicit list, builtin first). FQCNs bypass the search and are always preferred.

`collections` keyword applies at play and role level (and in `meta/main.yml`); does **not** apply to task-level for modules that already use FQCN.

### 16.2 `environment`

Sets env vars on the **remote** task execution. Dict at play / role / block / task level (innermost wins via merge). Common use: HTTP proxies for `apt`/`yum`/`pip`/`get_url`/`uri`.

```yaml
environment:
  http_proxy: "http://proxy:3128"
  HTTPS_PROXY: "http://proxy:3128"
  NO_PROXY: "127.0.0.1,localhost"
```

Limitations:
- Does not affect Ansible's own config or lookups (which run on the control node).
- Does not affect facts unless an explicit fact-gather task is run *with* the environment applied.
- Not a secrets channel — env vars are visible in process listings, command logs, etc.

For language-specific version managers (`nvm`, `rbenv`, `pyenv`):
```yaml
environment:
  NVM_DIR: /usr/local/nvm
  PATH: "/usr/local/nvm/versions/node/v20/bin:{{ ansible_env.PATH }}"
```

Care with `ansible_env.PATH`: that fact reflects the **remote_user / become_user that did the gather**, not the current task's user.

---

## 17. Complex data manipulation

### 17.1 List comprehensions

Use Jinja2's `|map`, `|select`, `|reject`, `|selectattr`, `|rejectattr`, plus `|list` to materialize:
```yaml
ips: "{{ hosts | map(attribute='ip') | list }}"
big: "{{ items | selectattr('size', 'gt', 100) | list }}"
even_squared: "{{ range(0, 10) | select('even') | map('pow', 2) | list }}"
```

### 17.2 Dict construction

- From pairs: `dict(list_of_tuples)`.
- From parallel lists: `dict(keys | zip(values))`.
- Alternating list: `dict(single_list[::2] | zip(single_list[1::2]))`.
- Progressive merge in a loop: `set_fact: result: "{{ result | combine({key: value}) }}"`.

### 17.3 `combine` deep-merge semantics

```yaml
out: "{{ a | combine(b, c, recursive=True, list_merge='append_rp') }}"
```

`list_merge`:
- `replace` — overwrite (default).
- `keep` — preserve original.
- `append` — concatenate (allow duplicates).
- `prepend` — concat with new first.
- `append_rp` — append, removing duplicates that exist in both.
- `prepend_rp` — prepend, removing duplicates that exist in both.

### 17.4 `hash_behaviour`

Global (config-only, deprecated to enable but still supported):
- `replace` (default) — same-named hashes from different precedence levels overwrite, not merge.
- `merge` — they deep-merge. Strongly discouraged because it produces hard-to-trace results.

### 17.5 JMESPath

`{{ data | community.general.json_query('users[?active].name') }}` — full JMESPath syntax for deep extraction.

### 17.6 `subelements`

Iterate over a list of dicts plus a sub-key list:
```yaml
loop: "{{ users | subelements('groups', skip_missing=True) }}"
# item.0 = the parent dict, item.1 = an element from .groups
```

---

## 18. Quirks and gotchas

### 18.1 YAML pitfalls

- **Norway problem**: `country: NO` parses as boolean `false`. Quote `"NO"`.
- `yes`, `no`, `Yes`, `No`, `True`, `False`, `on`, `off` (and old-style `Y`/`N`) all parse as booleans in YAML 1.1. Quote anything you need as a string.
- `1.0` is a float; `1.10` becomes `1.1`. Quote version strings: `"1.10"`.
- Octal: `mode: 0644` is parsed as decimal 644 (which is **not** `rw-r--r--`). Use `mode: "0644"` or `mode: 0o644`.
- Sexagesimal (in some YAML libs): `12:34:56` may parse as a number. Quote.
- A bare `*` in flow context begins an alias. Quote `"*"`.
- Trailing whitespace after `>` or `|` block indicators changes folding/keep behavior.
- `null`, `~`, empty value, and missing key all map to `None` in Python.

### 18.2 When variables evaluate

- `vars:` are stored unrendered; rendered each time they're used. A `vars` value referencing a host fact only resolves *after* facts gather.
- `set_fact` evaluates RHS at task execution and stores the concrete value.
- `register` stores the entire return dict at task completion.
- `import_*` filename templating must use vars present at parse time (extra-vars, inventory).
- `include_*` filename templating uses runtime vars (host vars, facts).

### 18.3 Fact precedence over user vars

Facts are precedence level 11 — they **beat** anything below (role defaults, all inventory and group/host vars). So you cannot override `ansible_distribution` from inventory; you'd have to use `set_fact` (level 19+).

`set_fact: ... cacheable: true` writes to the fact cache and is treated as a fact (level 11) on subsequent runs.

### 18.4 Handler de-duplication

- Notify the **same handler name** N times → handler runs once.
- Two handlers defined with the **same name** → only the last-loaded definition exists; earlier ones are silently shadowed.
- Handlers run in **definition order**, not notify order.

### 18.5 Notify by `listen` vs by name

Listen topics are useful for grouping (one notify hits many handlers; safer for cross-role plumbing because callers don't need to know exact handler names). Caveat: `listen` topic strings **cannot be Jinja-templated**.

### 18.6 `with_items` vs `loop`

`with_items` implicitly flattens one level; `loop` does not. Migrating without `flatten(levels=1)` will change behavior on lists-of-lists.

`with_*` are not deprecated, but new code should use `loop` + filters.

### 18.7 Type coercion gotchas

- Comparing strings as numbers: `when: result.rc == 0` works because `rc` is int. But `when: lsb.major_release >= 6` may compare strings; use `| int`.
- Boolean strings: `vars: enabled: "yes"` then `when: enabled` → truthy because non-empty string. Always: `when: enabled | bool`.
- Module return values: stringly-typed in some modules (`stdout` is always a string). Always cast.
- JSON-parsed numbers stay numeric; YAML-loaded `"1.10"` stays string.

### 18.8 `register` is always set

Even on `when: false` skip or fail, the registered variable exists. Use `is defined` only when you might not have *reached* the register task at all.

### 18.9 Block tag interaction

Tasks in `rescue:` or `always:` are skipped if the block's tag is not selected, even though logically they should clean up.

### 18.10 Imports lose `--start-at-task` flexibility for inner tasks

Plus, `--start-at-task` cannot find tasks inside *includes* (only imports).

### 18.11 `meta` quirks

- Some `meta` actions historically ignored `when:` (e.g. `flush_handlers` pre-2.x). Verify on current version.
- `meta: end_play` doesn't fire `always:` blocks of enclosing structures; `end_host` is per-host but other hosts continue.
- `clear_host_errors` revives unreachable hosts but doesn't re-attempt them automatically — the next task will.

### 18.12 Async & strategies

- `async` + `linear` strategy: the task barrier still applies — Ansible waits for all hosts' poll loops before next task.
- `async` + `free` strategy: each host advances independently; this is where async truly parallelizes.

### 18.13 Delegation surprises

- Facts gathered while delegated are stored against the original `inventory_hostname` *unless* `delegate_facts: true`.
- If you `delegate_to` a host not in inventory and never registered with `add_host`, connection vars come from the play's defaults — usually wrong.
- `local_action` uses `connection: local` which means **no SSH**; relative paths are relative to the control node.

### 18.14 Run-once weirdness

`run_once: true` inside a block + `when:` that's false on the chosen first host → the task is skipped *for the entire batch*. The "first host" selection is based on play order, not on which host the condition is true for.

### 18.15 Collections resolution surprise

`collections:` at play level affects only short-name lookups inside that play. Tasks that already use FQCN ignore it. Roles loaded by `collections:` honor it; external roles loaded via `roles_path` may not.

### 18.16 Empty list / dict default

`vars: my_list: []` and `vars: my_dict: {}` don't merge under `combine`/`union` — they just give an empty result. Use `default([])` / `default({})` defensively.

### 18.17 Pre/post task handler timing

Handlers notified from `pre_tasks` flush **before roles** start. Handlers notified inside `roles` flush **before `tasks:`**. Handlers notified inside `tasks:` flush **before `post_tasks`**. Handlers notified inside `post_tasks` flush at the very end. So handler timing is partitioned by these section boundaries — not arbitrary "later in the play".

### 18.18 Loops and `register.results` order

`results` list order matches the iteration order of the input list — guaranteed. But concurrent execution across hosts means you cannot use `register` order to reason across hosts.

### 18.19 `no_log` propagation

`no_log: true` at play/block/role propagates to children, but a child explicitly set to `no_log: false` will override and log. Sensitive data near `no_log` boundaries deserves audit.

### 18.20 Vault and lazy decryption

Vault-encrypted vars are decrypted lazily on access. A play that never references a vaulted secret never needs the password. This is occasionally a footgun: typos in var names silently skip decryption.

### 18.21 Error codes vs return states

Modules can return any of `ok`, `changed`, `failed`, `unreachable`, `skipped`, `rescued`, `ignored`. `register.failed` is true only for `failed`, not `unreachable`. To catch both: `register.failed or register.unreachable`.

### 18.22 `serial` and `any_errors_fatal` interaction

With `serial: 5`, `any_errors_fatal: true` applies to the **current batch of 5** — Ansible finishes that batch's failing task, halts the play across all five, and never starts the next batch.

---

## Appendix A — Example: full keyword bingo card

```yaml
- name: Update web tier
  hosts: webservers:&production
  order: shuffle
  serial:
    - 1
    - "20%"
  max_fail_percentage: 10
  any_errors_fatal: false
  strategy: linear
  throttle: 4
  force_handlers: true
  ignore_unreachable: false
  gather_facts: true
  gather_subset:
    - "!all"
    - "!min"
    - "network"
    - "hardware"
  gather_timeout: 30
  fact_path: /etc/ansible/facts.d
  remote_user: deploy
  connection: ssh
  port: 22
  become: true
  become_user: root
  become_method: sudo
  become_flags: "-H -S -n"
  collections:
    - mycorp.platform
  environment:
    HTTPS_PROXY: "{{ corp_proxy }}"
  module_defaults:
    ansible.builtin.file:
      owner: app
      group: app
      mode: "0640"
    group/aws:
      region: us-west-2
  vars:
    app_version: "1.4.2"
  vars_files:
    - vars/secrets.yml
  vars_prompt:
    - name: confirm
      prompt: "Type DEPLOY to proceed"
      private: false
  check_mode: false
  diff: true
  no_log: false
  tags: [deploy, web]

  pre_tasks:
    - name: Drain from LB
      delegate_to: lb01.corp
      run_once: true
      ansible.builtin.command: drain {{ inventory_hostname }}

  roles:
    - role: common
    - role: webserver
      vars:
        port: 8080
      tags: web

  tasks:
    - name: Pull artifact
      ansible.builtin.get_url:
        url: "https://artifacts/{{ app_version }}.tar.gz"
        dest: /tmp/app.tgz
      register: dl
      until: dl is succeeded
      retries: 5
      delay: 10
      async: 600
      poll: 5

    - name: Apply config
      block:
        - ansible.builtin.template:
            src: app.conf.j2
            dest: /etc/app/app.conf
          notify: restart app
        - ansible.builtin.command: validate-config
          changed_when: false
      rescue:
        - debug: msg="rolling back {{ ansible_failed_task.name }}"
        - ansible.builtin.copy:
            src: /etc/app/app.conf.bak
            dest: /etc/app/app.conf
      always:
        - meta: flush_handlers

  post_tasks:
    - name: Add back to LB
      delegate_to: lb01.corp
      run_once: true
      ansible.builtin.command: enable {{ inventory_hostname }}

  handlers:
    - name: restart app
      listen: app reload
      ansible.builtin.service:
        name: myapp
        state: restarted

    - name: reload nginx
      listen: app reload
      ansible.builtin.service:
        name: nginx
        state: reloaded
```

This single play exercises essentially every major play-level keyword and demonstrates the interaction of the subsystems in this document.

---

## Appendix B — Quick keyword scope table

Legend: P=play, R=role, B=block, T=task, H=handler.

| Keyword | P | R | B | T | H |
|---|---|---|---|---|---|
| `name` | ✔ | ✔ | ✔ | ✔ | ✔ |
| `hosts` | ✔ |   |   |   |   |
| `vars` | ✔ | ✔ | ✔ | ✔ | ✔ |
| `vars_files` | ✔ |   |   |   |   |
| `vars_prompt` | ✔ |   |   |   |   |
| `module_defaults` | ✔ | ✔ | ✔ | ✔ |   |
| `environment` | ✔ | ✔ | ✔ | ✔ |   |
| `collections` | ✔ | ✔ | ✔ | ✔ |   |
| `connection`, `port`, `remote_user`, `timeout` | ✔ | ✔ | ✔ | ✔ |   |
| `become`, `become_user`, `become_method`, `become_flags`, `become_exe` | ✔ | ✔ | ✔ | ✔ |   |
| `tags` | ✔ | ✔ | ✔ | ✔ |   |
| `when` |   | ✔ | ✔ | ✔ |   |
| `ignore_errors`, `ignore_unreachable` | ✔ | ✔ | ✔ | ✔ |   |
| `any_errors_fatal` | ✔ | ✔ | ✔ | ✔ |   |
| `force_handlers` | ✔ |   |   |   |   |
| `max_fail_percentage` | ✔ |   |   |   |   |
| `serial`, `strategy`, `order`, `gather_*`, `fact_path` | ✔ |   |   |   |   |
| `throttle` | ✔ |   | ✔ | ✔ |   |
| `run_once` | ✔ | ✔ | ✔ | ✔ |   |
| `no_log` | ✔ | ✔ | ✔ | ✔ | ✔ |
| `check_mode`, `diff` | ✔ | ✔ | ✔ | ✔ |   |
| `delegate_to`, `delegate_facts` |   | ✔ | ✔ | ✔ |   |
| `notify` |   |   | ✔ | ✔ |   |
| `register`, `loop`, `with_*`, `loop_control`, `until`, `retries`, `delay` |   |   |   | ✔ |   |
| `async`, `poll` |   |   |   | ✔ |   |
| `failed_when`, `changed_when` |   |   |   | ✔ |   |
| `args`, `action`, `local_action` |   |   |   | ✔ |   |
| `block`, `rescue`, `always` |   |   | ✔ |   |   |
| `pre_tasks`, `tasks`, `post_tasks`, `handlers`, `roles` | ✔ |   |   |   |   |
| `debugger` | ✔ | ✔ | ✔ | ✔ |   |
| `listen` |   |   |   |   | ✔ |

---

## Appendix C — Things deliberately *not* covered here

- Full module catalog (each module's args, returns, idempotency rules) — separate document.
- Plugin internals (action plugins, callback plugins, connection plugins, become plugins) beyond what's externally observable in playbooks.
- Inventory grammar (INI, YAML, scripts, dynamic plugins) — separate document.
- ansible-vault on-disk format and CLI workflow.
- Galaxy / collection packaging mechanics.
- ansible.cfg setting-by-setting reference.

These belong in their own research files for the runsible port.
