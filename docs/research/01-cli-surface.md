# Ansible CLI Surface — Reference for runsible

Exhaustive command-line surface for every Ansible CLI binary, distilled from
docs.ansible.com/ansible/latest/cli/ and adjacent guides. One section per
binary. Used as the source of truth when carving the runsible crate boundaries.

The 12 binaries covered (one runsible crate per binary):

| # | Binary | Crate role |
|---|--------|-----------|
| 1 | `ansible` | ad-hoc task runner |
| 2 | `ansible-playbook` | playbook executor |
| 3 | `ansible-galaxy` | role/collection package manager |
| 4 | `ansible-vault` | secrets at rest |
| 5 | `ansible-inventory` | inventory introspection |
| 6 | `ansible-doc` | plugin/module documentation |
| 7 | `ansible-config` | configuration view/init/validate |
| 8 | `ansible-console` | interactive REPL |
| 9 | `ansible-pull` | VCS-pull executor |
| 10 | `ansible-test` | dev-time test harness |
| 11 | `ansible-connection` | internal persistent-connection daemon |
| 12 | `ansible-lint` | static analysis (separate project) |

Cross-cutting flags (carried by almost every binary that touches hosts) are
defined once in the `ansible` section and referenced by name elsewhere; per-
binary sections list only what is novel or what overrides defaults.

---

## 1. `ansible`

### Purpose
Run a single ad-hoc task ("module invocation") against a host pattern. It is
the imperative one-shot of the toolkit: pick a pattern, pick a module, pass
`-a "k=v ..."`, get parallel execution across the matched inventory. Default
module is `command`, so `ansible web -a "uptime"` works without `-m`. Output is
per-host JSON-ish text on stdout; non-zero exit signals at least one host
failure. There is no playbook concept here; one run, one task.

### Synopsis
```
ansible [-h] [--version] [-v] [-b] [--become-method BECOME_METHOD]
        [--become-user BECOME_USER]
        [-K | --become-password-file BECOME_PASSWORD_FILE]
        [-i INVENTORY] [--list-hosts] [-l SUBSET] [--flush-cache]
        [-P POLL_INTERVAL] [-B SECONDS] [-o] [-t TREE]
        [--private-key PRIVATE_KEY_FILE] [-u REMOTE_USER]
        [-c CONNECTION] [-T TIMEOUT]
        [--ssh-common-args SSH_COMMON_ARGS]
        [--sftp-extra-args SFTP_EXTRA_ARGS]
        [--scp-extra-args SCP_EXTRA_ARGS]
        [--ssh-extra-args SSH_EXTRA_ARGS]
        [-k | --connection-password-file CONNECTION_PASSWORD_FILE]
        [-C] [-D] [-e EXTRA_VARS] [--vault-id VAULT_IDS]
        [-J | --vault-password-file VAULT_PASSWORD_FILES]
        [-f FORKS] [-M MODULE_PATH] [--playbook-dir BASEDIR]
        [--task-timeout TASK_TIMEOUT] [-a MODULE_ARGS]
        [-m MODULE_NAME] pattern
```

### Subcommands
None. Pattern is positional; module is selected via `-m`.

### Flags

Identification / help
- `-h, --help` — show help and exit
- `--version` — print version, config-file path, module search paths, executable path, and exit
- `-v, --verbose` — repeatable; `-v` through `-vvvvvv`. `-vvv` is the recommended debug start; `-vvvv` adds connection debugging

Inventory / host selection
- `-i, --inventory, --inventory-file INVENTORY` — path or comma-separated list; repeatable; env `ANSIBLE_INVENTORY`
- `-l, --limit SUBSET` — narrow the matched pattern further
- `--list-hosts` — print matched hosts and exit, no execution
- `--flush-cache` — drop fact cache for every targeted host before run

Module / task
- `-m, --module-name MODULE_NAME` — module to invoke; default `command`
- `-a, --args MODULE_ARGS` — module args, `key=value` space-separated or JSON
- `-M, --module-path MODULE_PATH` — colon-separated, repeatable; prepended to library; default `${ANSIBLE_HOME}/plugins/modules:/usr/share/ansible/plugins/modules`; env `ANSIBLE_LIBRARY`

Connection
- `-c, --connection CONNECTION` — connection plugin; default `ssh`
- `-u, --user REMOTE_USER` — remote user; default unset (SSH chooses)
- `-T, --timeout TIMEOUT` — connect timeout (seconds); default per-plugin
- `--private-key, --key-file PRIVATE_KEY_FILE` — SSH key file
- `-k, --ask-pass` — prompt for connection password
- `--connection-password-file CONNECTION_PASSWORD_FILE` — read connection password from file (mutually exclusive with `-k`)
- `--ssh-common-args` — extra args for ssh/scp/sftp (e.g. `ProxyCommand`)
- `--ssh-extra-args` — extra args for ssh only
- `--scp-extra-args` — extra args for scp only
- `--sftp-extra-args` — extra args for sftp only

Privilege escalation (become)
- `-b, --become` — enable become
- `--become-method BECOME_METHOD` — default `sudo`; valid choices via `ansible-doc -t become -l`
- `--become-user BECOME_USER` — default `root`
- `-K, --ask-become-pass` — prompt for become password
- `--become-password-file BECOME_PASSWORD_FILE` — file path (mutually exclusive with `-K`)

Async / concurrency
- `-f, --forks FORKS` — parallel workers; default `5`
- `-B, --background SECONDS` — run task asynchronously; abort after N seconds
- `-P, --poll POLL_INTERVAL` — poll interval (seconds) when `-B` set; default `15`
- `--task-timeout TASK_TIMEOUT` — per-task timeout in seconds; positive int

Modes
- `-C, --check` — predict changes without executing them
- `-D, --diff` — show file/template diffs (use with `--check` for dry-run with diff)

Variables / vault
- `-e, --extra-vars EXTRA_VARS` — `k=v`, YAML/JSON inline, or `@file.yml`; repeatable
- `--vault-id VAULT_IDS` — vault identity; repeatable
- `-J, --ask-vault-password, --ask-vault-pass` — prompt for vault password
- `--vault-password-file, --vault-pass-file VAULT_PASSWORD_FILES` — file (mutually exclusive with `-J`)

Output
- `-o, --one-line` — condense each host's output to a single line
- `-t, --tree TREE` — write per-host JSON output trees into directory

Path
- `--playbook-dir BASEDIR` — synthetic playbook root for resolving `roles/`, `group_vars/`, `host_vars/` even though no playbook is present

### Environment variables (selected)
- `ANSIBLE_INVENTORY`, `ANSIBLE_LIBRARY`, `ANSIBLE_CONFIG`
- Anything settable in `ansible.cfg` has an `ANSIBLE_*` env equivalent

### Exit codes
Not formally documented. Empirically: `0` success, non-zero on at least one
host failure or unreachable. Specific codes used by the Python runner: `1`
generic error, `2` parse / unprocessable, `3` host unreachable, `4` parser
error, `5` bad options, `99` user-interrupted, `250` unexpected.

### Common patterns
- `ansible all -m ansible.builtin.ping`
- `ansible atlanta -a "/sbin/reboot" -f 10 -u username --become --ask-become-pass`
- `ansible webservers -m ansible.builtin.copy -a "src=/etc/hosts dest=/tmp/hosts"`
- `ansible all -m ansible.builtin.setup` (dump facts)
- `ansible localhost -m ansible.builtin.apt -a "name=apache2 state=present" -b -K`

### Quirks / gotchas
- The host pattern is **positional and required**, unlike `ansible-playbook`
  which takes a playbook file positionally instead.
- Default module is `command`, which silently rejects shell features (`|`,
  `>`, `&&`, env vars). Need `-m shell` for those.
- `-K` and `--become-password-file` are mutually exclusive. Same for `-k`
  vs. `--connection-password-file`, and `-J` vs. `--vault-password-file`.
- `--playbook-dir` exists on a tool that has no playbook concept; it is
  purely a hint for relative path resolution.
- `-B`/`-P` puts the task in async-poll mode — radically different exit
  semantics from a normal sync run.

---

## 2. `ansible-playbook`

### Purpose
Run one or more YAML playbook files against an inventory, executing plays in
order, dispatching tasks per play with a strategy plugin. This is the workhorse
binary; it consumes the same connection/become/vault flags as `ansible` but
swaps the ad-hoc `-m`/`-a` for a positional list of playbook files plus a
richer set of selection flags (`--tags`, `--skip-tags`, `--start-at-task`,
`--step`, `--list-tasks`, `--syntax-check`).

### Synopsis
```
ansible-playbook [-h] [--version] [-v] [--private-key PRIVATE_KEY_FILE]
                 [-u REMOTE_USER] [-c CONNECTION] [-T TIMEOUT]
                 [--ssh-common-args SSH_COMMON_ARGS]
                 [--sftp-extra-args SFTP_EXTRA_ARGS]
                 [--scp-extra-args SCP_EXTRA_ARGS]
                 [--ssh-extra-args SSH_EXTRA_ARGS]
                 [-k | --connection-password-file CONNECTION_PASSWORD_FILE]
                 [--force-handlers] [-b]
                 [--become-method BECOME_METHOD]
                 [--become-user BECOME_USER]
                 [-K | --become-password-file BECOME_PASSWORD_FILE]
                 [-t TAGS] [--skip-tags SKIP_TAGS] [-C] [-D]
                 [-i INVENTORY] [--list-hosts] [-l SUBSET]
                 [--flush-cache] [-e EXTRA_VARS] [--vault-id VAULT_IDS]
                 [-J | --vault-password-file VAULT_PASSWORD_FILES]
                 [-f FORKS] [-M MODULE_PATH] [--syntax-check]
                 [--list-tasks] [--list-tags] [--step]
                 [--start-at-task START_AT_TASK]
                 playbook [playbook ...]
```

### Subcommands
None. Multiple playbook files may be given positionally; they execute in order.

### Flags
All connection, become, vault, inventory, and SSH-args flags from `ansible`
apply (`-i`, `-l`, `--list-hosts`, `--flush-cache`, `-c`, `-u`, `-T`, `-k`,
`--connection-password-file`, `--private-key`, `-b`, `-K`, `--become-method`,
`--become-user`, `--become-password-file`, `--ssh-common-args`, `--ssh-extra-args`,
`--scp-extra-args`, `--sftp-extra-args`, `-e`, `--vault-id`, `-J`,
`--vault-password-file`, `-f`, `-M`, `-C`, `-D`, `-v`, `--version`, `-h`).

Novel to `ansible-playbook`:
- `-t, --tags TAGS` — only run tasks/plays with matching tags; comma-separated; repeatable
- `--skip-tags SKIP_TAGS` — inverse of `--tags`; repeatable
- `--list-tasks` — print all tasks that would run, no execution
- `--list-tags` — print all tags found across the playbook(s)
- `--syntax-check` — parse and validate; do not run
- `--step` — interactive: confirm each task before it runs
- `--start-at-task START_AT_TASK` — skip until a task whose name matches
- `--force-handlers` — run handlers even if a task in the same play failed

Removed/absent (vs. `ansible`):
- `-m`, `-a` (no ad-hoc module)
- `-B`, `-P`, `-o`, `-t TREE` (no async, no one-line, no tree output)
- `--task-timeout` (set in playbook YAML instead)
- `--playbook-dir` (the playbook itself defines the dir)

### Environment variables
- `ANSIBLE_INVENTORY`, `ANSIBLE_LIBRARY`, `ANSIBLE_CONFIG`
- All `ANSIBLE_*` mirrors of `ansible.cfg`. Notables: `ANSIBLE_FORCE_HANDLERS`,
  `ANSIBLE_VAULT_PASSWORD_FILE`, `ANSIBLE_TAGS`, `ANSIBLE_SKIP_TAGS`.

### Exit codes
Same Python-runner convention as `ansible`. Specifically `0` for clean run,
`2` for any task failure, `3` for unreachable hosts, `4` for parser error,
`5` for bad CLI options, `8` for ctrl-c during play, `99` user-interrupted.

### Common patterns
- `ansible-playbook -i inventory site.yml`
- `ansible-playbook -i inv -u my_user -k -f 3 -T 30 -t my_tag -M ./modules -b -K my_playbook.yml`
- `ansible-playbook site.yml --syntax-check`
- `ansible-playbook site.yml --list-tasks`
- `ansible-playbook site.yml --start-at-task "configure nginx" --check --diff`

### Quirks / gotchas
- Playbook(s) are **positional and required**; `ansible-playbook` with no
  argument errors out unlike `ansible` (which only requires a pattern).
- `-t` here is `--tags` (not `--tree` as in `ansible`).
- `--check` plus `--diff` is the canonical dry-run; some modules silently
  skip in check mode and emit `skipped`, not `changed`.
- Multiple `-i` flags are merged; multiple playbook positionals run sequentially.
- `--force-handlers` is play-scoped at the CLI; YAML `force_handlers: true`
  on the play level overrides regardless.

---

## 3. `ansible-galaxy`

### Purpose
Package manager for Ansible content. Two top-level "types" — `collection`
(modern, namespaced bundles distributed via Galaxy or Automation Hub) and
`role` (legacy, single-purpose units). Each type has a verb tree (`install`,
`init`, `list`, `build`, `publish`, `download`, `verify`, `remove`, `import`,
`search`, `info`, `setup`, `delete`). Talks to a Galaxy API server, supports
requirements files, GPG signature verification (collections only), and offline
install from tarballs.

### Synopsis
```
ansible-galaxy [-h] [--version] [-v] {collection,role} <action> [opts] [args]
```

### Subcommands
- `collection download` — fetch collections + dependencies as tarballs
- `collection init <namespace.name>` — scaffold collection skeleton
- `collection build` — build collection artifact tarball for upload
- `collection publish <tarball>` — push tarball to Galaxy
- `collection install <name|tarball|url|...>` — install collection(s)
- `collection list` — list installed collections
- `collection verify <name>` — verify checksums and (optionally) GPG sigs
- `role init <name>` — scaffold role skeleton
- `role install <name|file|url|...>` — install role(s)
- `role list` — list installed roles
- `role remove <name>` — remove installed role
- `role info <name>` — show role + Galaxy metadata
- `role search` — search Galaxy server for roles
- `role import <github_user> <github_repo>` — register repo on Galaxy
- `role setup` — manage GitHub/Travis integration for roles
- `role delete <github_user> <github_repo>` — delete role from Galaxy

### Flags

Parent
- `-h, --help`, `--version`, `-v/--verbose`

Server / API (almost every action)
- `-s, --server API_SERVER` — Galaxy API server URL
- `--token, --api-key API_KEY` — Galaxy API key
- `-c, --ignore-certs` — skip SSL verification
- `--timeout TIMEOUT` — server wait, default `60` seconds

`collection install` — full novel set
- `-r, --requirements-file FILE` — requirements YAML
- `-p, --collections-path PATH` — install root
- `-f, --force` — overwrite existing
- `--force-with-deps` — force overwrite collection and its dep tree
- `-i, --ignore-errors` — keep going on per-item failure (does not bypass dep conflicts)
- `-n, --no-deps` — skip dependency download
- `-U, --upgrade` — upgrade installed
- `--pre` — include pre-release versions
- `--offline` — install only from local tarballs
- `--no-cache` — bypass server response cache
- `--clear-response-cache` — wipe cache then proceed
- `--disable-gpg-verify` — do not verify signatures
- `--keyring PATH` — GPG keyring file
- `--signature URL` — extra signature source; repeatable
- `--required-valid-signature-count N|+all|-1` — minimum sigs required
- `--ignore-signature-status-code CODE` — repeatable; suppress one code
- `--ignore-signature-status-codes "CODE CODE ..."` — space-separated list

`collection list`
- `--format FORMAT` (default human; supports `json`, `yaml`)
- `-p, --collections-path` — repeatable, colon-separated

`collection verify`
- `-r, --requirements-file`, `-p, --collections-path`, `-i, --ignore-errors`
- `--offline`, `--keyring`, `--signature`, `--required-valid-signature-count`, `--ignore-signature-status-code(s)`

`collection init`
- `--collection-skeleton PATH`, `--init-path PATH`, `-f, --force`, `-e, --extra-vars`

`collection build`
- `--output-path PATH`, `-f, --force`

`collection publish`
- `--import-timeout SECONDS`, `--no-wait`

`collection download`
- `-r, --requirements-file`, `-p, --download-path`, `-n, --no-deps`, `--pre`, `--no-cache`, `--clear-response-cache`

`role install`
- `-r, --role-file FILE`, `-p, --roles-path PATH` (repeatable)
- `-f, --force`, `--force-with-deps`, `-n, --no-deps`, `-i, --ignore-errors`
- `-g, --keep-scm-meta` — use `tar` rather than archived SCM checkout (preserves `.git`)

`role init`
- `--init-path PATH`, `--offline`, `--role-skeleton PATH`, `--type {container,apb,network}`, `-f, --force`, `-e, --extra-vars`

`role list / info / remove / setup`
- `-p, --roles-path` (repeatable)
- `info` adds `--offline`
- `setup` adds `--list`, `--remove ID`

`role search`
- `--author USER`, `--galaxy-tags TAGS`, `--platforms OS_LIST`

`role import`
- `--branch REF`, `--no-wait`, `--role-name NAME`, `--status`

### Environment variables
- `ANSIBLE_GALAXY_SERVER_LIST`, `ANSIBLE_GALAXY_SERVER`, `ANSIBLE_GALAXY_TOKEN_PATH`
- `ANSIBLE_COLLECTIONS_PATH`, `ANSIBLE_ROLES_PATH`
- `ANSIBLE_CONFIG`

### Exit codes
Not formally documented. Empirically: `0` success, `1` operation failed
(install/publish/verify), `2` argument or input error, `3` no such
collection/role on server.

### Common patterns
- `ansible-galaxy collection install mynamespace.mycollection`
- `ansible-galaxy collection install -r requirements.yml`
- `ansible-galaxy collection install -U mynamespace.mycollection`
- `ansible-galaxy collection install --offline ./mynamespace-mycollection-1.2.3.tar.gz`
- `ansible-galaxy collection build && ansible-galaxy collection publish *.tar.gz`
- `ansible-galaxy collection verify --keyring ~/.gnupg/pubring.kbx mynamespace.mycollection`
- `ansible-galaxy role install -r requirements.yml`
- `ansible-galaxy role search --author geerlingguy --platforms EL`

### Quirks / gotchas
- `collection` and `role` use **disjoint flag sets** for some seemingly-shared
  concepts. Roles use `-r, --role-file` while collections use `-r, --requirements-file`.
- Roles support `--type {container,apb,network}` for skeleton init, not collections.
- Signature verification flags exist only for collections, not roles.
- `--required-valid-signature-count` accepts integer, the literal string `all`,
  or the `+` prefix (`+all`) to require even when zero are configured.
- `-c` here means `--ignore-certs`, not `--connection` as in `ansible`/`ansible-playbook`.
- `role install -g/--keep-scm-meta` is roles-only; switches the install mechanism.
- `ansible-galaxy install <name>` (no `role`/`collection`) is a deprecated
  shortcut that defaults to roles for back-compat; the modern form is explicit.

---

## 4. `ansible-vault`

### Purpose
Encrypt and decrypt YAML/text files at rest using a symmetric (AES256-CTR)
vault format. Useful for storing secrets in a repo. Operates on files
(`encrypt`, `decrypt`, `view`, `edit`, `create`, `rekey`) or on individual
strings to be embedded inline in a YAML file (`encrypt_string`). Supports
multiple "vault identities" so different files can be encrypted with different
passwords.

### Synopsis
```
ansible-vault [-h] [--version] [-v]
              {create,decrypt,edit,view,encrypt,encrypt_string,rekey} ...
```

### Subcommands
- `create FILE` — create a new file, open `$EDITOR`, save encrypted
- `decrypt FILE [...]` — decrypt in-place (or to `--output`)
- `edit FILE` — round-trip decrypt → editor → encrypt
- `view FILE` — pipe decrypted contents through pager
- `encrypt FILE [...]` — encrypt existing file(s) in-place (or to `--output`)
- `encrypt_string [STRING]` — encrypt a value to inline `!vault` YAML literal
- `rekey FILE [...]` — change the vault password / identity

### Flags

Parent
- `-h, --help`, `--version`, `-v/--verbose`

Common to most subcommands
- `--vault-id VAULT_IDS` — repeatable identity selector (`label@source`)
- `-J, --ask-vault-password, --ask-vault-pass` — prompt
- `--vault-password-file, --vault-pass-file FILE` — read password from file

`create`, `edit`, `encrypt`, `encrypt_string`, `rekey`
- `--encrypt-vault-id ENCRYPT_VAULT_ID` — explicit identity used for encryption
  (required when multiple `--vault-id` are present)
- `create` adds: `--skip-tty-check` (allow editor without TTY)

`decrypt`, `encrypt`, `encrypt_string`
- `--output OUTPUT_FILE` — write to file or `-` for stdout

`encrypt_string`
- `-n, --name NAME` — variable name(s) to attach (repeatable)
- `-p, --prompt` — prompt for the secret interactively
- `--show-input` — echo the secret as you type (instead of silent prompt)
- `--stdin-name ENCRYPT_STRING_STDIN_NAME` — variable name when reading from stdin

`rekey`
- `--new-vault-id NEW_VAULT_ID` — destination identity
- `--new-vault-password-file NEW_VAULT_PASSWORD_FILE` — destination password file

### Environment variables
- `ANSIBLE_VAULT_PASSWORD_FILE` — default password file
- `ANSIBLE_VAULT_IDENTITY_LIST` — comma-separated identities
- `ANSIBLE_VAULT_ENCRYPT_IDENTITY` — default `--encrypt-vault-id`
- `ANSIBLE_CONFIG`

### Exit codes
Not formally documented. Empirically: `0` success, `1` decryption / wrong
password / corrupt file, `2` invalid CLI arguments, `5` file not found.

### Common patterns
- `ansible-vault create secrets.yml`
- `ansible-vault encrypt --vault-id prod@prompt secrets.yml`
- `ansible-vault decrypt --output - secrets.yml | jq`
- `ansible-vault encrypt_string --vault-id dev@~/.vp 'sup3r' --name 'db_password'`
- `ansible-vault rekey --new-vault-password-file ~/.vp.new secrets.yml`

### Quirks / gotchas
- Encrypted-file output is pure ASCII (header `$ANSIBLE_VAULT;1.1;AES256` or
  `1.2` with vault-id), safe in text/yaml.
- Inline `!vault | ...` strings produced by `encrypt_string` are **not** the
  same wire format as encrypted files; they cannot be `decrypt`-ed as files.
- `--vault-id` syntax is `label@source` where source is `prompt`, a file path,
  or the keyword `prompt_ask`.
- `view` invokes the system pager (`$PAGER`, fallback `less`); on no-TTY runs
  it falls back to plain stdout.
- `create` needs `$EDITOR`; with `--skip-tty-check` it can be scripted.
- `rekey` writes only the new ciphertext; the plaintext is never persisted.
- `encrypt_string` with `--prompt` plus `--show-input` is the only way to
  visibly type a secret; default behaviour is silent.

---

## 5. `ansible-inventory`

### Purpose
Read-only inspection tool for inventories. Resolves dynamic inventory
plugins, applies group/host vars, and emits the merged result as JSON
(default), YAML, or TOML, or as a textual `--graph` view. Useful for
debugging "what inventory does Ansible see?" before running playbooks.

### Synopsis
```
ansible-inventory [-h] [--version] [-v] [-i INVENTORY] [-l SUBSET]
                  [--flush-cache] [--vault-id VAULT_IDS]
                  [-J | --vault-password-file VAULT_PASSWORD_FILES]
                  [--playbook-dir BASEDIR] [-e EXTRA_VARS] [--list]
                  [--host HOST] [--graph] [-y] [--toml] [--vars]
                  [--export] [--output OUTPUT_FILE]
                  [group]
```

### Subcommands
None. Mode is selected by mutually-exclusive flags.

### Flags

Identification
- `-h, --help`, `--version`, `-v/--verbose`

Inventory
- `-i, --inventory, --inventory-file INVENTORY` — repeatable
- `-l, --limit SUBSET` — host pattern restriction
- `--flush-cache` — drop fact cache
- `--playbook-dir BASEDIR` — relative-path resolution root

Variables / vault
- `-e, --extra-vars EXTRA_VARS` — repeatable
- `--vault-id VAULT_IDS` — repeatable
- `-J, --ask-vault-password, --ask-vault-pass`
- `--vault-password-file, --vault-pass-file FILE`

Mode (mutually exclusive trio)
- `--list` — dump entire inventory (script-protocol JSON shape)
- `--host HOST` — dump variables for a single host
- `--graph` — render hierarchical text graph; uses positional `group` if given

Format / output
- `-y, --yaml` — emit YAML (with `--list`); ignored with `--graph`
- `--toml` — emit TOML (with `--list`); ignored with `--graph`
- `--vars` — include host vars in graph (only with `--graph`)
- `--export` — produce export-friendly form (drops some computed fields)
- `--output OUTPUT_FILE` — write to file rather than stdout (with `--list`)

Positional
- `group` — group name; consumed by `--graph`

### Environment variables
- `ANSIBLE_INVENTORY`, `ANSIBLE_INVENTORY_ENABLED` (which plugins are enabled),
  `ANSIBLE_INVENTORY_CACHE`, `ANSIBLE_CONFIG`

### Exit codes
Not formally documented. Empirically: `0` ok, `1` inventory parse failure,
`2` argument error.

### Common patterns
- `ansible-inventory --list`
- `ansible-inventory --host server01.example.com`
- `ansible-inventory --graph webservers --vars`
- `ansible-inventory -i ./aws_ec2.yml --list -y`
- `ansible-inventory --list --output inventory_snapshot.json`

### Quirks / gotchas
- `--list`, `--host`, and `--graph` are mutually exclusive but argparse will
  silently let last-one-wins; `--vars` is silently ignored without `--graph`.
- `--export` strips the convenience computed group memberships used at run
  time; the file is **not** a 1:1 rehydratable inventory in all edge cases.
- `--toml` requires the optional `tomli-w` extra in modern Ansible-core.
- Default JSON shape is the legacy "inventory script" protocol (a top-level
  `_meta.hostvars`), not a plain group-tree.

---

## 6. `ansible-doc`

### Purpose
Show documentation for any installed plugin (modules, lookups, filters,
become plugins, connection plugins, callbacks, inventory plugins, vars
plugins, strategy plugins, cache plugins, cliconf, httpapi, netconf, shell
plugins, tests, roles, and even reserved Ansible keywords). Lists installed
plugins, prints docstrings, generates skeleton snippets for playbooks.

### Synopsis
```
ansible-doc [-h] [--version] [-v] [-M MODULE_PATH]
            [--playbook-dir BASEDIR]
            [-t {become,cache,callback,cliconf,connection,httpapi,inventory,
                 lookup,netconf,shell,vars,module,strategy,test,filter,role,
                 keyword}]
            [-j] [-r ROLES_PATH]
            [-e ENTRY_POINT | -s | -F | -l | --metadata-dump]
            [--no-fail-on-errors]
            [plugin ...]
```

### Subcommands
None. Plugin name(s) are positional.

### Flags

Identification
- `-h, --help`, `--version`, `-v/--verbose`

Path / source
- `-M, --module-path MODULE_PATH` — repeatable, colon-separated; env `ANSIBLE_LIBRARY`
- `-r, --roles-path ROLES_PATH` — repeatable
- `--playbook-dir BASEDIR`

Selection
- `-t, --type TYPE` — plugin family; default `module`
- `-e, --entry-point ENTRY_POINT` — for role docs (multi-entrypoint roles)

Mode (last four mutually exclusive with each other and with `-e`)
- `-s, --snippet` — emit playbook snippet (only for `module`, `lookup`, `inventory`)
- `-F, --list_files` — list plugin name + source path; implies `--list`
- `-l, --list` — list plugins of selected type; optional positional filters by namespace/collection
- `--metadata-dump` — internal; dump JSON metadata for everything; ignores other options
- `-j, --json` — switch normal docs output to JSON

Internal
- `--no-fail-on-errors` — used with `--metadata-dump`; report errors in JSON instead of failing

### Environment variables
- `ANSIBLE_LIBRARY`, `ANSIBLE_DOC_FRAGMENT_PLUGINS`, `ANSIBLE_CONFIG`

### Exit codes
Not formally documented. Empirically: `0` doc rendered, `1` plugin not found
or unparseable, `2` bad CLI options. With `--no-fail-on-errors` plus
`--metadata-dump`, always `0`.

### Common patterns
- `ansible-doc -l` — list all modules
- `ansible-doc ansible.builtin.copy`
- `ansible-doc -t lookup file`
- `ansible-doc -s ansible.builtin.copy` — playbook snippet
- `ansible-doc -F` — name + source-file map
- `ansible-doc -t become -l`
- `ansible-doc -t keyword loop`

### Quirks / gotchas
- `-s/--snippet` is silently rejected outside `module`, `lookup`, `inventory`.
- `--metadata-dump` is documented as internal; output schema is unstable.
- `keyword` "plugin type" is a virtual category for playbook-language keywords
  (`when`, `loop`, `register`, ...) — it documents the language, not a plugin.
- `-r` / roles-path is independent of `-M`; role docs need their own path.
- Long-form `--list_files` uses an underscore (the only place in the entire
  Ansible CLI surface that does), per the help text.

---

## 7. `ansible-config`

### Purpose
Inspect, dump, validate, and scaffold Ansible configuration. Configuration in
Ansible is a layered merge of `ansible.cfg`, `ANSIBLE_*` env vars, and runtime
overrides; `ansible-config` exposes the resolved view and which layer set each
value. Also emits a starter `ansible.cfg` for any plugin type.

### Synopsis
```
ansible-config [-h] [--version] [-v] {list,dump,view,init,validate} ...
```

### Subcommands
- `list` — list every available config option (with metadata: env var, ini key, default, choices)
- `dump` — show currently effective settings, optionally filtered to ones changed from default
- `view` — print the active config file verbatim
- `init` — emit a starter `ansible.cfg`
- `validate` — validate a config file's contents

### Flags

Parent
- `-h, --help`, `--version`, `-v/--verbose`

Common to all subcommands
- `-c, --config CONFIG_FILE` — path to file; default is precedence-search (`./ansible.cfg`, `~/.ansible.cfg`, `/etc/ansible/ansible.cfg`)
- `-t, --type TYPE` — filter to a plugin type (e.g. `connection`, `become`, `inventory`, ...)
- `-f, --format FORMAT` — output format; subcommand-dependent (`ini`, `yaml`, `json`, `env`, `vars`, `display`)

`dump`
- `--only-changed, --changed-only` — show only non-default values

`init`
- `--disabled` — comment out every emitted entry (so it's a "what could I set?" template)

### Environment variables
- `ANSIBLE_CONFIG` — config file path

### Exit codes
Not formally documented. Empirically: `0` success, `1` validation failure,
`2` argument error, `5` config file not found.

### Common patterns
- `ansible-config list`
- `ansible-config dump --only-changed`
- `ansible-config view`
- `ansible-config init --disabled -t all > ansible.cfg.example`
- `ansible-config validate -c ./ansible.cfg`

### Quirks / gotchas
- `view` is dumb cat; it does not pretty-print or merge.
- `dump` always shows the *resolved* values (env > file > default), not what's
  literally in the file; combine with `--only-changed` to see your overrides.
- `-t all` for `init` is the documented way to get every plugin's config block.
- `init`'s `-t` accepts a plugin family; it does **not** accept a specific
  plugin name.
- Different subcommands accept different `--format` values; `list` and `dump`
  speak `yaml`/`json`/`ini`/`env`/`vars`/`display`, `init` only `ini`/`env`/`vars`.

---

## 8. `ansible-console`

### Purpose
Interactive REPL with tab-completion that lets you fire ad-hoc tasks against a
host pattern, switching pattern/become/forks/check/diff state mid-session.
Built on the same execution path as `ansible`. The session has a "current
group" the same way a shell has a cwd; you `cd` to narrow scope.

### Synopsis
```
ansible-console [-h] [--version] [-v] [-b]
                [--become-method BECOME_METHOD] [--become-user BECOME_USER]
                [-K | --become-password-file BECOME_PASSWORD_FILE]
                [-i INVENTORY] [--list-hosts] [-l SUBSET] [--flush-cache]
                [--private-key PRIVATE_KEY_FILE] [-u REMOTE_USER]
                [-c CONNECTION] [-T TIMEOUT]
                [--ssh-common-args SSH_COMMON_ARGS]
                [--sftp-extra-args SFTP_EXTRA_ARGS]
                [--scp-extra-args SCP_EXTRA_ARGS]
                [--ssh-extra-args SSH_EXTRA_ARGS]
                [-k | --connection-password-file CONNECTION_PASSWORD_FILE]
                [-C] [-D] [--vault-id VAULT_IDS]
                [-J | --vault-password-file VAULT_PASSWORD_FILES]
                [-f FORKS] [-M MODULE_PATH] [--playbook-dir BASEDIR]
                [-e EXTRA_VARS] [--task-timeout TASK_TIMEOUT] [--step]
                [pattern]
```

### Subcommands (REPL commands)
Inside the running console:
- `cd [pattern]` — change current host/group; supports `app*.dc*:!app01*`
- `list` — list current hosts
- `list groups` — list groups
- `forks [N]` — set parallelism
- `become [bool]` — toggle become
- `become_user [user]` — change become user
- `become_method [method]` — change become method
- `remote_user [user]` — change remote user
- `verbosity [N]` — set verbosity 0–6
- `check [bool]` — toggle check mode
- `diff [bool]` — toggle diff mode
- `timeout [N]` — set per-task timeout (`0` disables)
- `help [command|module]` — show docs
- `!` — prefix to force the `shell` module rather than `command`
- `exit` — exit

### Flags
All flags from `ansible` apply. Removed: `-m`, `-a`, `-B`, `-P`, `-o`,
`-t TREE`. Pattern is positional but **optional** here (defaults to `all`).

Novel:
- `--step` — confirm each task before it runs

### Environment variables
- Same as `ansible` (`ANSIBLE_INVENTORY`, `ANSIBLE_LIBRARY`, `ANSIBLE_CONFIG`).

### Exit codes
Not formally documented. `0` clean exit (`exit` command, EOF), non-zero on
startup failure (bad inventory, bad options).

### Common patterns
- `ansible-console -i inventory all`
- `ansible-console -i hosts -b -K web_servers`
- `ansible-console -i hosts -f 20 webservers`
- `ansible-console -i inventory -C production_hosts` (check mode session)

### Quirks / gotchas
- The pattern is positional but **optional** (unlike `ansible`); default `all`.
- The REPL's `!` prefix is the only way to use shell features without typing
  `shell <cmd>`.
- `cd` accepts the full pattern grammar including intersection (`:&`),
  difference (`:!`), and globs.
- `check` / `diff` / `become` etc. accept either an explicit boolean or no
  argument (which toggles).
- No history persistence by default; `readline` history per-session only.
- If Python `readline` module is missing, tab completion silently degrades.

---

## 9. `ansible-pull`

### Purpose
Inverts the push model: a managed node clones a playbook repository (git, hg,
svn, bzr) and runs it locally. Designed for cron-driven pull-based deployment
across very large fleets where a central controller would not scale. Default
playbook is the FQDN, then short hostname, then `local.yml`. Connection is
typically `local`. Has VCS-management flags on top of all the regular
playbook flags.

### Synopsis
```
ansible-pull [-h] [--version] [-v] [--private-key PRIVATE_KEY_FILE]
             [-u REMOTE_USER] [-c CONNECTION] [-T TIMEOUT]
             [--ssh-common-args SSH_COMMON_ARGS]
             [--sftp-extra-args SFTP_EXTRA_ARGS]
             [--scp-extra-args SCP_EXTRA_ARGS]
             [--ssh-extra-args SSH_EXTRA_ARGS]
             [-k | --connection-password-file CONNECTION_PASSWORD_FILE]
             [--vault-id VAULT_IDS]
             [-J | --vault-password-file VAULT_PASSWORD_FILES]
             [-e EXTRA_VARS] [-t TAGS] [--skip-tags SKIP_TAGS]
             [-i INVENTORY] [--list-hosts] [-l SUBSET] [--flush-cache]
             [-M MODULE_PATH]
             [-K | --become-password-file BECOME_PASSWORD_FILE]
             [--purge] [-o] [-s SLEEP] [-f] [-d DEST] [-U URL] [--full]
             [-C CHECKOUT] [--accept-host-key] [-m MODULE_NAME]
             [--verify-commit] [--clean] [--track-subs] [--check]
             [--diff]
             [playbook.yml ...]
```

### Subcommands
None. Optional playbook positionals override the FQDN/hostname/local.yml search.

### Flags

Most `ansible-playbook` flags apply (connection, vault, tags, extra-vars,
inventory). Reused: `-i`, `-l`, `--list-hosts`, `--flush-cache`, `-c`, `-u`,
`-T`, `-k`, `--connection-password-file`, `--private-key`, `-K`,
`--become-password-file`, `-e`, `--vault-id`, `-J`, `--vault-password-file`,
`-t/--tags`, `--skip-tags`, `-M`, `--check`, `--diff`, `--ssh-common-args`,
`--ssh-extra-args`, `--scp-extra-args`, `--sftp-extra-args`, `-v`, `--version`, `-h`.

Novel — VCS controls:
- `-U, --url URL` — repository URL
- `-d, --directory DEST` — local checkout path
- `-C, --checkout CHECKOUT` — branch/tag/commit
- `-m, --module-name MODULE_NAME` — VCS module: `git` (default), `subversion`, `hg`, `bzr`
- `--accept-host-key` — auto-accept SSH host key for the repo
- `--full` — full clone (default is shallow)
- `--clean` — discard local repo modifications
- `--track-subs` — submodules track latest (`--remote`)
- `--verify-commit` — GPG-verify the checked-out commit; abort if invalid

Novel — execution:
- `-s, --sleep SLEEP` — random sleep `0..N` seconds before run (jitter for cron)
- `-f, --force` — run even if VCS update failed
- `-o, --only-if-changed` — skip run if repo HEAD did not move
- `--purge` — delete the checkout after the run

### Environment variables
- `ANSIBLE_INVENTORY`, `ANSIBLE_LIBRARY`, `ANSIBLE_CONFIG`
- Plus everything `ansible-playbook` reads.

### Exit codes
Not formally documented. Same shape as `ansible-playbook`. Plus `1` for VCS
clone/update failure (unless `-f`), and `0` early-exit when `-o` decides
nothing changed.

### Common patterns
- `ansible-pull -U https://github.com/me/cfg.git`
- `ansible-pull -U git@github.com:me/cfg.git -C production -d /opt/ansible-pull`
- Cron line: `*/15 * * * * ansible-pull -U https://example/cfg.git -o -s 60 site.yml`
- `ansible-pull -U https://example/cfg.git --verify-commit --clean`

### Quirks / gotchas
- `-C` here is `--checkout` (a VCS ref), **not** `--check` as in
  `ansible-playbook`. `--check` exists as long form only.
- `-m` here is `--module-name` for the VCS module, not for an Ansible task module.
- Default playbook search order: `<FQDN>.yml` → `<hostname>.yml` → `local.yml`.
- `-s` adds randomized sleep, intended for thundering-herd avoidance under cron.
- `--purge` deletes the *clone*, not facts or logs.
- Connection defaults to `local`; the `-c`/`-u`/`-k`/SSH flags are present for
  the sub-spawned playbook, not for talking to the repo.
- `--accept-host-key` writes to the user's `known_hosts` — it is
  trust-on-first-use for whatever runs `ansible-pull`.

---

## 10. `ansible-test`

### Purpose
The collection/core developer test harness. Runs sanity tests (linters and
static analysis), unit tests, and integration tests for Ansible content
inside isolated environments (Docker, Podman, cloud "remote" VMs, or local
venvs). Not a user-facing tool; lives in the `ansible-core` package and is
intended to be invoked from inside a checked-out collection or `ansible/`
source tree. Drives the upstream CI matrix.

### Synopsis
```
ansible-test <command> [options] [test_targets ...]
```

### Subcommands
- `sanity` — linters and static analysis (the bulk of test types live here)
- `units` — pytest-based unit tests
- `integration` — POSIX integration tests
- `network-integration` — integration tests against network platforms
- `windows-integration` — integration tests against Windows targets
- `shell` — drop into an interactive shell inside a test environment
- `coverage` — manage coverage data
  - `coverage erase` — wipe collected data
  - `coverage report` — text report
  - `coverage html` — HTML report (`test/results/reports/coverage/`)
  - `coverage xml` — XML report
  - `coverage combine` — merge per-process data
  - `coverage analyze` — coverage queries (targets, missing, etc.)
- `env` — print environment metadata used by other commands

### Flags

Cross-cutting (most subcommands)
- `-h, --help`
- `-v, --verbose` (repeatable)
- `--color`, `--no-color`
- `--truncate WIDTH`, `--redact` / `--no-redact`
- `--metadata FILE` — load environment metadata
- `--time-limit MINUTES` — fail if total time exceeds limit
- `--terminate {success,never,always}` — when to terminate the environment

Environment selection
- `--docker [IMAGE]` — run inside a container image; default `default`
- `--podman` — explicit podman backend
- `--remote PLATFORM` — run in cloud-hosted ephemeral VM (requires API key)
- `--venv` — run in a venv ansible-test creates
- `--venv-system-site-packages` — venv with system site-packages
- `--python VERSION` — Python version (e.g. `3.13`)
- `--controller {docker,remote,venv,...}` — controller env (composite arg)
- `--target {docker,remote,venv,...}` — target env (composite arg)

Requirements / setup
- `--requirements` — install per-test requirements automatically
- `--requirements-mode {only,skip}`
- `--no-pip-check`
- `--keep-git` — preserve `.git` inside the workdir
- `--no-temp-workdir`, `--no-temp-unicode`

Selection
- `--changed` — only test what changed vs. base branch
- `--base-branch BRANCH` — base for `--changed`
- `--include PATTERN`, `--exclude PATTERN`
- `--allow-disabled`, `--allow-unstable`, `--allow-unsupported`
- `--list-targets`
- `--retry-on-error`, `--continue-on-error`

`sanity` (most numerous specific flags)
- `--test TEST` — repeatable; pick a single sanity test
- `--skip-test TEST` — repeatable
- `--list-tests` — print all sanity test names; with `--allow-disabled` includes disabled
- `--allow-disabled` — let disabled tests run
- `--enable-test TEST` — enable a normally-skipped test
- `--lint` — emit lint-style output

Sanity test families (used as `--test` values):
`action-plugin-docs`, `ansible-doc`, `changelog`, `compile`, `empty-init`,
`ignores`, `import`, `line-endings`, `no-assert`, `no-basestring`,
`no-dict-iteritems`, `no-dict-iterkeys`, `no-dict-itervalues`,
`no-get-exception`, `no-illegal-filenames`, `no-main-display`,
`no-smart-quotes`, `no-unicode-literals`, `pep8`, `pslint`, `pylint`,
`replace-urlopen`, `runtime-metadata`, `shebang`, `shellcheck`, `symlinks`,
`use-argspec-type-path`, `use-compat-six`, `validate-modules`, `yamllint`,
plus core-only: `ansible-requirements`, `bin-symlinks`, `boilerplate`,
`integration-aliases`, `mypy`, `no-unwanted-files`, `obsolete-files`,
`package-data`, `pymarkdown`, `release-names`,
`required-and-default-attributes`, `test-constraints`.

`units`
- `--coverage` — collect coverage
- `--num-workers N` — pytest-xdist worker count
- Test target = module name (e.g. `apt`) or path (`test/units/...`)

`integration`, `network-integration`, `windows-integration`
- `--coverage`
- `--docker-privileged` — run docker privileged (mac hosts mostly)
- `--retry-on-error`, `--continue-on-error`
- `--changed-all-target`, `--changed-all-mode {default,include,exclude}`
- `--list-targets`
- Test target = role/test-target name (e.g. `ping`, `lineinfile`)

`shell`
- `--raw` — bypass setup
- Plus all environment selectors above

`coverage erase|report|html|xml|combine|analyze`
- `--all` — operate on all collected data
- `--stub`, `--export`, `--show-missing`

`env`
- `--show` — print env metadata to stdout
- `--dump` — write to `test/results/.tmp/env.json`
- `--list-files`, `--timeout N`

### Environment variables
- `ANSIBLE_TEST_PREFER_PODMAN` — non-empty to prefer podman over docker
- `ANSIBLE_KEEP_REMOTE_FILES=1` — preserve remote files (venv only)
- `ANSIBLE_TEST_CONTAINER_REGISTRY`, `ANSIBLE_TEST_REMOTE_*`
- `PYTHONDONTWRITEBYTECODE`, `PYTHONUNBUFFERED`

### Exit codes
Not formally documented. `0` on pass, non-zero on failure or environment
setup error. `2` typically argparse error. Sanity reports per-test failures
through aggregated rc; `--continue-on-error` softens this.

### Common patterns
- `ansible-test sanity`
- `ansible-test sanity --test pep8 --python 3.13`
- `ansible-test sanity --list-tests --allow-disabled`
- `ansible-test units --docker default --python 3.13`
- `ansible-test units --coverage apt`
- `ansible-test integration --docker ubuntu2204 ping`
- `ansible-test coverage erase && ansible-test units --coverage && ansible-test coverage html`
- `ansible-test shell --venv --python 3.13`

### Quirks / gotchas
- Must be invoked from a specific layout: collection root (with `galaxy.yml`)
  or a checkout of `ansible/ansible`. Will refuse to run elsewhere.
- Environment is **isolated by default** — env vars from your shell do NOT
  propagate into `--docker` or `--remote` runs unless explicitly forwarded.
- `--docker` with no arg uses `default` (a special tag, not literal `default`).
- `--controller` and `--target` are composite (subcommand-level): the same
  flag can carry plugin-name + colon-options.
- Coverage output goes to `test/results/...` relative to the working tree;
  caller can't redirect easily.
- `--changed` requires a clean git tree and a `--base-branch`; otherwise the
  diff set is undefined.
- Sanity tests are mostly exclude-listed via per-test `ignore.txt` files
  inside the collection / module path; `ansible-test ignores` reports those.

---

## 11. `ansible-connection`

### Purpose
Internal persistent-connection daemon. Not user-facing. Spawned automatically
by `ansible` and `ansible-playbook` when a connection plugin needs persistent
state across multiple tasks (most commonly the `network_cli`, `netconf`, and
`httpapi` network connection plugins). Maintains a Unix-domain control socket
under `~/.ansible/pc/` keyed by host/user, accepts JSON-RPC over the socket,
and forwards `exec_command`-style requests through the underlying transport.
Per the source, it takes pickled play context plus options on stdin and emits
JSON results on stdout/stderr. Process is named in logs by PID (`p=<pid>`).

### Synopsis
```
ansible-connection <playbook_pid> <task_uuid>
```

### Subcommands
None.

### Flags
The script reuses Ansible's "base parser" so it accepts the standard
`-h/--help`, `--version`, and `-v/--verbose` flags (verbosity passed through
to logging). It does **not** accept inventory, become, or vault flags;
everything substantive arrives via the pickled stdin payload from the parent.

### Stdin / IPC
- stdin: pickled `(play_context, init_options)` from the parent process
- socket: Unix-domain socket under `~/.ansible/pc/<hash>` (path derives from
  the connection plugin's control-path logic)
- protocol: JSON-RPC requests (`exec_command`, `put_file`, `fetch_file`,
  `update_play_context`, `reset`, `close`)

### Environment variables
- `ANSIBLE_PERSISTENT_CONTROL_PATH_DIR` — override socket directory (default `~/.ansible/pc`)
- `ANSIBLE_PERSISTENT_CONNECT_TIMEOUT` — initial-connect timeout
- `ANSIBLE_PERSISTENT_COMMAND_TIMEOUT` — per-RPC timeout
- `ANSIBLE_CONFIG`

### Exit codes
- `0` — success, JSON result on stdout
- `1` — failure, JSON error on stderr
Process also installs SIGALRM handlers for both idle-timeout and
command-timeout, exiting non-zero when fired.

### Common patterns
Not invoked by users. The integration looks like:
```
# from a playbook with connection: network_cli
# Ansible spawns:
#   ansible-connection 12345 a1b2c3d4-... < pickled_payload
```

### Quirks / gotchas
- Despite living on `$PATH`, it is effectively private API; argv shape can
  change between minor versions.
- It is the daemon that owns the persistent SSH session — killing it forces
  the next task to renegotiate.
- All errors surface to playbooks as the catch-all "unable to open shell"
  unless `ANSIBLE_LOG_PATH` is set so the daemon's log has detail.
- The runsible counterpart should treat this as an **internal binary** —
  exposed as a crate, but not as a stable CLI surface.

---

## 12. `ansible-lint`

### Purpose
Static analyser for Ansible content, maintained as a **separate project**
(github.com/ansible/ansible-lint, distributed via PyPI rather than
ansible-core). Runs a configurable rule set against playbooks, roles, and
collections. Supports profiles (a graduated severity ladder from `min` to
`production`), per-rule warn/skip lists, custom rule directories, multiple
output formats (including SARIF for CI integration), an autofix mode, and an
ignore file for grandfathered violations.

### Synopsis
```
ansible-lint [-h] [-P | -L | -T]
             [-f {brief,full,md,json,codeclimate,quiet,pep8,sarif}]
             [--sarif-file SARIF_FILE] [-q]
             [--profile {min,basic,moderate,safety,shared,production}]
             [--project-dir PROJECT_DIR] [-r RULESDIR] [-R] [-s]
             [--fix [WRITE_LIST]] [--show-relpath] [-t TAGS] [-v]
             [-x SKIP_LIST] [--generate-ignore] [-w WARN_LIST]
             [--enable-list ENABLE_LIST] [--nocolor] [--force-color]
             [--exclude EXCLUDE_PATHS [EXCLUDE_PATHS ...]]
             [-c CONFIG_FILE] [-i IGNORE_FILE]
             [--yamllint-file YAMLLINT_FILE] [--offline | --no-offline]
             [--version]
             [lintables ...]
```

### Subcommands
None. Lintables are positional; absence of positionals triggers auto-detect
mode (walks the project root).

### Flags

Identification
- `-h, --help`, `--version`

Listing modes (mutually exclusive trio)
- `-P, --list-profiles`
- `-L, --list-rules`
- `-T, --list-tags`

Output
- `-f, --format {brief,full,md,json,codeclimate,quiet,pep8,sarif}` — `json` is alias of `codeclimate`
- `--sarif-file SARIF_FILE` — write SARIF to file
- `-q` — quieter (repeatable up to `-qq`)
- `-v` — louder (repeatable up to `-vv`)
- `--show-relpath` — paths relative to cwd
- `--nocolor` — disable color (env equivalent `NO_COLOR=1`)
- `--force-color` — force color (env equivalent `FORCE_COLOR=1`)

Rule selection
- `--profile {min,basic,moderate,safety,shared,production}`
- `-r, --rules-dir RULESDIR` — repeatable, custom rules
- `-R` — keep default rules in addition to `-r`
- `-t, --tags TAGS` — only rules matching tags (space/comma separated)
- `-x, --skip-list SKIP_LIST` — skip rules by id or tag
- `-w, --warn-list WARN_LIST` — treat as warning, not error; default `experimental, jinja[spacing], fqcn[deep]`
- `--enable-list ENABLE_LIST` — opt in to optional rules

Behaviour
- `-s, --strict` — non-zero exit even for warnings
- `--fix [WRITE_LIST]` — auto-fix mode; arg is `all` / `none` / comma list of rule ids/tags
- `--exclude EXCLUDE_PATHS [...]` — skip paths; repeatable
- `--offline / --no-offline` — skip / force requirements install + schema refresh
- `--generate-ignore` — write `.ansible-lint-ignore` of all current violations

Paths / config
- `--project-dir PROJECT_DIR` — project root override
- `-c, --config-file CONFIG_FILE` — explicit config; default search:
  `.ansible-lint`, `.ansible-lint.yml`, `.ansible-lint.yaml`,
  `.config/ansible-lint.yml`, `.config/ansible-lint.yaml`
- `-i, --ignore-file IGNORE_FILE` — explicit ignore file; default search:
  `.ansible-lint-ignore`, `.config/ansible-lint-ignore.txt`
- `--yamllint-file YAMLLINT_FILE` — yamllint config; default search:
  `.yamllint(.yaml|.yml)`, `~/.config/yamllint/config`, plus
  `XDG_CONFIG_HOME` and `YAMLLINT_CONFIG_FILE` env vars

### Environment variables
- `ANSIBLE_LINT_CUSTOM_RULESDIR` — extra rules dir
- `ANSIBLE_LINT_IGNORE_FILE` — override default ignore filename
- `ANSIBLE_LINT_WRITE_TMP` — write fixes to temp files (test only)
- `ANSIBLE_LINT_SKIP_SCHEMA_UPDATE` — skip schema refresh
- `ANSIBLE_LINT_NODEPS` — skip dep install + dep-requiring checks
- `NO_COLOR`, `FORCE_COLOR`, `XDG_CONFIG_HOME`, `YAMLLINT_CONFIG_FILE`

### Exit codes
- `0` — clean (no violations; or only warnings without `--strict`)
- non-zero — violations present (or warnings, with `--strict`)
- Specific rcs in the source: `2` runtime/internal error, `3` uncaught
  exception, `4` config error, `5` invalid options.

### Output streams
- stdout: violations in chosen format
- stderr: stats, info, log

### Configuration file format
YAML, mirroring long-option names:
```yaml
profile: production
exclude_paths:
  - .cache/
  - tests/output/
skip_list:
  - yaml[line-length]
warn_list:
  - experimental
enable_list:
  - no-same-owner
use_default_rules: true
verbosity: 1
offline: true
```
Configuration files searched in the order listed under `-c`.

### Common patterns
- `ansible-lint`
- `ansible-lint playbook.yml`
- `ansible-lint --profile production`
- `ansible-lint --list-rules`
- `ansible-lint -t idempotency playbook.yml`
- `ansible-lint -x formatting,metadata playbook.yml`
- `ansible-lint --fix playbook.yml`
- `ansible-lint --fix=fqcn,yaml playbook.yml`
- `ansible-lint --format sarif --sarif-file report.sarif`
- `ansible-lint --generate-ignore` (then commit the file)

### Quirks / gotchas
- Separate project; release cadence is decoupled from `ansible-core`. The
  rule set evolves and `--profile` is the correct way to pin behaviour.
- `-f json` is **not** an arbitrary JSON shape — it is alias for
  `codeclimate` (which is itself a JSON dialect).
- `--fix` with no arg defaults to `all`; `--fix=none` disables every transform.
- `-q` and `-v` are independently repeatable; combining them is undefined.
- `--strict` is what most CI gates want; without it, warnings are silently
  ignored in the exit code.
- `--offline` does **not** skip rule loading; it skips the
  `requirements.yml` install pass and the JSON-schema refresh.
- The default config search begins at the **project dir**, which itself is
  auto-detected via config file location, then git root, then `$HOME`.

---

## Cross-cutting summary

Flags shared by every host-touching binary (`ansible`, `ansible-playbook`,
`ansible-console`, `ansible-pull`, `ansible-inventory`):
- inventory: `-i`, `-l`, `--list-hosts`, `--flush-cache`
- vault: `--vault-id`, `-J`, `--vault-password-file`
- vars: `-e/--extra-vars`
- verbosity: `-v` (six levels)

Flags shared by the executor binaries (`ansible`, `ansible-playbook`,
`ansible-console`, `ansible-pull`):
- connection: `-c`, `-u`, `-T`, `-k`, `--connection-password-file`,
  `--private-key`, `--ssh-common-args`, `--ssh-extra-args`,
  `--scp-extra-args`, `--sftp-extra-args`
- become: `-b`, `-K`, `--become-method`, `--become-user`,
  `--become-password-file`
- modes: `-C/--check`, `-D/--diff`
- modules: `-M/--module-path`
- forks: `-f` (default 5)

Mutual-exclusion pairs that recur:
- `-K` ⊕ `--become-password-file`
- `-k` ⊕ `--connection-password-file`
- `-J` ⊕ `--vault-password-file`

Shared `ANSIBLE_*` env vars:
- `ANSIBLE_CONFIG`, `ANSIBLE_INVENTORY`, `ANSIBLE_LIBRARY`,
  `ANSIBLE_ROLES_PATH`, `ANSIBLE_COLLECTIONS_PATH`,
  `ANSIBLE_VAULT_PASSWORD_FILE`, `ANSIBLE_FORCE_HANDLERS`

Common config-file precedence:
1. `ANSIBLE_CONFIG` env var
2. `./ansible.cfg`
3. `~/.ansible.cfg`
4. `/etc/ansible/ansible.cfg`

Default inventory:
- `/etc/ansible/hosts`

Recurring quirks for runsible to design around:
- The ad-hoc binary takes a **pattern** positionally; the playbook binary
  takes **playbooks** positionally; the inventory binary takes a **group**
  positionally (and only when `--graph` is used). No two binaries agree on
  what the leading positional means.
- Single-letter flags collide across binaries:
  - `-t` is `--tree` in `ansible`, `--tags` in `ansible-playbook` and
    `ansible-pull`, `--type` in `ansible-doc` and `ansible-config`,
    `--list-tags` in `ansible-lint`.
  - `-C` is `--check` in `ansible`, `ansible-playbook`, `ansible-console`,
    but `--checkout` in `ansible-pull`.
  - `-c` is `--connection` in executors, `--ignore-certs` in
    `ansible-galaxy`, `--config` in `ansible-config` and `ansible-lint`.
  - `-m` is `--module-name` in `ansible`/`ansible-pull`, but in
    `ansible-pull` it's the **VCS** module, not the task module.
  - `-r` is `--requirements-file` in galaxy, `--roles-path` in
    `ansible-doc`, `--rules-dir` in `ansible-lint`.
  - `-s` is `--snippet` in `ansible-doc`, `--server` in `ansible-galaxy`,
    `--sleep` in `ansible-pull`, `--strict` in `ansible-lint`.
  - `-f` is `--forks` in executors, `--force` in `ansible-pull`,
    `--format` in `ansible-config`/`ansible-lint`.
- The collision matrix above is the strongest argument for runsible to
  treat short flags as **per-binary**, not via a shared global table.
- Several "pseudo-CLIs" exist: the `ansible-console` REPL command set, the
  `ansible-vault` editor sub-flow, and the JSON-RPC protocol of
  `ansible-connection`. These deserve their own runsible sub-modules.
