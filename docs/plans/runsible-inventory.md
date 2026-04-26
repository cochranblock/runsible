# runsible — `runsible-inventory`

## 1. Mission

`runsible-inventory` defines hosts, groups them, attaches variables, decrypts vault'd values lazily, resolves pattern expressions to concrete host sets, and exposes the merged view as a stable Rust API to every other crate in the workspace. It is the foundation layer of variable resolution: every other source of vars layers on top of inventory, never under it. The CLI (`runsible-inventory --list/--host/--graph/...`) is a thin pass-through to the API; the API is the product.

## 2. Scope

**In:**
- Parse TOML inventories (canonical, source of truth).
- Parse INI inventories read-only, for migration; output is always TOML on round-trip.
- Parse YAML inventories read-only, via in-process `yaml2toml`.
- `host_vars/` and `group_vars/` file-and-directory layouts.
- Pattern matching: the full `web*:&prod:!staging` family, plus `~regex`, indexed `web[0:2]`, and globs.
- Merging multiple inventory sources (`-i a.toml -i b.toml -i ./inv-dir/`).
- Subprocess JSON contract for dynamic inventory, byte-compatible with Ansible's existing script protocol so user-space scripts keep working.
- The `runsible-inventory` CLI: `--list`, `--host`, `--graph`, `--toml`, `--yaml`, `--json`, `--export`, `--limit`, `--vault-recipients`, `--vars`/`--no-vars`.
- Range expansion (`web[01:50]`, `db-[a:f]`, `host[01:20:2]`) — leading zeros preserved, inclusive on both ends.
- Vault-encrypted inline vars (`{ vault = "..." }`) and vault-encrypted whole files in `host_vars/` and `group_vars/`.

**Out:**
- Python plugin-based inventory (`BaseInventoryPlugin` class hierarchy gone). All dynamic inventory uses the subprocess JSON contract; users write scripts in any language.
- Reading directly from cloud APIs in this crate — those are user-space subprocess scripts.
- Writing inventory back to a remote source. Round-trip parse → serialize is in-scope; on-disk mutation is not.
- Fact caching/gathering (lives in `runsible-facts`).
- Connection establishment (lives in `runsible-connection`). Behavioral vars (`ansible_host`, `ansible_user`, etc.) are just plain vars here.

## 3. The TOML inventory schema

A canonical example showing every shape:

```toml
# inventory/site.toml — [all] and [ungrouped] are implicit; may be declared.

[all.vars]
ntp_pool = "pool.ntp.org"
ansible_python_interpreter = "/usr/bin/python3"
db_root_password = { vault = "age1xyz...", payload = "...", recipients = ["ops"] }

[all.hosts]
"bastion.prod.example.com" = { ansible_user = "deploy", ansible_port = 22 }
"localhost" = { ansible_connection = "local" }

[webservers.vars]
http_port = 8080
worker_threads = 4
# Per-merge-site directive (§25 of poor-decisions); no global hash_behaviour.
nginx_config = { merge = "deep", server_name = "%h.example.com", listen = 80 }

[webservers.hosts]
"web[01:20]" = {}                          # range, inclusive both ends, zeros preserved
"web[21:25]" = { http_port = 8443 }        # per-host override

[prod]
children = ["webservers", "dbservers", "cache"]
[prod.vars]
environment = "prod"
log_level = "warn"

[dbservers.hosts]
"db01.prod" = { ansible_host = "10.0.5.10" }
"db02.prod" = { ansible_host = "10.0.5.11" }

[cache.hosts]
"redis-[a:c].prod" = {}                    # alphabetic range

# Multi-source manifest: each path loaded as a separate inventory, merged in order.
[sources]
paths = ["./inventory/aws-ec2.toml", "./inventory/onprem.toml", "./inventory/dynamic.sh"]
on_conflict = "error"                      # or "merge", "last_wins"
```

### 3.1 Inline vars are TOML scalars, full stop

Ansible's INI format has the worst quirk in the inventory ecosystem: in `[group]` sections, `host1 foo=1` types `foo` as a Python literal (so `1` is int, `[1,2]` is list, `'1'` is string). In `[group:vars]`, *the same syntax* yields a string. Two adjacent lines with the same shape produce different types — exactly the kind of footgun runsible exists to delete.

**runsible always treats inline vars as TOML scalars.** No Python-literal parser, no asymmetric typing. Want a string `"022"`? Write `port = "022"`. Want an int? `port = 22`. The TOML spec decides.

When the INI importer reads `host1 foo=1`, it emits TOML `foo = 1` and adds a comment `# was: Python-literal 1 from INI`. The importer logs every type-resolution decision for auditability.

### 3.2 Inline vs file-based vars

Inline vars and file-based vars (`group_vars/<group>.toml`, `host_vars/<host>.toml`, or directory variants) are equivalent in content and live at the same precedence level. Directory-form files load in **lexicographic order** and merge into one var dict.

Per §3 of poor-decisions, runsible drops Ansible's split between *inventory-relative* and *playbook-relative* var directories. Variable files resolve relative to the inventory source only. If a playbook needs to override an inventory variable, it does so explicitly via play `vars`, `vars_files`, or `--extra-vars`. This eliminates the most common "why is my variable wrong" debugging session in the Ansible tracker.

### 3.3 Encrypted vars

A vault'd value is a typed inline table, not a magic string:

```toml
[secrets.vars]
db_password = { vault = "age1qpz...", payload = "<base64 ciphertext>", recipients = ["ops", "ci"] }
api_key = { vault_file = "secrets/api_key.age" }
```

Two shapes accepted: inline `vault = "<recipients>"` + `payload = "<ciphertext>"` (round-trippable through `toml_edit`), or `vault_file = "<relative path>"` for larger secrets. Decryption is lazy — the loader produces an opaque `VaultedValue`; consumers call `.decrypt(&AgeKey)` on access. Without `--vault-recipients`, output retains the encrypted shape verbatim.

## 4. Pattern grammar

The full grammar accepted by `runsible -i inv.toml -l <pattern>`, `--limit <pattern>`, `runsible-inventory --limit <pattern>`, and the `hosts:` field of a play:

| Pattern              | Meaning                                                       |
|----------------------|---------------------------------------------------------------|
| `all` or `*`         | Every host in inventory.                                      |
| `host1`              | A specific host (exact name match).                           |
| `groupname`          | All hosts in the group.                                       |
| `web*`               | Glob match against host *or* group names.                     |
| `*.example.com`      | FQDN glob.                                                    |
| `192.0.*`            | IP-segment glob.                                              |
| `~web\d+`            | A regular expression, prefixed with `~`. Rust `regex` syntax. |
| `host1:host2`        | Union (OR). Same as `host1,host2`.                            |
| `web:db`             | Union of two groups.                                          |
| `web:&staging`       | Intersection: in `web` AND in `staging`.                      |
| `web:!atlanta`       | Exclusion: in `web` but NOT in `atlanta`.                     |
| `webservers[0]`      | First host (zero-indexed, sorted by host name).               |
| `webservers[-1]`     | Last host.                                                    |
| `webservers[0:2]`    | Slice; **inclusive on both ends** (so 3 hosts: 0, 1, 2).      |
| `webservers[1:]`     | From index 1 to end.                                          |
| `webservers[:3]`     | From start to index 3 inclusive.                              |
| `web*:&prod:!staging`| Combined chain.                                               |

### 4.1 Evaluation order

Mirroring Ansible so existing patterns port verbatim:

1. **Tokenize** on `:` and `,` at the top level; wildcards/regex/slice subscripts are atomic tokens.
2. **Resolve atoms** to host sets.
3. **Apply unions** — `:` and `,` accumulate.
4. **Apply intersections** — `:&` reduces.
5. **Apply exclusions** — `:!` subtracts last.

Rewriting term order does not change meaning: `a:b:&c:!d:!e == &c:a:!d:b:!e`. The engine emits a `pattern.normalized` NDJSON event so users can confirm the parse.

### 4.2 Quoting rules

- On the shell, **single-quote** any pattern containing `!`, `*`, `&`, `~`, or `:`.
- Inside TOML, the pattern is a TOML string; no special escaping.
- `{{ jinja }}` templates evaluate before pattern resolution; the result is reparsed as a pattern.

### 4.3 Unmatched patterns

Ansible silently warns. runsible **errors by default** — an unmatched pattern is a typo 95% of the time. `--allow-empty-pattern` opts back into the warning behavior.

## 5. Dynamic inventory

The contract is byte-compatible with Ansible's: an executable file producing JSON to stdout. Preserves the long tail of existing scripts (`ec2.py`, `gcp.py`, custom CMDB integrations).

**Detection.** A path is treated as dynamic if it has the executable bit set, has a `#!` shebang, or matches the first-4-bytes-are-`#!/` heuristic.

**The `--list` call.** runsible runs the script with `--list` and reads JSON of shape:

```json
{
  "_meta": { "hostvars": { "host001": { "var1": "v" }, "host002": { } } },
  "all":       { "children": ["webservers", "dbservers"] },
  "webservers": { "hosts": ["host001", "host002"], "vars": { "http_port": 80 } },
  "dbservers":  { "hosts": ["host003"], "vars": { "pg_version": "16" }, "children": [] },
  "ungrouped":  { "children": [] }
}
```

Each group's value is an object with `hosts`/`vars`/`children` (any omittable) or a bare list of hostnames (legacy, accepted).

**The `--host <name>` call.** If `_meta.hostvars` is absent, runsible falls back to invoking `--host <name>` once per host. If `_meta.hostvars` is present (even `{}`), per-host forks are skipped. Modern scripts always provide it.

**Errors.** A non-zero exit, non-JSON stdout, or timeout (default 60s, configurable via `[inventory.dynamic] timeout`) is logged as a parse failure. The Ansible `INVENTORY_(ANY_)UNPARSED_IS_FAILED` knobs become `[inventory] unparsed_is_failed = "any" | "all" | "none"`. Default `"none"`.

**Not supported.** Plugin-config YAML (`plugin: amazon.aws.aws_ec2`). These require a Python interpreter; runsible's wedge is "no Python in the controller." Users wanting `aws_ec2`-equivalents write or install a script in any language that emits JSON.

## 6. CLI surface

Every flag for the `runsible-inventory` binary:

```
runsible-inventory [GLOBAL FLAGS] [MODE] [OUTPUT] [SELECTION]
```

**Mode (mutually exclusive):**
- `--list` — dump the entire merged inventory.
- `--host <name>` — dump one host's resolved vars (ignores `--limit`).
- `--graph [<group>]` — render a hierarchical text graph, optionally rooted at a group.

**Inventory:**
- `-i, --inventory <path>` — repeatable; accepts a file or directory. Default: search for `./runsible.toml` `[inventory.path]`, then `./inventory/`, then `./hosts.toml`, then `./hosts`.
- `-l, --limit <pattern>` — restrict the output to hosts matching the pattern.

**Output format (with `--list` or `--host`):**
- `--toml` — TOML (canonical; round-trippable).
- `-y, --yaml` — YAML (lossy; emitted via TOML→YAML in `yaml2toml`).
- `--json` — JSON (default; matches the dynamic-inventory script schema for backward compat).
- `--output <path>` — write to a file rather than stdout.

**Vars in graph output:**
- `--vars` — include resolved vars per host/group line (only with `--graph`).
- `--no-vars` — explicitly suppress vars (only with `--graph`).

**Vault:**
- `--vault-recipients <recipients>` — decrypt vault'd vars in the output. Repeatable. Without this flag, vault'd values render in their encrypted shape (`{ vault = "...", payload = "..." }`).
- `--vault-key-file <path>` — explicit age private-key file. Default: read from `[vault] key_file` in `runsible.toml`.

**Export mode:**
- `--export` — apply the `host_vars/group_vars` file expansion to the output. Without this flag, `--list` emits the raw inventory tree as it was loaded; with `--export`, every host's vars are flattened from all sources (inventory + group_vars + host_vars + dynamic) into one `_meta.hostvars[<host>]` map. Useful for snapshotting.

**Variables:**
- `--vars` (default with `--list`) — include vars in the dump.
- `--no-vars` — exclude vars (output is just the host/group/children skeleton).
- `--extra-vars <toml>` / `-e <toml>` — repeatable; inject ad-hoc vars at level 4 (Runtime). These appear in `--list --export` output.

**Standard:**
- `-h, --help`, `--version`, `-v` (repeatable up to `-vvv` for debug logging).

**Removed from Ansible:**
- `--flush-cache` — runsible-inventory has no fact cache; facts live in `runsible-facts`.
- `--playbook-dir` — gone with the inventory-relative-vs-playbook-relative split.
- `-J / --ask-vault-password` — vault uses age recipients, not passwords. The migration helper (`runsible-vault import-ansible-password`) handles legacy vaults.

### 6.1 Output schemas

`--list --json` mirrors the dynamic-inventory `--list` schema (group-tree plus `_meta.hostvars`), so users can pipe runsible into anything that expects Ansible's script protocol.

`--list --toml` produces the §3 canonical schema, directly reusable as `runsible-inventory -i <output>`.

`--graph` renders a UTF-8 box-drawing tree (`├──`, `│`, `└──`); `--no-utf8` falls back to ASCII (`|--`). With `--vars`, each line is annotated with resolved key=value pairs.

## 7. Variable precedence (the runsible 5 levels)

Per §3 of poor-decisions, runsible collapses Ansible's 22 variable sources to **5 layers**. Higher numbers win:

1. **Project defaults** — `runsible.toml` `[defaults]`.
2. **Inventory** — everything in this crate's domain.
3. **Playbook** — play-level `vars`, task-level `vars`, role params, vars files.
4. **Runtime** — `--extra-vars` / `-e`, `--vars-file`, env-injected vars.
5. **Set-facts** — explicit, scoped to the play unless declared `[scope] global`.

Inventory is level 2 — the foundation. Within level 2, the substructure is:

### 7.1 Inventory sub-precedence (lowest → highest within level 2)

a. `all.vars`.
b. `<group>.vars` for ancestor groups, recursively, **outermost first** (children win over parents).
c. Sibling groups merge in **alphabetical order** by name. `ansible_group_priority` (int, default 1, higher = applied later) overrides alphabetical order. Per the inventory research §9.7, runsible honors this *anywhere* a group's vars can be declared, including `group_vars/<group>.toml`. Ansible's silent no-op in `group_vars/` was a documented bug we don't inherit.
d. `<group>.vars` from `group_vars/<group>.toml` and `group_vars/<group>/*.toml` (lexicographic). When inline and file both set the same key, file wins (file = "more specific" by stable convention).
e. `<host>` inline vars from `[<group>.hosts] foo = { var = ... }`.
f. `host_vars/<host>.toml` and `host_vars/<host>/*.toml` (lexicographic). Host vars win over group vars unconditionally.

### 7.2 Hash behavior

There is **no global `hash_behaviour` knob.** Per §25 of poor-decisions, the global toggle was a footgun. Merge behavior is declared **per merge site**:

```toml
[webservers.vars]
nginx_config = { merge = "deep", server_name = "%h.example.com", listen = 80 }

[prod.vars]
nginx_config = { server_name = "%h.prod.example.com", workers = 8 }
```

`merge = "deep"` recursively merges; `merge = "replace"` (default) replaces the parent's dict. If both sides specify, the child wins. `runsible explain-var <name> --host <host>` prints the resolution stack.

## 8. Data model (Rust types)

The public API the rest of runsible uses:

```rust
pub struct Inventory { sources: Vec<Source>, hosts: IndexMap<HostName, Host>, groups: IndexMap<GroupName, Group> }
pub struct Host  { pub name: HostName, pub vars: VarMap, pub groups: Vec<GroupName>, pub source: SourceRef }
pub struct Group { pub name: GroupName, pub vars: VarMap, pub hosts: Vec<HostName>, pub children: Vec<GroupName>, pub priority: i32 }

pub enum Pattern {
    All, Atom(Atom),
    Union(Vec<Pattern>),                       // : or ,
    Intersection(Vec<Pattern>),                // :&
    Exclusion(Box<Pattern>, Box<Pattern>),     // :!
}
pub enum Atom { HostExact(HostName), GroupExact(GroupName), Glob(GlobPattern), Regex(regex::Regex), Indexed { group: GroupName, slice: Slice } }

pub struct MergedView<'inv> { inventory: &'inv Inventory, extra_vars: VarMap }

impl Inventory {
    pub fn load(sources: &[InventorySource]) -> Result<Self, InventoryError>;
    pub fn parse_pattern(s: &str) -> Result<Pattern, PatternError>;
    pub fn hosts_matching(&self, pattern: &Pattern) -> Vec<&Host>;
    pub fn host(&self, name: &str) -> Option<&Host>;
    pub fn group(&self, name: &str) -> Option<&Group>;
    pub fn graph(&self) -> InventoryGraph;
    pub fn merged_view(&self, extra_vars: VarMap) -> MergedView<'_>;
    pub fn explain_var(&self, host: &str, var: &str) -> ResolutionStack;
}
impl MergedView<'_> {
    pub fn host_vars(&self, host: &str) -> VarMap;
    pub fn to_toml(&self, opts: SerializeOpts) -> String;
    pub fn to_yaml(&self, opts: SerializeOpts) -> String;
    pub fn to_json(&self, opts: SerializeOpts) -> String;
}
```

`VarMap` is `IndexMap<String, VarValue>`. `VarValue` is a tagged enum over scalar TOML values, arrays, tables, and `Vaulted(VaultedValue)`. The `Vaulted` variant carries recipients so `--vault-recipients` can pre-check decryption capability.

## 9. INI / YAML import

### 9.1 INI

Lossless parser covering every feature in §1.1 of the inventory research: `[group]` sections with ranges and alias-with-vars, `[group:children]`, `[group:vars]`, both `#` and `;` comment styles (the latter deprecated; normalized to `#` on TOML output), and inline-host-var typing — re-typed as TOML scalars with a per-decision audit log.

Exposed via:
- `runsible-inventory --toml < inv.ini` — INI on stdin, TOML on stdout.
- `runsible-inventory -i inv.ini --list --toml` — load and emit merged view.
- Library function `runsible_inventory::ini::parse(&str) -> Result<Inventory, IniError>`.

INI is read-only; there is no INI emitter. After import, the TOML is the source of truth.

### 9.2 YAML

YAML imports invoke `yaml2toml` in-process: read the YAML to `serde_yaml::Value`, pass to `yaml2toml::convert(value, Profile::Inventory)` (which knows inventory shapes), parse the resulting TOML. All YAML-quirk handling (norway problem, leading-zero preservation, unquoted-colon detection) lives in `yaml2toml` and is shared with playbook imports. `yaml2toml` errors surface unchanged with the file path attached.

## 10. CLI surface

(Merged with §6 above. `runsible-inventory`'s CLI is intentionally small; the heavy machinery lives in the library and the master `runsible` CLI surfaces the same flags via subcommands.)

## 11. Redesigns vs Ansible

Cited per §-numbering in `11-poor-decisions.md`:

- **§1 (TOML canonical, INI/YAML import only).** TOML is the source of truth. INI is read-only. YAML is read-only via `yaml2toml`. The canonical output of `--list` *is* TOML.
- **§3 (5-level precedence).** Inventory is level 2 of 5. The 22-level Ansible scheme exists in the migration guide as a translation reference, never at runtime.
- **§22 (no `collections:` keyword in inventory).** Ansible's `collections:` keyword propagates into inventory plugin name resolution in some versions and not others. runsible has no such keyword. Dynamic-inventory scripts are explicit paths or names registered in `runsible.toml` `[inventory.scripts]`. The lexical `[imports]` block (used in playbooks) does not apply to inventory — there is nothing to namespace-import.
- **§25 (per-merge-site `merge =` instead of global `hash_behaviour`).** Detailed in §7.2. No `hash_behaviour` knob exists. The `ansible.cfg` → `runsible.toml` importer warns when `hash_behaviour = merge` is set and emits a checklist of vars likely needing explicit `merge = "deep"`.

Additional redesigns from §9 of the inventory research:

- Implicit `all` and `ungrouped` are first-class — they appear in `group_names`, `--graph`, and `--list`. Ansible's hiding was a UX surprise we don't repeat.
- The inventory-relative vs playbook-relative `host_vars/group_vars` split is gone. One search root: the inventory directory.
- `ansible_group_priority` works in `group_vars/` files, not just inline.
- Dynamic inventory is subprocess JSON only. No Python plugin classes.
- Vault is a transparent lazy-decryption layer over `VarMap`, not a special inventory plugin. Crypto lives in `runsible-vault`.

## 12. Milestones

**M0 (alpha wk 4) — TOML + pattern matcher + basic CLI.** `toml_edit`-backed parser; canonical schema (§3) implemented; range expansion; pattern parser + evaluator (§4 operators); `--list` (JSON) and `--host`; round-trip golden tests.

**M1 (alpha wk 8) — INI, dynamic inventory, vars files, vault.** INI parser with full lossless coverage (§9.1); subprocess JSON dynamic inventory (`--list` + `--host` + `_meta.hostvars` optimization); `host_vars/` and `group_vars/` file and directory layouts; lazy vault decryption via `runsible-vault`; `--vault-recipients`.

**M2 (alpha wk 12) — Graph, export, plugin extension points.** `--graph` (UTF-8/ASCII, `--vars`, group-rooted); `--export` snapshotting; documented subprocess plugin protocol (vars providers, etc.) — JSON over stdin/stdout, same shape as dynamic inventory; performance pass: `--list` on 10k hosts under 200ms cold, 50ms warm.

## 13. Dependencies on other crates

- **`runsible-vault`** — `Vaulted::decrypt(&AgeKey)`. All crypto lives there; we carry `Vaulted` as an opaque type.
- **`runsible-config`** — reads `runsible.toml` `[inventory]` and `[defaults]` (defaults for `-i`, `unparsed_is_failed`, dynamic-script timeout).
- **`yaml2toml`** — in-process YAML inventory imports.
- **(Dev only) `runsible-test`** — corpus harness.

External: `toml_edit` (round-trip parsing), `serde` + `serde_json` (script protocol), `regex` (regex atoms), `globset` (wildcard atoms), `indexmap` (deterministic ordering), `tracing` (structured logs).

## 14. Tests

Structured around the failure modes that hurt Ansible users most:

**14.1 Round-trip TOML.** Every fixture in `tests/fixtures/toml/` parses → serializes via `to_toml(Lossless)` → reparses → asserts identity. Fixtures cover minimal one-host, deeply nested children, range expansion, vault'd vars, `[sources]` blocks, magic-var emission.

**14.2 INI golden tests.** `tests/fixtures/ini/` contains a curated corpus from upstream Ansible tests plus the top 30 GitHub INI inventories (anonymized). For each: parse INI → serialize TOML → diff against `.expected.toml`. This catches the long tail of Ansible quirks (python-literal typing, `[group:children]` interactions, alias-with-inline-vars).

**14.3 Pattern matching.** Table-driven over every operator in §4 against a synthetic 50-host, 12-group inventory with deliberate naming overlap (host `web01` and group `web01_legacy`). Includes the `:&`/`:!` reorder property (`a:b:&c:!d == &c:a:!d:b`), empty-match patterns, and templated patterns.

**14.4 Dynamic inventory contract.** `tests/fixtures/dynamic/` has scripts in Bash, Python, and Rust. Each is run through `--list --json` with: `_meta.hostvars` present (assert no `--host` calls), `_meta.hostvars` absent (assert one `--host` call per host), non-zero exit (assert configured `unparsed_is_failed`), timeout (assert honored).

**14.5 Variable precedence.** A fixture sets `the_var` at every level (defaults, `all.vars`, parent-group inline, parent `group_vars/`, child inline, child `group_vars/`, host inline, `host_vars/`, `--extra-vars`, `set_fact!`). Asserts the intended winner per case. Includes the `ansible_group_priority` regression: two sibling groups, one with priority 10 — wins regardless of alphabetical order, both inline and in `group_vars/`.

**14.6 Vault integration.** A vault'd inline var renders encrypted without `--vault-recipients` and decrypted with the right key; a vault'd `host_vars/` file behaves the same; a missing or wrong key produces an error naming the var, host/group, file path, and recipients.

## 15. Risks

**INI import correctness on real Ansible content.** The corpus harness (§14.2) is the mitigation; CI fails any PR breaking a fixture. Reputational risk: if `runsible-inventory -i mycorp.ini` mis-parses a well-known inventory, that's a first impression we don't get back. Budget 1-2 weeks of focused INI iteration during M1.

**Pattern grammar edge cases.** `:&` and `:!` precedence is the most likely subtle bug. Every §14.3 row is named after the ambiguity it pins down. Ansible's documentation is the spec; any disagreement is a bug in runsible (not a redesign).

**Informative vault decryption errors.** Missing keys, expired recipients, corrupted ciphertexts must name the variable, host/group, file path, and recipients. A bare "decryption failed" is unacceptable.

**Dynamic inventory latency.** A misconfigured script (no `_meta.hostvars`, 1k hosts → 1k forks) balloons `--list` to minutes. We log a warning when `_meta.hostvars` is absent above a configurable threshold (default 50). Fix is script-side; we make it visible.

**Magic var leakage.** `inventory_hostname`, `groups`, `hostvars`, `inventory_dir` etc. must be populated consistently — a wrong-layer lookup wastes hours of debugging. Dedicated test per documented magic var (§4 of `03-inventory.md` lists 16).

## 16. Open questions

**Q1: `[sources]` blocks vs `-i` CLI form only?** For: a project's inventory definition lives in one declarative place. Against: conflates "where inventory lives" with "what inventory is"; a footgun if `[sources]` and inline `[all.hosts]` mix in one file. Lean: support `[sources]`, warn on mixed manifest+content files.

**Q2: Host appears in multiple inventories — error or merge?** Ansible silently merges (last writer wins). This produces "why did prod vars land on a staging host" tickets. Lean: error by default with `--allow-host-overlap` to opt into merge; ship a `--check-overlap` audit helper.

**Q3: Implicit `all` parent or require explicit declaration?** Implicit is tax-free; no one writes it. Explicit prevents "why is this var here, I forgot `all` existed" debugging. Lean: keep implicit but render the edges visibly in `--graph` (dimmer color or `(implicit)` annotation) and in `explain-var`. Implicit-but-visible is the right balance.
