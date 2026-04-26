# Ansible Inventory Subsystem — Exhaustive Reference

Research source: every page under https://docs.ansible.com/ansible/latest/inventory_guide/, plus
the ansible-inventory CLI reference, the developing_inventory dev guide, the special_variables
reference, the configuration reference, and the variable precedence guide.

This document is the source-of-truth research for the `runsible-inventory` crate. It captures
*what Ansible actually does today* so we can decide what to keep, what to drop, and what to
reshape for the TOML-native rewrite.

---

## 1. Inventory File Formats

Ansible recognizes several built-in inventory file formats. Each is loaded by a corresponding
inventory plugin (`ini`, `yaml`, `toml`, `script`, `host_list`, `auto`, `constructed`). Default
plugin enable order is:

```
host_list, script, auto, yaml, ini, toml
```

Configured via `INVENTORY_ENABLED` (env `ANSIBLE_INVENTORY_ENABLED`, ini `[inventory] enable_plugins`).
Order matters: the first plugin whose `verify_file()` returns true wins for that source. Custom
collection plugins must be appended (or substituted) by the user.

### 1.1 INI format

The INI format is the historical default. Filename `hosts` with no extension, or `*.ini`.

#### Grammar

```ini
# Bare host (becomes a member of the implicit `ungrouped` group)
mail.example.com

# Group header
[webservers]
foo.example.com
bar.example.com

# Group of children (parent group composed of other groups)
[southeast:children]
atlanta
raleigh

# Group variables (string-typed, single key=value per line)
[atlanta:vars]
ntp_server=ntp.atlanta.example.com
proxy=proxy.atlanta.example.com
```

Section headers use `[name]` for hosts, `[name:children]` for nested groups, `[name:vars]` for
group-scoped variables. Group names follow the same rules as Python identifier-style variable
names (letters, digits, underscores, no leading digit, no hyphens recommended).

#### Inline host variables

```ini
[atlanta]
host1 http_port=80 maxRequestsPerChild=808
host2 http_port=303 maxRequestsPerChild=909
```

Multiple `key=value` pairs are allowed per host line. Values are parsed as **Python literals**
(string, int, float, list, dict, tuple, bool, `None`). Whitespace-containing values must be
quoted. This is asymmetric with `:vars` sections — values in `:vars` are always treated as
**strings**.

#### Port and aliases

```ini
badwolf.example.com:5309

# Alias `jumper` → real host 192.0.2.50 on port 5555
jumper ansible_port=5555 ansible_host=192.0.2.50
```

The first column is the *inventory hostname* (what `inventory_hostname` returns). It does not
have to be a real DNS name; `ansible_host` provides the actual connection target.

#### Range patterns

```ini
[webservers]
www[01:50].example.com         # numeric range, inclusive
www[01:50:2].example.com       # numeric range with stride 2 → www01,03,05,...49
db-[a:f].example.com           # alphabetic range a..f inclusive
```

Numeric ranges accept leading zeros which are preserved in the expansion. Both bounds are
inclusive. The third `:N` field is an optional stride. Alphabetic ranges expand single
characters in lexicographic order.

### 1.2 YAML format

Filename ends in `.yml`, `.yaml`, or matches a plugin's expected suffix. Loaded by the `yaml`
inventory plugin.

#### Grammar

The top-level keys are group names. Each group is an object with optional keys: `hosts`,
`children`, `vars`. Hosts are mappings whose keys are inventory hostnames and whose values are
either `null` (no inline vars) or a mapping of variables.

```yaml
ungrouped:
  hosts:
    mail.example.com:
webservers:
  hosts:
    foo.example.com:
    bar.example.com:
dbservers:
  hosts:
    one.example.com:
    two.example.com:
    three.example.com:
```

#### Host vars and group vars

```yaml
atlanta:
  hosts:
    host1:
      http_port: 80
      maxRequestsPerChild: 808
    host2:
      http_port: 303
      maxRequestsPerChild: 909
  vars:
    ntp_server: ntp.atlanta.example.com
    proxy: proxy.atlanta.example.com
```

#### Nested groups

```yaml
usa:
  children:
    southeast:
      children:
        atlanta:
          hosts:
            host1:
            host2:
        raleigh:
          hosts:
            host2:
            host3:
      vars:
        some_server: foo.southeast.example.com
        halon_system_timeout: 30
        self_destruct_countdown: 60
        escape_pods: 2
    northeast:
    northwest:
    southwest:
```

A child group is named under `children:`; its definition can sit either at the top level or
nested directly under the parent. Both styles are equivalent because hosts and groups are
*global*: defining them in two places merges, it does not duplicate.

#### Range patterns in YAML

```yaml
webservers:
  hosts:
    www[01:50].example.com:
    www[01:50:2].example.com:
```

Same range syntax as INI.

#### Inventory aliases in YAML

```yaml
hosts:
  jumper:
    ansible_port: 5555
    ansible_host: 192.0.2.50
```

### 1.3 TOML format

A TOML inventory plugin ships with Ansible. Filename **must** end in `.toml`. The format mirrors
YAML's `groups → {hosts, children, vars}` structure but using TOML tables.

```toml
[web]
children = ["apache", "nginx"]
vars = { http_port = 8080, myvar = 23 }

[web.hosts]
host1 = {}
host2 = { ansible_port = 222 }

# Equivalent verbose form
[web.hosts.host1]
[web.hosts.host2]
ansible_port = 222

[all.vars]
has_java = false
```

`[group]` is the group; `[group.hosts]`, `[group.vars]`, and a `children` array under the group
table mirror the YAML keys. The TOML format is fully supported by the `--toml` output flag of
`ansible-inventory`. Range expansion and the rest of the semantic layer behave identically to
INI/YAML.

### 1.4 Script-based dynamic inventory

The `script` plugin executes any file with the executable bit set, passing it CLI flags and
parsing JSON from stdout. The contract:

- `--list` → returns the entire inventory.
- `--host <hostname>` → returns the variables for one host.

#### --list output schema

```json
{
  "group001": {
    "hosts":    ["host001", "host002"],
    "vars":     { "var1": true },
    "children": ["group002"]
  },
  "group002": {
    "hosts":    ["host003", "host004"],
    "vars":     { "var2": 500 },
    "children": []
  }
}
```

Each group's value is either an *object* with `hosts`, `vars`, `children` (any of which may be
omitted) or a *bare list of host strings*. Empty `children` arrays may be omitted.

#### --host output schema

```json
{
  "VAR001": "VALUE",
  "VAR002": "VALUE"
}
```

A flat mapping of variable names to JSON-encoded values. May be empty.

#### `_meta.hostvars` optimization

To avoid one fork per host, `--list` may include a top-level `_meta` key:

```json
{
  "group001": { "hosts": ["host001", "host002"] },
  "_meta": {
    "hostvars": {
      "host001": { "var001": "value" },
      "host002": { "var002": "value" }
    }
  }
}
```

When `_meta.hostvars` is present (even empty: `"_meta": {"hostvars": {}}`), Ansible skips
calling the script with `--host` per host. Modern scripts almost always provide it.

#### Minimal valid output

```json
{
  "_meta": { "hostvars": {} },
  "all":   { "children": ["ungrouped"] },
  "ungrouped": { "children": [] }
}
```

The `all` group must include every host; `ungrouped` collects hosts with no other group.

### 1.5 Plugin-based dynamic inventory (interface contract)

Inventory plugins are Python classes. Configuration is a YAML file with `plugin: <name>` as the
single required key:

```yaml
plugin: myplugin
api_user: myuser
api_pass: mypass
api_server: myserver.example.com
cache_plugin: jsonfile
cache_timeout: 3600
```

#### Class contract

```python
from ansible.plugins.inventory import BaseInventoryPlugin, Constructable, Cacheable

class InventoryModule(BaseInventoryPlugin, Constructable, Cacheable):
    NAME = 'namespace.collection.myplugin'

    def verify_file(self, path):
        # Quick check — is this source mine to handle?
        if super().verify_file(path):
            return path.endswith(('myplugin.yml', 'myplugin.yaml'))
        return False

    def parse(self, inventory, loader, path, cache=True):
        super().parse(inventory, loader, path, cache)
        config = self._read_config_data(path)
        # ... call API, populate inventory
        self.inventory.add_group('webservers')
        self.inventory.add_host('host1', group='webservers')
        self.inventory.set_variable('host1', 'ansible_host', '10.0.0.1')
```

Required pieces:

- `NAME` class attribute. For collection plugins must be `namespace.collection.plugin_name`.
- `verify_file(path)` — fast yes/no on whether to handle `path`. Need not be 100 % accurate.
- `parse(inventory, loader, path, cache)` — main work. `inventory` exposes `add_group`,
  `add_child`, `add_host`, `set_variable`. Errors should raise `AnsibleParserError`.

Optional mixins:

- `Cacheable` — gives `self._cache`, `load_cache_plugin()`, `get_cache_key()`,
  `set_cache_plugin()`, `update_cache_if_changed()`, `clear_cache()`.
- `Constructable` — gives the `compose`, `groups`, `keyed_groups` options for synthesizing
  vars/groups from raw host data via `_set_composite_vars()`,
  `_add_host_to_composed_groups()`, `_add_host_to_keyed_groups()`.

#### The `auto` plugin

Since 2.5, the `auto` plugin matches any YAML file whose `plugin:` key names an installed
inventory plugin. This eliminates per-plugin file-extension tricks. `verify_file` is delegated
to the plugin matching `plugin:`.

### 1.6 Multiple inventory sources & merge precedence

Multiple `-i` flags or a directory inventory are merged in **load order**:

```bash
ansible-playbook get_logs.yml -i staging -i production
```

Ansible defines hosts, groups, and variables as it encounters them. Last writer wins for a given
variable. For directories: files are read **alphabetically, top-down**, recursively, so file
naming (`01-foo.yml`, `02-bar.yml`) controls precedence.

```
inventory/
  01-openstack.yml         # plugin config
  02-dynamic-inventory.py  # executable script
  03-static-inventory      # static INI/YAML
  group_vars/
    all.yml
```

Files are classified by:

- Executable bit set → run as `script` plugin source.
- Otherwise → static source, dispatched to `auto`/`yaml`/`ini`/`toml` per `enable_plugins`.

Default ignored extensions: `~`, `.orig`, `.bak`, `.ini`, `.cfg`, `.retry`, `.pyc`, `.pyo`, plus
the constants in `INVENTORY_IGNORE_EXTS` (env `ANSIBLE_INVENTORY_IGNORE`, ini
`[defaults] inventory_ignore_extensions` or `[inventory] ignore_extensions`). Default ignored
patterns via `INVENTORY_IGNORE_PATTERNS` (env `ANSIBLE_INVENTORY_IGNORE_REGEX`, ini
`[inventory] ignore_patterns`).

If `[all:vars] myvar=1` in `staging` and `myvar=2` in `production`:

- `-i staging -i production` → `myvar = 2`
- `-i production -i staging` → `myvar = 1`

Failure modes are configurable:

- `INVENTORY_UNPARSED_IS_FAILED` (default `False`) — fatal if **every** source fails.
- `INVENTORY_ANY_UNPARSED_IS_FAILED` (default `False`) — fatal if **any** source fails.

### 1.7 Default location & overrides

- Default path: `/etc/ansible/hosts`.
- Env var: `ANSIBLE_INVENTORY` (comma-separated).
- Config: `[defaults] inventory` (`DEFAULT_HOST_LIST`).
- CLI: `-i <path|comma-list>` (repeatable).

---

## 2. Host / Group Model

### 2.1 The two implicit groups: `all` and `ungrouped`

Even with no inventory at all, Ansible synthesizes:

- `all` — contains every host. `all` is the conceptual root of the parent/child tree.
- `ungrouped` — every host that has no other group membership.

These groups are always present but may be implicit; they often do not appear in `group_names`
listings.

### 2.2 Hosts in many groups

A host can belong to any number of groups. Grouping is "global": defining `host1` under both
`webservers` and `prod` does not create two host objects, it adds the same host to both groups.
The recommended grouping mental model is `What / Where / When`:

- **What** — the function (`webservers`, `dbservers`, `appservers`).
- **Where** — the location (`east`, `west`, `dc1`, `atlanta`).
- **When** — the lifecycle stage (`prod`, `staging`, `test`, `dev`).

### 2.3 Children semantics

Defining `[parent:children]` (INI) or `parent: { children: { child: ... } }` (YAML):

- Every host in `child` is automatically a host in `parent`.
- A group can have multiple parents AND multiple children.
- Circular relationships are forbidden.
- Hosts and groups are global identities; redefining merges, conflicting values overwrite by
  load order.

### 2.4 Variable scoping & loading

Variables can come from:

- Inline in the inventory file under a host or `[group:vars]`.
- A file in `host_vars/` named after the host.
- A directory in `host_vars/<hostname>/` containing one or more files.
- A file in `group_vars/` named after the group.
- A directory in `group_vars/<group>/` containing one or more files.

Path resolution is performed by the `host_group_vars` vars plugin, which searches relative to
**both** the inventory source directory and the playbook directory. When both exist, the
**playbook-relative** copy wins over the inventory-relative copy.

Files inside a host_vars/group_vars directory are loaded in **lexicographic order** and merged.

### 2.5 Variable precedence within inventory

From lowest to highest:

1. `all` group (the universal parent).
2. Parent groups (recursively, outermost first).
3. Child groups (innermost wins over its parent).
4. Host-level variables.

Among groups at the same depth (siblings of the same parent), groups merge in **alphabetical
order** unless overridden by `ansible_group_priority` (integer, default `1`, higher = merged
later = wins). `ansible_group_priority` is **only** valid in inventory sources, not in
`group_vars/` files.

Example:

```yaml
a_group:
  vars:
    testvar: a
    ansible_group_priority: 10
b_group:
  vars:
    testvar: b
```

Without `ansible_group_priority`, `b_group` wins (alphabetically later). With priority `10`,
`a_group` wins.

A child group's variables override its parent's.

---

## 3. Patterns (Host Targeting)

Patterns are the targeting mini-language for both `ansible <pattern> -m mod` and the
`hosts:` field in plays, plus `--limit`.

### 3.1 Operators

| Pattern              | Meaning                                                       |
|----------------------|---------------------------------------------------------------|
| `all` or `*`         | Every host in inventory.                                      |
| `host1`              | A specific host.                                              |
| `host1:host2`        | Union (OR). Same as `host1,host2`.                            |
| `groupname`          | All hosts in the group.                                       |
| `web*`               | Glob/wildcard match against host *or* group names.            |
| `192.0.*`            | Wildcards work on IP segments.                                |
| `*.example.com`      | FQDN wildcards.                                               |
| `~regex`             | A regular expression (Python `re` syntax) prefixed with `~`.  |
| `web:db`             | Union of two groups.                                          |
| `web:&staging`       | Intersection: in `web` AND in `staging`.                      |
| `web:!atlanta`       | Exclusion: in `web` but NOT in `atlanta`.                     |
| `webservers[0]`      | First host in the group (zero-indexed, sorted order).         |
| `webservers[-1]`     | Last host in the group.                                       |
| `webservers[0:2]`    | Hosts at indexes 0, 1, 2 (slice is **inclusive** on the end). |
| `webservers[1:]`     | From index 1 to the end.                                      |
| `webservers[:3]`     | From the start to index 3.                                    |

Comma is preferred over colon when ranges or IPv6 addresses are involved (since colons collide).

### 3.2 Combining

Patterns can be chained arbitrarily:

```
webservers:dbservers:&staging:!phoenix
```

= "(in `webservers` OR in `dbservers`) AND in `staging` AND NOT in `phoenix`."

### 3.3 Processing order

Operations are normalized regardless of write-order. Internally:

1. `:` and `,` (union) are accumulated.
2. `&` (intersection) is applied.
3. `!` (exclusion) is applied last.

So `a:b:&c:!d:!e == &c:a:!d:b:!e == !d:a:!e:&c:b`. Ansible documents this explicitly.

### 3.4 Templating in patterns

```
webservers:!{{ excluded }}:&{{ required }}
```

Jinja2 expressions are valid inside patterns and are evaluated before pattern resolution.

### 3.5 Quoting & gotchas

- On the shell, **single-quote** any pattern containing `!` to avoid bash history expansion:
  `--limit 'all:!host1'`.
- Patterns must match inventory: if a host or group is not present, you cannot pattern-target
  it. Unmatched patterns produce
  `[WARNING]: Could not match supplied host pattern, ignoring`.
- Patterns do not escape special characters automatically. Aliases must be referenced exactly
  as defined.

### 3.6 `--limit` interaction

`--limit <pattern>` further restricts whatever the play's `hosts:` field selected. It accepts
the full pattern grammar. `--limit @file.txt` loads patterns from a file (one per line);
`--limit @site.retry` is the auto-generated retry file form.

---

## 4. Magic Variables Exposed via Inventory

Variables Ansible always (or near-always) populates from the inventory subsystem:

| Name                          | Type   | Description                                                                  |
|-------------------------------|--------|------------------------------------------------------------------------------|
| `inventory_hostname`          | str    | The unique inventory key for the current host (alias, IP, or FQDN).         |
| `inventory_hostname_short`    | str    | Everything before the first dot in `inventory_hostname`.                     |
| `group_names`                 | list   | All groups the current host belongs to (excludes implicit `all`/`ungrouped`).|
| `groups`                      | dict   | All groups in inventory mapped to their member host lists.                   |
| `hostvars`                    | dict   | All hosts in inventory mapped to their variable dictionaries.                |
| `inventory_dir`               | str    | Directory of the inventory source where this host was first defined.         |
| `inventory_file`              | str    | File name of the inventory source where this host was first defined.         |
| `playbook_dir`                | str    | Directory of the currently executing playbook.                               |
| `ansible_play_hosts`          | list   | Hosts in the current play (not limited by `serial`).                         |
| `ansible_play_batch`          | list   | Hosts in the current `serial` batch.                                         |
| `ansible_play_hosts_all`      | list   | Every host originally targeted by the play, before failures.                 |
| `play_hosts`                  | list   | Deprecated alias of `ansible_play_batch`.                                    |
| `ansible_limit`               | str    | The raw `--limit` CLI value for this run.                                    |
| `ansible_inventory_sources`   | list   | All inventory sources used.                                                  |
| `omit`                        | sentinel | Magic value that, when used as an arg, removes the option entirely.        |

Plus the role-context magic variables `role_path`, `role_name`, `role_names`,
`ansible_role_name`, `ansible_parent_role_names`, `ansible_parent_role_paths` — these are not
inventory-sourced but appear alongside inventory data in templates.

### 4.1 Connection (behavioral) variables set via inventory

These are normal variables in name, but they have *special meaning* because Ansible reads them
to configure how the connection plugin attaches to the host:

| Variable                       | Purpose                                                                                |
|--------------------------------|----------------------------------------------------------------------------------------|
| `ansible_connection`           | Connection plugin: `ssh` (default), `paramiko`, `local`, `docker`, `kubectl`, `winrm`. |
| `ansible_host`                 | Real DNS name or IP if different from `inventory_hostname`.                            |
| `ansible_port`                 | Connection port (default 22 for SSH).                                                  |
| `ansible_user`                 | Remote user.                                                                           |
| `ansible_password`             | Auth password (vault this — never plaintext).                                          |
| `ansible_ssh_private_key_file` | Private key path.                                                                      |
| `ansible_ssh_common_args`      | Extra args appended to all of `ssh`, `scp`, `sftp`.                                    |
| `ansible_sftp_extra_args`      | Extra args appended to `sftp` only.                                                    |
| `ansible_scp_extra_args`       | Extra args appended to `scp` only.                                                     |
| `ansible_ssh_extra_args`       | Extra args appended to `ssh` only.                                                     |
| `ansible_ssh_pipelining`       | Override `pipelining` from `ansible.cfg`.                                              |
| `ansible_ssh_executable`       | Override the `ssh` binary path (added 2.2).                                            |
| `ansible_become`               | Force privilege escalation on/off.                                                     |
| `ansible_become_method`        | `sudo` / `su` / `doas` / etc.                                                          |
| `ansible_become_user`          | Target user for `become`.                                                              |
| `ansible_become_password`      | Password for `become`.                                                                 |
| `ansible_become_exe`           | Override the become binary path.                                                       |
| `ansible_become_flags`         | Flags appended to the become command.                                                  |
| `ansible_shell_type`           | Shell-syntax family on the target (default `sh`).                                      |
| `ansible_shell_executable`     | Shell binary on the target (default `/bin/sh`).                                        |
| `ansible_python_interpreter`   | Path to Python on target (`auto`, `auto_silent`, or absolute).                         |
| `ansible_*_interpreter`        | Generic shebang override for any language (Ruby, Perl, …); added 2.1.                  |

Example:

```ini
some_host         ansible_port=2222     ansible_user=manager
aws_host          ansible_ssh_private_key_file=/home/example/.ssh/aws.pem
freebsd_host      ansible_python_interpreter=/usr/local/bin/python
ruby_module_host  ansible_ruby_interpreter=/usr/bin/ruby.1.9.3
localhost         ansible_connection=local ansible_python_interpreter="/usr/bin/env python"
```

---

## 5. Variable Files Layout

### 5.1 File-vs-directory layouts

Both forms are equivalent; the directory form is for splitting by concern.

**File form:**

```
inventory/
  hosts                       # the inventory itself
  group_vars/
    all.yml                   # vars for the all group
    webservers.yml            # vars for the webservers group
    raleigh                   # extension-less is also OK
  host_vars/
    foosball.yml              # vars for one host
```

**Directory form:**

```
inventory/
  group_vars/
    raleigh/
      db_settings.yml         # loaded lexicographically
      cluster_settings.yml
    webservers/
      common.yml
      vault.yml               # encrypted file
  host_vars/
    foosball/
      settings1.yml
      settings2.yml
```

Files inside a host or group directory are read in lexicographic order and merged. The two
shapes can be mixed (e.g. `group_vars/all.yml` plus `group_vars/webservers/*.yml`).

### 5.2 Recognized extensions

`.yml`, `.yaml`, `.json`, or no extension at all. The `host_group_vars` vars plugin requires
YAML/JSON parseable content.

### 5.3 Inventory-relative vs playbook-relative

The `host_group_vars` plugin searches **both** roots:

1. The directory containing the inventory source.
2. The directory containing the playbook.

When the same variable is defined in both, the **playbook-relative** version wins. This is
critical: a per-environment `inventory/group_vars/db.yml` is overridden by the project's
`group_vars/db.yml` if the playbook lives in the project root.

### 5.4 Vault-encrypted files

Any of the variable files can be Ansible Vault-encrypted (entire-file or per-value via the
`!vault` YAML tag). Decryption happens lazily on access provided the right vault id/password is
available via `--vault-id`, `--ask-vault-pass`, or `--vault-password-file`. Vault interaction
is transparent to the inventory subsystem itself.

---

## 6. The `ansible-inventory` CLI

Sole purpose: introspect the resolved inventory. Useful for debugging plugins, scripts, and
merge precedence.

### 6.1 All flags

| Flag                                          | Purpose                                                                  |
|-----------------------------------------------|--------------------------------------------------------------------------|
| `-h`, `--help`                                | Show help.                                                               |
| `--version`                                   | Print Ansible version, config path, module paths.                        |
| `-v`, `-vv`, … `-vvvvvv`                      | Increase verbosity.                                                      |
| `-i`, `--inventory`, `--inventory-file PATH`  | Inventory source(s); repeatable; comma-separated also accepted.          |
| `-l`, `--limit SUBSET`                        | Restrict to a pattern.                                                   |
| `--flush-cache`                               | Clear the fact cache for every inventory host.                           |
| `--vault-id VAULT_IDS`                        | Vault identity (repeatable).                                             |
| `-J`, `--ask-vault-password`                  | Prompt for vault password.                                               |
| `--vault-password-file PATH`                  | Read the vault password from a file.                                     |
| `--playbook-dir BASEDIR`                      | Substitute playbook directory for relative role/group_vars resolution.   |
| `-e`, `--extra-vars`                          | Set extra vars (`key=value` or YAML/JSON); repeatable.                   |
| `--list`                                      | Output the entire inventory (default JSON).                              |
| `--host HOST`                                 | Output one host's variables (ignores `--limit`).                         |
| `--graph`                                     | Print a tree visualization. Argument may be a group name to root at.     |
| `-y`, `--yaml`                                | Use YAML output instead of JSON (ignored with `--graph`).                |
| `--toml`                                      | Use TOML output instead of JSON (ignored with `--graph`).                |
| `--vars`                                      | Include variables in the graph display (only with `--graph`).            |
| `--export`                                    | Optimize `--list` output for export rather than perfect Ansible-fidelity.|
| `--output OUTPUT_FILE`                        | Write `--list` output to a file instead of stdout.                       |

Environment overrides: `ANSIBLE_INVENTORY` (default inventory), `ANSIBLE_CONFIG`,
`ANSIBLE_INVENTORY_EXPORT` (default for `--export`).

### 6.2 Output shapes

**`--list` JSON** — same shape as a script plugin `--list` output: groups → `{hosts, vars,
children}` plus `_meta.hostvars` for per-host variables. With `--export`, the structure is
optimized for round-tripping rather than mirroring Ansible's internal merged view.

**`--list -y`** — same data in YAML form.

**`--list --toml`** — same data in TOML form (uses the TOML inventory schema).

**`--graph`** — text tree:

```
@all:
  |--@ungrouped:
  |--@webservers:
  |  |--foo.example.com
  |  |--bar.example.com
  |--@dbservers:
  |  |--one.example.com
```

With `--vars`, each line is annotated with variable assignments.

### 6.3 Behavioral notes

- `--host` ignores `--limit`.
- `--graph` ignores `--limit`, `--yaml`, `--toml`, and `--export`.
- `--export` is also surfaced as the `INVENTORY_EXPORT` config option (env
  `ANSIBLE_INVENTORY_EXPORT`, ini `[inventory] export`, default `False`).

---

## 7. Quirks, Gotchas, and Edge Cases

These are the things that will silently bite anyone reimplementing this.

### 7.1 Plugin enable order is order-of-attempt

`enable_plugins` is *both* the allowlist *and* the precedence list. The first plugin whose
`verify_file()` returns true claims the source. If `ini` comes before `yaml` and the file is
ambiguous (e.g. extensionless), the INI plugin wins. The default ordering
`host_list, script, auto, yaml, ini, toml` privileges:

1. Comma-list-on-CLI hosts (`host_list`).
2. Executable scripts (`script`).
3. YAML files declaring a `plugin:` key (`auto`).
4. Then file-format plugins by best guess.

### 7.2 INI inline vs `[group:vars]` value typing

This is the single most-confusing INI quirk:

- `host1 some_var=1` → `1` is a Python literal → integer.
- `[group:vars]` → `some_var=1` → string `"1"`.

So the same syntax produces different types depending on which section it lives in. Mass
converting an inline-host-vars file to `[group:vars]` will silently change types.

### 7.3 Range expansion is *inclusive on both ends*

`web[01:50]` produces 50 hosts (`web01` … `web50`), not 49. The `:N` stride field is optional.
Leading zeros are preserved.

### 7.4 Group merge order at the same depth

Default tie-break is **alphabetical**, so `a_group` merges *before* `b_group` and `b_group`
wins on conflict. Override only via inventory-source `ansible_group_priority` (higher = wins).
You **cannot** put `ansible_group_priority` in `group_vars/<group>.yml` — it must be inline
in the inventory file.

### 7.5 `DEFAULT_HASH_BEHAVIOUR` interacts with inventory merging

Default is `replace`: a later definition of a dict-typed variable replaces the earlier one
**entirely**. With `merge`, dict values are recursively merged. This setting is global and
deprecated; it changes inventory merging behavior in subtle ways and is widely considered a
foot-gun. `runsible` should pick a single semantic and stick to it.

### 7.6 host_vars / group_vars are loaded by a *separate vars plugin*

The `host_group_vars` plugin is what walks `host_vars/` and `group_vars/`. This is why files
declared next to the inventory *and* next to the playbook are both picked up — and why the
playbook copy wins.

### 7.7 Implicit `all` and `ungrouped` may be invisible to `group_names`

`group_names` for a host typically excludes `all` and `ungrouped`. Tests that assume "host has
no groups" can pass even when the host belongs to `ungrouped` because the magic variable
hides it.

### 7.8 Hosts and groups are global identities

Defining `host1` under two parents merges them; it does not produce two host objects. Multiple
inventory sources contribute to the *same* logical host. This means a typo in one source
(`host1.example.com` vs `host1`) silently creates two distinct hosts.

### 7.9 File extension auto-detection

Static file plugins typically need `.yml`, `.yaml`, `.json`, `.toml`, or `.ini`, and the `auto`
plugin requires `.yml`/`.yaml` plus a `plugin:` key. Default ignored extensions: those listed
in the Python constant `REJECT_EXTS` plus `.orig`, `.cfg`, `.retry`. Customize via
`INVENTORY_IGNORE_EXTS` and `INVENTORY_IGNORE_PATTERNS`.

### 7.10 Static groups of dynamic groups

When a static `[parent:children]` references a dynamic group (e.g. an EC2 tag), the dynamic
group must also be defined statically (typically as an empty header) **or** Ansible errors.
The cookbook is:

```ini
[tag_Name_staging_foo]

[tag_Name_staging_bar]

[staging:children]
tag_Name_staging_foo
tag_Name_staging_bar
```

The dynamic source then populates the empty groups at runtime.

### 7.11 Variable substitution / interpolation

Inventory file values are **not** Jinja2-templated at parse time. Templating happens later when
variables are evaluated against a host context (so `{{ inventory_hostname }}` inside an
inventory variable works, but only because evaluation is deferred).

### 7.12 Connection variable precedence is special-case

Behavioral inventory parameters like `ansible_user` follow the normal 22-level precedence for
*variable resolution*, but they also race against CLI flags (`-u`), playbook keywords
(`remote_user`), and config defaults (`DEFAULT_REMOTE_USER`) which use the playbook-keyword
precedence rules. Net effect: the same effective remote user can come from many places, and the
debug story is non-trivial.

### 7.13 Inventory plugin caching

Plugins that mix in `Cacheable` use a separate cache plugin (e.g. `jsonfile`, `redis`, `memory`)
configured via `cache_plugin` in the plugin YAML config, with a `cache_timeout`. Cache key is
derived from `(plugin_name, source_path)` via `get_cache_key()`. `--flush-cache` clears it.

### 7.14 The full 22-level variable precedence list (lowest → highest)

This is the master ordering Ansible documents for *all* variables (inventory ones occupy slots
3, 4, 6, 8, 9; the rest are non-inventory but matter for understanding what overrides what):

1. Command-line values (`-u my_user` etc., excluding `-e`).
2. Role defaults (`roles/*/defaults/main.yml`).
3. **Inventory file or script group vars.**
4. **Inventory `group_vars/all`.**
5. Playbook `group_vars/all`.
6. **Inventory `group_vars/*`.**
7. Playbook `group_vars/*`.
8. **Inventory file or script host vars.**
9. **Inventory `host_vars/*`.**
10. Playbook `host_vars/*`.
11. Host facts and cached `set_facts`.
12. Play vars.
13. Play `vars_prompt`.
14. Play `vars_files`.
15. Role vars (`roles/*/vars/main.yml`).
16. Block vars.
17. Task vars.
18. `include_vars`.
19. Registered vars and `set_facts`.
20. Role and `include_role` params.
21. `include` params.
22. Extra vars (`-e`) — always wins.

Inventory-sourced variables sit *below* almost everything else playbook-level. This means the
inventory layer is the foundation, not the override.

---

## 8. Configuration Reference (Inventory-Related)

| Setting                              | Env                                       | Ini key                                                  | Default                                          | Purpose                                                                |
|--------------------------------------|-------------------------------------------|----------------------------------------------------------|--------------------------------------------------|------------------------------------------------------------------------|
| `DEFAULT_HOST_LIST`                  | `ANSIBLE_INVENTORY`                       | `[defaults] inventory`                                   | `['/etc/ansible/hosts']`                         | Inventory source(s).                                                   |
| `INVENTORY_ENABLED`                  | `ANSIBLE_INVENTORY_ENABLED`               | `[inventory] enable_plugins`                             | `host_list, script, auto, yaml, ini, toml`       | Enabled plugins, in match-precedence order.                            |
| `INVENTORY_IGNORE_PATTERNS`          | `ANSIBLE_INVENTORY_IGNORE_REGEX`          | `[defaults] inventory_ignore_patterns` / `[inventory]`   | `[]`                                             | Regex patterns of files to skip when reading a directory.              |
| `INVENTORY_IGNORE_EXTS`              | `ANSIBLE_INVENTORY_IGNORE`                | `[defaults] inventory_ignore_extensions` / `[inventory]` | `REJECT_EXTS + ['.orig', '.cfg', '.retry']`      | Extensions to skip.                                                    |
| `INVENTORY_UNPARSED_IS_FAILED`       | `ANSIBLE_INVENTORY_UNPARSED_FAILED`       | `[inventory] unparsed_is_failed`                         | `False`                                          | Fatal if every source fails to parse.                                  |
| `INVENTORY_ANY_UNPARSED_IS_FAILED`   | `ANSIBLE_INVENTORY_ANY_UNPARSED_IS_FAILED`| `[inventory] any_unparsed_is_failed`                     | `False`                                          | Fatal if any source fails to parse.                                    |
| `INVENTORY_EXPORT`                   | `ANSIBLE_INVENTORY_EXPORT`                | `[inventory] export`                                     | `False`                                          | Default for `ansible-inventory --export`.                              |
| `DEFAULT_HASH_BEHAVIOUR`             | `ANSIBLE_HASH_BEHAVIOUR`                  | `[defaults] hash_behaviour`                              | `replace`                                        | `replace` overwrites dicts; `merge` recursively merges.                |
| Cache plugin (generic)               | `ANSIBLE_CACHE_PLUGIN`                    | `[defaults] fact_caching`                                | `memory`                                         | Cache plugin used by inventory caching when enabled.                   |

---

## 9. Implications for `runsible-inventory`

A few opinions to take into the implementation phase:

1. **TOML-first, not TOML-also.** Make TOML the canonical inventory file format; treat YAML/INI
   as importers behind feature flags. The TOML plugin's existing schema
   (`[group]`, `[group.hosts]`, `[group.vars]`, `children = [...]`) is a clean starting point.
2. **Drop `DEFAULT_HASH_BEHAVIOUR`.** Pick `replace` and never look back. Provide explicit merge
   constructs (e.g. `combine` filter equivalents in templates) when callers actually want a merge.
3. **Drop the `host_vars/` vs `group_vars/` *playbook-vs-inventory* split.** It's the source of
   "why is my variable wrong" tickets. Resolve relative to the inventory only; require explicit
   includes from playbook-side overrides.
4. **Keep range expansion (`web[01:50]`) and the `~regex` pattern prefix** — both are widely
   used and have no good ergonomic substitute.
5. **Modernize the dynamic inventory contract.** Instead of stdout-JSON-from-an-executable, use
   either a stable subprocess JSON-RPC or a Rust trait directly. Preserve the `_meta.hostvars`
   batching idea (one call returns everything) — it's the right design.
6. **Make implicit `all` / `ungrouped` first-class** rather than synthesized at the last
   moment. Putting them in `group_names` by default would surprise some users but is more
   honest than the current behavior.
7. **`ansible_group_priority` in `group_vars/` should work**, not silently no-op. If we keep
   merge tie-breaking, expose it everywhere.
8. **The 22-level precedence list is a smell.** Aim for ≤ 6 explicit tiers in `runsible`:
   `defaults → inventory → vars files → play vars → task/role params → CLI`. Map the rest
   into those.
9. **Vault is orthogonal.** Treat encrypted-at-rest variable files as a transparent lazy
   decryption layer over the variable file loader; do not bake it into the inventory plugin
   contract.
10. **`runsible-inventory` should expose a stable Rust API**: `Inventory::load(sources)`,
    `Inventory::resolve_pattern(pattern)`, `Inventory::host_vars(host)`, etc. The CLI surface
    (`runsible inventory --list/--graph/...`) should be a thin layer over that API, not the
    other way round (which is how Ansible ended up).
