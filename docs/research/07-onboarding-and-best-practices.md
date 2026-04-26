# 07 — Onboarding & Best Practices

> Research distilled from the Ansible community documentation:
> - `https://docs.ansible.com/ansible/latest/getting_started/index.html` (and sub-pages)
> - `https://docs.ansible.com/ansible/latest/installation_guide/index.html` (and sub-pages)
> - `https://docs.ansible.com/ansible/latest/tips_tricks/index.html` (and sub-pages)
>
> Goal: capture the canonical first-run UX, recommended layouts, and the
> documented anti-patterns that runsible should either eliminate or imitate.

---

## 1. Conceptual model: the three nodes

Ansible's mental model is intentionally lean. The introductory diagram and
glossary describe **three** concrete actors:

| Role            | What it is                                                                                          |
| --------------- | --------------------------------------------------------------------------------------------------- |
| **Control node**| The machine you run `ansible`, `ansible-playbook`, `ansible-vault`, etc. on. Linux/macOS/BSD/WSL.   |
| **Inventory**   | A list of managed nodes, organized into groups, with optional per-host/per-group vars.              |
| **Managed node**| A target machine (server, network device, container) that Ansible reaches over SSH/WinRM/local.     |

The four design pillars the docs cite explicitly:

1. **Agentless** — no daemon on managed nodes; uses existing OS credentials over
   SSH (Linux/BSD/macOS) or WinRM/PSRemoting (Windows). Network devices use
   their native CLI/NETCONF/HTTP transports.
2. **Simplicity** — "straightforward YAML syntax for code that reads like
   documentation."
3. **Scalability and flexibility** — same declarative content targets servers,
   clouds, switches, routers, and z/OS hosts.
4. **Idempotence and predictability** — "When the system is in the state your
   playbook describes, Ansible does not change anything, even if the playbook
   runs multiple times." This is the property runsible must inherit module-by-
   module.

### Glossary the docs lean on

- **Module** — the unit of work; a small program (Python, PowerShell, or binary)
  copied to the managed node and executed.
- **Task** — invocation of a module with parameters.
- **Play** — ordered list of tasks mapped to a host pattern.
- **Playbook** — ordered list of plays in a YAML file.
- **Role** — reusable bundle of tasks/handlers/vars/templates/files/meta with a
  fixed directory layout.
- **Handler** — a special task that only runs when notified by a `changed`
  result (typically used for service restarts).
- **Plugin** — connection plugins, lookup plugins, filter plugins, callback
  plugins, strategy plugins, etc. — orthogonal to modules.
- **Collection** — distributable bundle of modules + roles + plugins +
  playbooks. `ansible.builtin` is the implicitly-loaded core collection;
  everything else is third-party (community.general, ansible.posix, cloud
  vendor collections, etc.).
- **FQCN** — Fully Qualified Collection Name, e.g. `ansible.builtin.copy`.

---

## 2. Installation paths

The Installation Guide treats the controller as the only thing that needs
"Ansible" installed; managed nodes only need a Python interpreter (and even
that's optional for the `raw` module or for bootstrapping).

### 2.1 Two distributions

| Package         | What you get                                                                                       |
| --------------- | -------------------------------------------------------------------------------------------------- |
| **`ansible-core`** | Engine + the `ansible.builtin` namespace only. Smallest, fastest, what RHEL ships.              |
| **`ansible`**      | "Batteries included" metapackage = `ansible-core` + a community-curated set of collections.     |

The doc explicitly tells new users: install `ansible` for general purpose work,
install `ansible-core` if you want a minimal footprint and will pull in
collections from Galaxy on demand.

### 2.2 Installation methods, in order of how the docs prefer them

#### 2.2.1 `pipx` (recommended for individual users)

```
$ pipx install --include-deps ansible
$ pipx install ansible-core
$ pipx install ansible-core==2.12.3
$ pipx upgrade --include-injected ansible
$ pipx inject ansible argcomplete
```

`--include-deps` is the magic flag — without it pipx puts `ansible` into a
single venv but doesn't expose `ansible-playbook`/`ansible-galaxy` etc. on
`$PATH`. `pipx inject` is how you add collection dependencies (e.g. boto3 for
the AWS collection) into the same venv.

#### 2.2.2 `pip` (legacy, still widely used)

```
$ python3 -m pip -V
$ python3 -m pip install --user ansible
$ python3 -m pip install --user ansible-core
$ python3 -m pip install --user ansible-core==2.12.3
$ python3 -m pip install --upgrade --user ansible
```

`--user` is required outside a venv on PEP 668 ("externally managed") systems
like Debian 12+ and Ubuntu 24.04+; otherwise `pip install` will refuse.

#### 2.2.3 OS package managers

- **Fedora**: `sudo dnf install ansible` (full) or `sudo dnf install ansible-core` (minimal)
- **EPEL** (RHEL/Rocky/Alma/CentOS Stream): enable EPEL, then `dnf install ansible`
- **OpenSUSE**: `sudo zypper install ansible`
- **Ubuntu**: PPA route —
  ```
  sudo apt update
  sudo apt install software-properties-common
  sudo add-apt-repository --yes --update ppa:ansible/ansible
  sudo apt install ansible
  ```
- **Debian**: either Debian's stable repo or the Ubuntu PPA (substituting the
  matching `UBUNTU_CODENAME`):
  ```
  UBUNTU_CODENAME=jammy
  wget -O- "https://keyserver.ubuntu.com/pks/lookup?fingerprint=on&op=get&search=0x6125E2A8C77F2818FB7BD15B93C4A3FD7BB9C367" \
    | sudo gpg --dearmor -o /usr/share/keyrings/ansible-archive-keyring.gpg
  echo "deb [signed-by=/usr/share/keyrings/ansible-archive-keyring.gpg] http://ppa.launchpad.net/ansible/ansible/ubuntu $UBUNTU_CODENAME main" \
    | sudo tee /etc/apt/sources.list.d/ansible.list
  sudo apt update && sudo apt install ansible
  ```
- **Arch**: `sudo pacman -S ansible` or `ansible-core`
- **Windows**: not supported as a controller. WSL is the only path.

The OS-package version often lags 2-3 minor releases behind the upstream
release Ansible ships, which is why pipx is recommended for power users.

#### 2.2.4 From source

For development and bleeding-edge work:

```
$ python3 -m pip install --user https://github.com/ansible/ansible/archive/devel.tar.gz
```

or a clone-and-source-env-setup workflow:

```
$ git clone https://github.com/ansible/ansible.git
$ cd ./ansible
$ source ./hacking/env-setup
$ python3 -m pip install --user -r ./requirements.txt
$ git pull --rebase
```

`hacking/env-setup` mutates `$PATH`, `$PYTHONPATH`, `$MANPATH`, and
`$ANSIBLE_LIBRARY` so the in-tree code wins over anything installed.

### 2.3 Confirming the install

```
$ ansible --version
$ ansible-community --version   # only present in the "ansible" metapackage
```

`ansible --version` prints engine version, config file path, configured module
search path, ansible python module location, executable location, and the
Python interpreter version.

### 2.4 Shell completion

argcomplete-based; the docs suggest:

```
$ pipx inject --include-apps ansible argcomplete   # (pipx)
$ python3 -m pip install --user argcomplete        # (pip)
$ activate-global-python-argcomplete --user        # global
$ eval $(register-python-argcomplete ansible)      # per-command
$ eval $(register-python-argcomplete ansible-playbook)
$ eval $(register-python-argcomplete ansible-vault)
```

### 2.5 Control-node and managed-node requirements

- **Controller**: any UNIX-like host with a supported Python (varies by ansible-
  core release; consult the support matrix). Windows is not supported as a
  controller; use WSL.
- **Managed node**: a usable POSIX user account reachable over SSH with an
  interactive shell, plus Python somewhere on the box. Network modules and
  `raw`/`script` lift the Python requirement.

### 2.6 Configuring Ansible (`ansible.cfg`)

Three layers, in increasing precedence:

1. `ansible.cfg` — search order: `$ANSIBLE_CONFIG`, `./ansible.cfg`,
   `~/.ansible.cfg`, `/etc/ansible/ansible.cfg`. Generate a stub with:
   ```
   $ ansible-config init --disabled > ansible.cfg
   $ ansible-config init --disabled -t all > ansible.cfg   # include plugin sections
   ```
2. Environment variables (`ANSIBLE_*`) — override the file.
3. Command line flags — override everything.

The docs are blunt: "the stock configuration should be sufficient for most
users." The strong recommendation is to keep `ansible.cfg` minimal and
project-local, beside the playbooks.

---

## 3. First run: `ansible all -m ping`, end-to-end

The Getting Started guide walks new users through this exact ritual. Here's
the unpacked sequence of what actually happens:

### 3.1 Setup

```
$ pip install ansible        # or pipx, dnf, apt, etc.
$ mkdir ansible_quickstart && cd ansible_quickstart
```

The doc explicitly notes: "Using a single directory structure makes it easier
to add to source control as well as to reuse and share automation content."

### 3.2 Build an inventory

`inventory.ini` (the simplest possible form):

```
[myhosts]
192.0.2.50
192.0.2.51
192.0.2.52
```

Or YAML, which the doc recommends as the host count grows:

```yaml
myhosts:
  hosts:
    my_host_01:
      ansible_host: 192.0.2.50
    my_host_02:
      ansible_host: 192.0.2.51
    my_host_03:
      ansible_host: 192.0.2.52
```

Verify:

```
$ ansible-inventory -i inventory.ini --list
$ ansible-inventory -i inventory.ini --graph
```

### 3.3 Run ping

```
$ ansible myhosts -m ping -i inventory.ini
```

What this actually does, end-to-end (the docs imply this; runsible
implementers need it spelled out):

1. **Parse inventory.** The CLI loads `inventory.ini`, expands the `myhosts`
   group into a concrete host list, and for each host computes the merged var
   set (`all` group vars → child group vars → host_vars → CLI `-e`).
2. **Resolve connection per host.** For each host, look up `ansible_connection`
   (default `ssh`), `ansible_user` (default = current user), `ansible_port`
   (default 22), `ansible_python_interpreter` (default = auto-detect), etc.
3. **Open a transport.** Ansible spawns an `ssh` process to each host using the
   ControlPersist multiplex socket if available so subsequent calls reuse the
   TCP connection.
4. **Materialize the module.** The `ping` module's source file (small Python
   script) is read on the controller, combined with `module_utils` boilerplate,
   AnsiballZ-wrapped (a self-extracting zipapp), and base64-or-binary
   transferred to the managed node into a per-task tempdir under
   `~/.ansible/tmp/`.
5. **Execute.** Run `python3 <tempdir>/AnsiballZ_ping.py` over the SSH session.
6. **Collect JSON.** The module writes a single JSON line to stdout: `{"ping":
   "pong", "changed": false, "invocation": {...}}`.
7. **Cleanup.** Remove the tempdir on the managed node.
8. **Aggregate.** Print one line per host in the default callback's `host |
   STATUS => {result}` format. For ping success this looks like:
   ```
   192.0.2.50 | SUCCESS => {
       "ansible_facts": {"discovered_interpreter_python": "/usr/bin/python3"},
       "changed": false,
       "ping": "pong"
   }
   ```

The "first time you run anything against a host" step also fingerprints the
SSH host key (per `~/.ssh/known_hosts` and `host_key_checking` setting) and
runs interpreter discovery (the auto-detect logic that picks `/usr/bin/python3`
vs `/usr/libexec/platform-python` etc.).

### 3.4 Run a playbook

`playbook.yaml`:

```yaml
- name: My first play
  hosts: myhosts
  tasks:
    - name: Ping my hosts
      ansible.builtin.ping:
    - name: Print message
      ansible.builtin.debug:
        msg: Hello world
```

```
$ ansible-playbook -i inventory.ini playbook.yaml
```

The execution adds an implicit `Gathering Facts` task at play start (unless
`gather_facts: false`), which runs `ansible.builtin.setup` and stores
hundreds of host facts under `ansible_facts.*`. The recap line (`PLAY RECAP`)
is the canonical post-run summary, with `ok / changed / unreachable / failed
/ skipped / rescued / ignored` counters per host.

---

## 4. Recommended directory layout for production projects

The `tips_tricks/sample_setup.html` doc is the single most-cited "best
practices" page. There are two officially-blessed layouts.

### 4.1 Standard layout

```
production                # inventory file for production servers
staging                   # inventory file for staging environment

group_vars/
   group1.yml             # variables assigned to particular groups
   group2.yml
host_vars/
   hostname1.yml          # variables assigned to particular systems
   hostname2.yml

library/                  # custom modules go here (optional)
module_utils/             # custom module_utils to support modules (optional)
filter_plugins/           # custom filter plugins (optional)

site.yml                  # main playbook
webservers.yml            # playbook for webserver tier
dbservers.yml             # playbook for dbserver tier
tasks/                    # task files included from playbooks
    webservers-extra.yml  # avoids confusing playbook with task files

roles/
    common/               # this hierarchy represents a "role"
        tasks/
            main.yml      # tasks file can include smaller files if warranted
        handlers/
            main.yml      # handlers file
        templates/
            ntp.conf.j2   # templates end in .j2
        files/
            bar.txt       # files for use with the copy resource
            foo.sh        # script files for use with the script resource
        vars/
            main.yml      # variables associated with this role
        defaults/
            main.yml      # default lower priority variables for this role
        meta/
            main.yml      # role dependencies and optional Galaxy info
        library/          # roles can also include custom modules
        module_utils/     # roles can also include custom module_utils
        lookup_plugins/   # or other types of plugins

    webtier/              # same structure as "common" above
    monitoring/
    fooapp/
```

Important configuration note from the doc: "By default, Ansible assumes your
playbooks are stored in one directory with roles stored in a sub-directory
called `roles/`." For larger setups, move playbooks under `playbooks/` and
adjust `roles_path` in `ansible.cfg`.

### 4.2 Alternative layout — inventories per environment

```
inventories/
   production/
      hosts
      group_vars/
         group1.yml
         group2.yml
      host_vars/
         hostname1.yml
         hostname2.yml
   staging/
      hosts
      group_vars/
         group1.yml
         group2.yml
      host_vars/
         stagehost1.yml
         stagehost2.yml

library/
module_utils/
filter_plugins/

site.yml
webservers.yml
dbservers.yml

roles/
   common/
   webtier/
   monitoring/
   fooapp/
```

The doc's commentary: "more flexibility for larger environments, as well as a
total separation of inventory variables between different environments…
[but] this approach is harder to maintain." The trade-off is duplication of
`group_vars/` and `host_vars/` per environment vs. risk of cross-environment
variable bleed.

### 4.3 Sample `group_vars`/`host_vars` content

Geographic group vars:

```yaml
---
# file: group_vars/atlanta
ntp: ntp-atlanta.example.com
backup: backup-atlanta.example.com
```

Functional group vars:

```yaml
---
# file: group_vars/webservers
apacheMaxRequestsPerChild: 3000
apacheMaxClients: 900
```

Universal defaults:

```yaml
---
# file: group_vars/all
ntp: ntp-boston.example.com
backup: backup-boston.example.com
```

Per-host overrides:

```yaml
---
# file: host_vars/db-bos-1.example.com
foo_agent_port: 86
bar_agent_port: 99
```

### 4.4 Playbook composition

The recommendation is a small `site.yml` that imports tier playbooks:

```yaml
---
# site.yml
- import_playbook: webservers.yml
- import_playbook: dbservers.yml
```

```yaml
---
# webservers.yml
- hosts: webservers
  roles:
    - common
    - webtier
```

That structure unlocks the canonical run patterns:

```bash
ansible-playbook -i production site.yml                       # full infra
ansible-playbook -i production site.yml --tags ntp            # by tag
ansible-playbook -i production webservers.yml                 # one tier
ansible-playbook -i production webservers.yml --limit boston  # one location
ansible-playbook -i production webservers.yml --limit boston[0:9]  # rolling
ansible-playbook -i production webservers.yml --tags ntp --list-tasks
ansible-playbook -i production webservers.yml --limit boston --list-hosts
ansible boston -i production -m ping                          # ad hoc
ansible boston -i production -m command -a '/sbin/reboot'     # ad hoc
```

### 4.5 Sample role contents

`roles/common/tasks/main.yml`:

```yaml
---
- name: be sure ntp is installed
  yum:
    name: ntp
    state: present
  tags: ntp

- name: be sure ntp is configured
  template:
    src: ntp.conf.j2
    dest: /etc/ntp.conf
  notify:
    - restart ntpd
  tags: ntp

- name: be sure ntpd is running and enabled
  ansible.builtin.service:
    name: ntpd
    state: started
    enabled: true
  tags: ntp
```

`roles/common/handlers/main.yml`:

```yaml
---
- name: restart ntpd
  ansible.builtin.service:
    name: ntpd
    state: restarted
```

The doc states explicitly: "Handlers are only triggered when certain tasks
report changes. Handlers run at the end of each play."

### 4.6 Local modules and plugins

A `./library/` directory next to a playbook is auto-added to the module path
so you can ship one-off modules with the playbook without packaging them in a
collection. Same applies for `./filter_plugins/`, `./lookup_plugins/`,
`./module_utils/`, etc.

### 4.7 Deployment vs. configuration

The doc draws a deliberate line between *configuration* (OS state, packages,
config files — long-lived) and *deployment* (application code rollout — fast-
moving). Recommendation: keep them in separate playbooks/roles even when they
target the same hosts. Add deployment-specific playbooks like
`deploy_exampledotcom.yml` alongside `site.yml`.

---

## 5. Anti-patterns the docs warn against

Compiled across the tips_tricks pages and the broader Best Practices
guidance.

### 5.1 God-vars and ungoverned `set_fact`

The tips guide is direct: "Whenever you can, do things simply." Don't pile
multiple variable-management strategies (CLI `-e`, group_vars, host_vars,
inline play vars, `set_fact`) on top of each other for the same value — pick
one and stick with it.

`set_fact` specifically:

- It produces a *static* value, evaluated once at assignment time.
- It writes to host-scoped variables that persist for the rest of the play
  *and* across plays in the same run, which can surprise you when a later play
  inherits stale state.
- With `cacheable: true` it spills into the fact cache, which mutates variable
  precedence (drops 7 steps) and creates two copies of the same name (one
  high-precedence host var, one lower-precedence ansible_fact). The docs flag
  this as a "possibly confusing interaction with `meta: clear_facts`" — the
  meta clears the fact but leaves the host var.
- Treat `set_fact` as you would mutable global state: minimize, scope tightly,
  prefer registered task results when possible.

### 5.2 Inventory bloat

- "Use meaningful, unique group names (case-sensitive). Avoid spaces, hyphens,
  and numeric prefixes." (`floor_19`, not `19th_floor`).
- Group by function (`webservers`, `dbservers`), location (`atlanta`,
  `frankfurt`), and lifecycle (`production`, `staging`, `dev`) — and let
  group hierarchies (`children:`) compose those axes rather than smashing them
  into one giant flat list.
- For cloud-managed estates, **use dynamic inventory plugins** instead of
  hand-maintaining static lists. The cloud provider is the source of truth.
- Keep production and non-production inventories in separate files or
  separate directory trees so a stray `--limit` cannot leak.

### 5.3 Deeply-nested roles

Although not always called out by name, the pattern the tips guide warns
against is "an advanced feature for an advanced use case." Roles importing
roles importing roles makes precedence opaque (because vars/defaults from
each role layer in at different priorities) and tag inheritance unpredictable.
Prefer:

- **Shallow role hierarchies**: one tier of meta `dependencies` if you must.
- **`include_role` over deep `meta/main.yml` chains** when role activation is
  conditional — explicit is better than implicit.
- Use `roles:` keyword for static unconditional inclusion; use
  `include_role`/`import_role` inside tasks for everything else.

### 5.4 `set_fact` abuse and lazy-vs-static confusion

Beyond the cacheable trap above:

- Don't use `set_fact` to "rename" a variable for convenience — you've doubled
  the bookkeeping.
- Don't use `set_fact` inside loops to accumulate a list — the static
  evaluation means every iteration overwrites; use `loop_control`/`register`
  + filters or build the list in defaults.
- Boolean strings (`yes`, `no`, `true`, `false`) auto-convert in `key=value`
  syntax but YAML notation behaves differently — pick one syntax and don't
  mix.
- Reserved names: the literal name `cacheable` cannot be used as a fact name
  because the module reserves the parameter name.

### 5.5 Insecure CLI shortcuts

- Do not pass secrets in `-e` on the command line — they end up in shell
  history and process lists.
- Do not bare-interpolate user input into `shell:` or `command:` — use the
  `quote` filter (`{{ var | quote }}`) or pass `argv:` lists.
- Do not disable `host_key_checking` on persistent infrastructure; use
  `accept_newhostkey: true` (Git module style) or pre-populate
  `~/.ssh/known_hosts`.
- Do not run `apt`/`yum`/`dnf` with `force: true` or `disable_gpg_check:
  true` outside short-term debugging — those flags exist for emergencies.

### 5.6 Configuration-dependent paths

Don't hardcode paths derived from `ansible.cfg`. The docs recommend the magic
variables `playbook_dir` and `role_name` so playbooks remain relocatable.

### 5.7 Forgetting `state:`

"Explicitly setting `state: present` or `state: absent` makes playbooks and
roles clearer." Module defaults differ (`file` defaults to `state: file`,
`apt` defaults to `state: present`, `service` requires `state` or `enabled`),
so omitting `state` makes intent ambiguous to readers.

### 5.8 Forgetting FQCNs

"Use fully qualified collection names (FQCN) to avoid ambiguity in which
collection to search for the correct module or plugin for each task." For
built-ins, this means `ansible.builtin.copy` not bare `copy`. This matters
when a Galaxy collection ships a module with a colliding short name (e.g.
`community.general.archive` vs the absent `ansible.builtin.archive`).

### 5.9 Skipping names on plays/tasks/blocks

Optional but "extremely useful." Tag-based runs, `--list-tasks`, debug
output, and the Tower/AWX UI all key off `name:`. An unnamed task shows up
in the recap as `TASK [ansible.builtin.copy] ********` — usable but noisy.

### 5.10 Skipping comments

Even with task names and explicit `state:`, the docs explicitly call out that
"sometimes a part of a playbook or role (or inventory/variable file) needs
more explanation." Comments are free.

---

## 6. Sample workflows from the docs

### 6.1 Classic three-environment promotion

```bash
ansible-playbook -i staging site.yml --check                # dry run on staging
ansible-playbook -i staging site.yml                        # apply to staging
ansible-playbook -i production site.yml --limit boston[0:9] # canary
ansible-playbook -i production site.yml --limit boston      # full Boston
ansible-playbook -i production site.yml                     # everywhere
```

The doc recommends `--syntax-check` first, then `--check` (no-op mode), then
real apply, with `serial:` controlling batch size for rolling updates.

### 6.2 OS / distro fan-out

The recommended pattern for dealing with mixed OSes is the `group_by` module:

```yaml
- name: Talk to all hosts just so we can learn about them
  hosts: all
  tasks:
    - name: Classify hosts depending on their OS distribution
      ansible.builtin.group_by:
        key: os_{{ ansible_facts['distribution'] }}
```

Then run OS-specific plays:

```yaml
- hosts: os_CentOS
  gather_facts: False
  tasks:
    - name: Ping my CentOS hosts
      ansible.builtin.ping:
```

…with OS-specific group_vars files (`group_vars/all.yml`, `group_vars/os_CentOS.yml`)
or in-play `include_vars`:

```yaml
- name: Use include_vars to include OS-specific variables and print them
  hosts: all
  tasks:
    - name: Set OS distribution dependent variables
      ansible.builtin.include_vars: "os_{{ ansible_facts['distribution'] }}.yml"
    - name: Print the variable
      ansible.builtin.debug:
        var: asdf
```

### 6.3 Vault for secrets, with searchable variable names

The `tips_tricks` page describes the canonical pattern for keeping secrets
encrypted while still letting you grep for variable names:

1. Make `group_vars/<group>/` a directory (not a file).
2. Inside it, create `vars` (plaintext) and `vault` (encrypted) files.
3. Define `db_password: "{{ vault_db_password }}"` in `vars`.
4. Define `vault_db_password: hunter2` in `vault`.
5. Encrypt `vault` with `ansible-vault encrypt`.
6. Roles consume `db_password`; reviewers `grep` for `db_password` and find it
   in `vars` (visible) without ever decrypting.

### 6.4 Custom callback for CLI output

Callback plugins reshape stdout. Common ones the docs mention: `default`
(per-task line-by-line), `dense` (compact), `yaml` (yaml-formatted task
results), `json` (full JSON for parsing), `minimal` (one line per task), and
`debug` (everything).

### 6.5 Execution Environments (EEs)

The tips guide pushes EEs as the modern way to ship a controller:
"Reduce complexity with portable container images known as Execution
Environments." An EE bundles `ansible-core`, a fixed set of collections,
their Python dependencies, and the system libraries those depend on. AWX/
Automation Controller speaks EE natively; the standalone `ansible-navigator`
CLI is the user-facing tool.

This is the doc's answer to "it works on my laptop but breaks on the build
agent" — pin the runtime in a container.

### 6.6 Ad-hoc one-liners

Throughout the docs, ad-hoc invocations are positioned as the fastest way to
inspect or nudge fleets:

```bash
ansible all -m ping
ansible all -m setup
ansible webservers -m service -a "name=httpd state=restarted" --become
ansible webservers -m shell -a "uptime"
ansible webservers -m copy -a "src=./hosts dest=/etc/hosts" --become
ansible all -i inventory -m apt -a "name=nginx state=latest update_cache=yes" --become
ansible all -m setup --tree /tmp/facts        # dump facts per host to disk
```

The shape `ansible <pattern> -m <module> -a "<args>"` should be a first-class
runsible CLI mode.

### 6.7 `--check` and `--diff`

Best-practice dry-run flow:

```bash
ansible-playbook site.yml --syntax-check
ansible-playbook site.yml --check --diff
ansible-playbook site.yml --check --diff --limit web01.example.com
ansible-playbook site.yml --diff
```

Modules that support `check_mode: full` will simulate; modules that support
`diff_mode: full` will additionally print a unified diff of changed files.
Modules that only have `check_mode: partial` (e.g. `command`/`shell` with
`creates`/`removes`) will skip during check and emit `skipping: [host]`.

---

## 7. Implications for runsible

(Not from the Ansible docs, but distilled here so the research lands with a
recommendation.)

### 7.1 First-run UX targets

`runsible` should match or beat the `ansible all -m ping` ritual:

```
$ runsible install   # ships single static binary; no Python deps
$ runsible inventory init
$ runsible ping
$ runsible run playbook.toml
```

The "no Python on the controller" promise is the primary differentiator.

### 7.2 Idempotency and check-mode are non-negotiable

Every first-tier module must declare a check_mode support level (full/partial/
none) and a diff support level. Module authors who can't articulate their
idempotency story shouldn't ship in v1.

### 7.3 Default to FQCN, ban shortname collisions

TOML tables are namespaced anyway; we should require `[ansible.builtin.copy]`-
style table headers and refuse bare `[copy]` to prevent the historical
"oops, I called the wrong copy" footgun.

### 7.4 Vault-equivalent must ship in v1

The "searchable variable names + encrypted values" pattern is so embedded in
how production Ansible is run that runsible needs an equivalent on day one.

### 7.5 Two layouts, both supported

Standard layout (single inventory file in repo root) and alternative layout
(`inventories/<env>/`) both have legitimate users. Don't force one. Provide
`runsible scaffold` for both.

### 7.6 `ansible.cfg` lessons

Search precedence (`$ANSIBLE_CONFIG` → `./ansible.cfg` → `~/.ansible.cfg` →
`/etc/ansible/ansible.cfg`) is sane and runsible should mirror it (with
`runsible.toml` instead). The "stock should be enough" promise is worth
preserving — most users never edit `ansible.cfg` and we should keep it that
way.

### 7.7 Roles are the unit of reuse

Galaxy is the secondary layer; roles inside a project are the primary one.
runsible's role schema should be a near-1:1 port (`tasks/`, `handlers/`,
`templates/`, `files/`, `vars/`, `defaults/`, `meta/`) so existing operators
can move muscle memory over.

### 7.8 The `setup` task is not free

Implicit fact gathering at play start runs *every* enabled fact collector on
every host. The docs nod to this with `gather_subset` and `gather_timeout`,
and `gather_facts: false` is a frequent escape hatch in tight loops. runsible
should make fact gathering opt-in or at least cheap-by-default (parallel,
cacheable, scoped subsets).
