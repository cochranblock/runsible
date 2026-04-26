# 09 — Connection, Become, Templating, and Facts

This document captures the conceptual surface area Ansible exposes through
connection plugins, become (privilege-escalation) plugins, Jinja2 templating, and
the fact-gathering subsystem. It is scoped to inform two crates in the
`runsible` workspace:

- `runsible-connection` — the worker that opens transports to remote hosts and
  shovels modules / files / commands across them.
- `runsible-playbook` — the part of the engine that embeds templating and
  applies privilege escalation when invoking a task.

Per-plugin reference is intentionally skipped. The goal is the *behavior we
must match or replace*, not the Python class hierarchy.

---

## 1. Connection — concepts and runtime surface

### 1.1 What a connection is in Ansible terms

A "connection" in Ansible is the abstraction that lets the controller execute
a *module* on a remote target. It is responsible for four things:

1. Establishing a transport session (TCP, exec, socket, etc.).
2. Copying module payload + arguments to a place the target can read.
3. Invoking an interpreter on the target to run the payload.
4. Streaming stdout/stderr/return code back, then cleaning up the temp dir.

Some connections collapse those steps (e.g. `local`, `pipelining` over `ssh`),
but the four-step shape is the model.

### 1.2 Connection plugins shipped or commonly used

These are the connection types `runsible` needs equivalents for, with the
single-line "what it's for":

- **`ssh`** — Default. Wraps the system OpenSSH client. Supports
  `ControlPersist` multiplexing, Kerberos/GSSAPI, jumphosts, anything you can
  put in `~/.ssh/config`. Fastest for general Linux/Unix targets.
- **`paramiko_ssh`** — Pure-Python SSH fallback. Used historically when system
  OpenSSH didn't support `ControlPersist` (RHEL 6 era). Deprecated for removal
  in Ansible 2.21. Useful conceptually because it shows the
  "no-shell-out" path.
- **`local`** — Runs the module in a child process on the controller; no
  transport at all. Used for `localhost`, `delegate_to: localhost`, and
  controller-side actions.
- **`winrm`** — Talks WS-Management to Windows targets. Auth: `kerberos`,
  `basic`, `ntlm`, `certificate`, `credssp`. Default port `5986` (HTTPS) or
  `5985` (HTTP). Required because Windows has no native SSH server in default
  Server SKUs (until OpenSSH-Win became standard).
- **`docker`** (community.docker) — Wraps `docker exec` / `docker cp`. Targets
  an *existing* container by name or ID. Requires `docker` CLI (≥ 18.06 for
  some features). `docker_api` is the daemon-socket variant.
- **`kubectl`** (kubernetes.core) — Wraps `kubectl exec` / `kubectl cp` against
  a pod. Needs `kubectl` on the controller and a usable kubeconfig. Pod must
  contain a Python interpreter for normal modules.
- **`community.general.lxd`** — Wraps the `lxc` CLI to run commands and copy
  files into LXD instances (containers or VMs).
- **Network persistent**: `network_cli` (CLI over SSH), `netconf` (XML over
  SSH), `httpapi` (REST over HTTPS). All require `network_os` and use the
  ansible-connection daemon (see §1.5).
- **Other historical**: `chroot`, `jail`, `lxc` (legacy), `psrp`
  (PowerShell Remoting), `buildah`, `podman`, `qubes`, `zone`. Not core, but
  worth knowing the shape: each is "exec into a thing that already exists."

### 1.3 Connection variables (host/group level)

Ansible exposes the entire connection layer as inventory variables; this is
the surface `runsible` must mirror in its host/group TOML:

General:
- `ansible_connection` — name of the connection plugin.
- `ansible_host` — resolvable name/IP if different from inventory alias.
- `ansible_port` — non-default port (22 for ssh, 5986 for winrm, …).
- `ansible_user` — login user.
- `ansible_password` — login password (vault-encrypted in practice).

SSH-specific:
- `ansible_ssh_private_key_file` — explicit private key path.
- `ansible_ssh_common_args` — appended to *every* `ssh`/`scp`/`sftp` invocation.
- `ansible_ssh_extra_args` — appended only to the `ssh` invocation.
- `ansible_sftp_extra_args`, `ansible_scp_extra_args` — same idea, per tool.
- `ansible_ssh_pipelining` — boolean, disable temp-file dance and pipe module
  source straight to interpreter stdin. Massive perf win, but requires
  `requiretty` to be off in sudoers.
- `ansible_ssh_executable` — override `/usr/bin/ssh` lookup.

Privilege escalation (covered in detail in §2):
- `ansible_become`, `ansible_become_method`, `ansible_become_user`,
  `ansible_become_password`, `ansible_become_exe`, `ansible_become_flags`.

Remote environment:
- `ansible_shell_type` — `sh`, `csh`, `fish`, `powershell`, …
- `ansible_shell_executable` — path to the shell to invoke on the remote.
- `ansible_python_interpreter` — explicit Python path on the target.
- `ansible_*_interpreter` — generic form (`ansible_ruby_interpreter`,
  `ansible_perl_interpreter`, …) for non-Python module languages.

### 1.4 Connection persistence: `ControlPersist`, `ControlMaster`, `ControlPath`

OpenSSH supports a "first connection opens a master socket; subsequent
connections multiplex over it" pattern. Ansible ships `ssh_args` defaulting to
roughly:

```
-C -o ControlMaster=auto -o ControlPersist=60s
```

Knobs Ansible exposes on the `ssh` connection plugin:

- `control_path` — explicit socket path. Default uses an MD5 hash of host/port
  /user under `~/.ansible/cp/`. The hashing matters because the path has to
  fit inside `sun_path` (≈ 108 bytes on Linux).
- `control_path_dir` — directory holding sockets (default `~/.ansible/cp`).
- `ssh_args` — full OpenSSH argument string; replaces the defaults if set.
- `ssh_common_args` / `ssh_extra_args` / `scp_extra_args` /
  `sftp_extra_args` — additive.
- `use_persistent_connections` (config option) — enables the
  `ansible-connection` daemon path for plugins that support it.

Behaviorally, ControlPersist means: the first task pays the TCP/auth cost; the
next 60 s of tasks reuse the connection. With 100 hosts × 30 tasks this turns
hours of SSH handshakes into seconds.

#### Rust equivalents for `runsible-connection`

Three realistic options, with trade-offs:

1. **Shell out to system `ssh`** (matches Ansible exactly).
   - Use the [`openssh`](https://docs.rs/openssh) crate. It is a thin wrapper
     around the system `ssh` binary that *uses ControlMaster internally* —
     `Session`/`SessionBuilder` create a master socket, and every spawned
     `Command` multiplexes over it. Two modes:
     `ProcessImpl` (spawn a child `ssh` per command — works on any OS) and
     `Mux` (talk directly to the control socket — faster, Linux/macOS).
   - Pros: identical semantics to Ansible, free `~/.ssh/config` and
     `ProxyJump` support, free GSSAPI, free PKCS11/SmartCard via `ssh-agent`.
   - Cons: requires system OpenSSH; fork/exec per command in `ProcessImpl`;
     control sockets clutter `/tmp`.
2. **Pure-Rust SSH** ([`russh`](https://docs.rs/russh)).
   - Tokio-based, channel-oriented, supports modern crypto (Ed25519,
     ChaCha20-Poly1305). SFTP via separate `russh-sftp` crate. No system
     `ssh` needed.
   - Pros: cleanly async; no temp sockets; can hold one `Session` per host
     with an unbounded number of `Channel`s — that *is* the multiplexing.
   - Cons: must implement what `~/.ssh/config` gives for free (jumphosts,
     `IdentityFile` resolution, `Match` blocks, `Include`); must implement
     known_hosts handling; less battle-tested in ops contexts than OpenSSH.
3. **Hybrid** — shell out to `ssh` for setup/jumphost/agent, then use
   `russh` for the actual session. Probably more pain than it's worth.

Recommendation for `runsible`: lean on `openssh` initially for Linux/macOS
parity with Ansible, expose a connection-plugin trait so a `russh` backend
can drop in later.

### 1.5 The `ansible-connection` daemon

For network device plugins (`network_cli`, `netconf`, `httpapi`) Ansible
spawns a long-lived `ansible-connection` worker process per host. The
playbook process talks to it over a Unix socket. It:

- Holds the SSH/HTTP session open across tasks (network gear is expensive to
  re-auth into).
- Buffers prompts / `enable` mode state.
- Lets multiple `ansible-connection` workers share a controller without each
  task forking ssh.

Conceptually, this is "ControlPersist for non-OpenSSH transports." For
`runsible-connection` the same effect can be achieved by:

- Holding a `tokio::sync::Mutex<Session>` per host inside the worker process.
- Or, if `runsible` itself becomes multi-process, factoring out a per-host
  daemon analogous to `ansible-connection`.

### 1.6 `delegate_to` — runtime connection swap

`delegate_to: <host>` rewrites the connection target for a single task at
execution time, while keeping the rest of the play's variables (especially
`inventory_hostname`) intact. Specifics:

- Connection variables (`ansible_host`, `ansible_user`, `ansible_port`,
  connection plugin, become plugin, shell plugin, interpreter…) are resolved
  *from the delegated host's* hostvars.
- `inventory_hostname` stays as the original. Anything you template from
  `hostvars[inventory_hostname]['x']` still refers to the original target.
- `delegate_facts: true` — facts gathered during the delegated task are
  written under the delegated-to host, not the original.
- `local_action: …` is sugar for `delegate_to: 127.0.0.1` with
  `connection: local`.
- `run_once: true` is often paired with `delegate_to` to run a one-shot
  on a single host instead of per-host.

Gotchas:
- The delegated host *does not inherit* variables from the delegating host —
  except connection variables, which are templated using the delegated
  hostvars.
- Forks still apply; if 50 hosts all `delegate_to: load_balancer`, you get
  ~5 parallel `ssh load_balancer` connections at once unless you serialize.
- Delegating to a host not in inventory works but is brittle; prefer
  `add_host` first.

For `runsible`: model a task execution as `(target_host, exec_host,
hostvars_source)` where `target_host` is the variable-namespace owner and
`exec_host` is what the connection actually opens.

### 1.7 Runtime concerns runsible must solve in Rust

- **TTY allocation**: `-tt` forces a PTY. Required when the remote sudo
  config has `requiretty` on. `openssh::Command` exposes this through
  `ssh_args`/`raw_arg("-tt")`. With `russh` you call `request_pty` on the
  channel. Default to "no PTY unless `become` and pipelining is off."
- **Password handling**: `ansible_password` and `ansible_become_password`
  must never reach a process arg vector or the environment. Approaches:
  - `sshpass -d <fd>` — feed password through a file descriptor (Ansible's
    historical fallback).
  - `SSH_ASKPASS` script — points to a wrapper that prints the password.
  - Native: hand bytes to `russh::client::Session::authenticate_password` or
    pipe to sudo's stdin (`-S`). No env vars, no `/proc/<pid>/cmdline` leak.
- **Timeouts**: distinguish "TCP connect timeout", "auth/banner timeout",
  "command execution timeout", and "data idle timeout". Ansible exposes
  `timeout` (connect) and `command_timeout` (network plugins). `runsible`
  should expose all four explicitly.
- **Reconnection on transient SSH failure**: `reconnection_retries` (default
  0 in Ansible). The retryable errors are: TCP RST, auth banner timeout,
  socket closed mid-stream. Auth failures should *not* retry. Implementation
  pattern: classify the error, retry only `Transient`, with exponential
  backoff and a hard cap.
- **Multiplexing**: see §1.4. `openssh` does it for you. `russh` does it via
  multiple channels per `Session`. Either way, hold sessions in a per-host
  pool keyed by `(user, host, port, plugin)` and drop them on a TTL.
- **Keepalives**: long playbooks need `ServerAliveInterval`/`ServerAliveCountMax`
  or russh-side keepalives, otherwise NAT boxes will drop idle sessions.
- **Host-key verification**: equivalent to OpenSSH's `StrictHostKeyChecking`.
  Default-on, with a clear escape hatch (`host_key_checking = false`,
  inventory `--ssh-extra-args="-o StrictHostKeyChecking=no"`). Implementation
  must read both `~/.ssh/known_hosts` and `/etc/ssh/ssh_known_hosts`, and
  understand `@cert-authority` and hashed host entries.
- **File transfer**: Ansible's `ssh_transfer_method` is `smart` (default,
  picks sftp then scp), `sftp`, `scp`, or `piped` (cat into stdin). For
  pipelining-disabled flows runsible needs the same matrix.

---

## 2. Become — privilege escalation

### 2.1 Concept and surface

Become is "after I'm logged in, switch to user X using mechanism Y." It is
*independent* of the connection: an SSH session as `deploy` plus
`become_user: root` via `become_method: sudo` produces `sudo -u root <cmd>`
inside the existing SSH channel.

Three task-level (or play-level) directives:
- `become` — boolean. **Setting other become_* values does not imply this.**
- `become_user` — target user (default `root`). Setting this alone does *not*
  enable escalation.
- `become_method` — the plugin to use (default `sudo`).
- `become_flags` — extra args to the chosen method.

Connection-variable equivalents (host/group/inventory):
- `ansible_become`, `ansible_become_method`, `ansible_become_user`,
  `ansible_become_password`, `ansible_become_exe`, `ansible_become_flags`.

CLI flags: `-b`/`--become`, `--become-method`, `--become-user`,
`-K`/`--ask-become-pass`.

### 2.2 Become methods (the conceptual menu)

Every method is "a setuid-ish program that takes a command line and runs it
as another user." Differences are in argument shape, password channel, and
environment behavior.

- **`sudo`** — Most common. Ansible runs
  `sudo -H -S -n -u <user> <shell> -c '<module invocation>'`. `-S` reads
  password from stdin (Ansible writes it through the SSH channel). `-n`
  fails fast if a password is needed but none is provided. `-H` resets HOME.
  Sensitive to `requiretty`.
- **`su`** — `su - <user> -c '<cmd>'`. Awkward password handshake (no
  `-S`-equivalent), Ansible has to pattern-match the prompt. Flags vary by
  distro (`-l`, `--login`, `-s /bin/sh`, …). Default prompt regex is
  configurable via `su_prompt_l10n`.
- **`pbrun`** — BeyondTrust PowerBroker. `pbrun -u <user> <cmd>`.
- **`pfexec`** — Solaris/illumos profile-based exec.
- **`doas`** — OpenBSD's lightweight sudo: `doas -u <user> <cmd>`. Config
  in `/etc/doas.conf`. No interactive password timeout subtlety like sudo.
- **`dzdo`** — Centrify's sudo-equivalent for AD-joined Unix.
- **`ksu`** — Kerberized su; needs valid TGT.
- **`runas`** — Windows. The *only* method supported on Windows. Uses the
  Secondary Logon service (`seclogon`); needs `SeDebugPrivilege` on the
  connection account or the target's `SeBatchLogonRight` /
  `SeNetworkLogonRight` for passwordless flows. `become_flags` for runas
  include `logon_type` (interactive/batch/network/network_cleartext/
  new_credentials) and `logon_flags` (with_profile, netcredentials_only).
- **`machinectl`** — `systemd`'s session-based escalation. Useful precisely
  *because* it opens a new systemd session, populating `XDG_RUNTIME_DIR`,
  `DBUS_SESSION_BUS_ADDRESS`, etc. — which sudo/su deliberately do not.
- **`sesu`** — CA Privileged Access Manager Server Control.
- **`enable`** — Network devices only. Promotes a `network_cli` session
  from EXEC into privileged EXEC mode (Cisco IOS et al.). Requires
  `connection: ansible.netcommon.network_cli` (or `httpapi`).

### 2.3 Order of evaluation

When Ansible composes a become invocation it walks (highest wins):

1. CLI flags (`--become`, `--become-method`, `--become-user`,
   `--ask-become-pass`).
2. Task-level `become*` directives.
3. Play-level `become*` directives.
4. Connection-variable `ansible_become*` for the target host.
5. Role/include defaults.
6. `ansible.cfg` `[privilege_escalation]` block (`become`, `become_method`,
   `become_user`, `become_ask_pass`).
7. Defaults (method `sudo`, user `root`, ask_pass `false`).

For `runsible`: replicate this precedence as a single resolved "BecomeSpec"
struct per task, computed at task-bind time so it can be logged for audit.

### 2.4 sudo vs su semantics, and the "shell quoting" gotcha

- `sudo -u user /bin/sh -c '<long base64 of module>'` works because sudo
  preserves arg vectors cleanly.
- `su - user -c '<cmd>'` invokes a login shell that re-parses the command
  string, so quoting is more fragile. Ansible has historical bugs here
  whenever the module payload contains stray `'`.
- `requiretty` in sudoers refuses sudo without a TTY. Ansible's escape
  hatch is `-tt` in `ssh_args`, or pipelining off.
- `Defaults targetpw` (sudo asks for the *target* user's password, not the
  invoker's) breaks Ansible's password-on-stdin flow if `become_user` is
  not the same user the password belongs to.
- sudo strips the environment by default. If a module relies on
  `LANG`/`PATH`, you need `Defaults env_keep` on the target.

### 2.5 "Unsafe writes" and unprivileged become

When the connection user is unprivileged *and* `become_user` is also
unprivileged (e.g. SSH as `deploy`, become `app` for an app-deploy step),
the temp module file written by `deploy` is unreadable by `app`. Ansible
walks a fallback chain to make it readable:

1. `setfacl` (POSIX ACLs) — preferred. Requires the `acl` package.
2. `chown` — only works if the connection user can chown to the become
   target (almost never).
3. `chmod +a` — macOS-specific extended ACLs (Ansible 2.11+).
4. `chgrp` to `ansible_common_remote_group` (Ansible 2.10+) — both users
   must be in that group.
5. `world_readable_temp` — last resort, `chmod a+r`. **Security risk**: any
   local user on the target can read the module payload while it exists.
   Sensitive arguments leak.

Mitigation:
- Pipelining (`ssh_pipelining = true`) skips the temp file entirely by
  feeding the module to the interpreter's stdin. **Pipelining is the
  proper fix to the unprivileged-become problem.**
- Or just become as root.

### 2.6 Fact gathering with become

A few facts only become correct when gathered *after* become has happened
(`ansible_user_id`, `ansible_user_uid`, `ansible_user_dir`, `ansible_env`,
some `/proc` data). A pattern is: gather minimal facts as the connection
user, then `become: yes` and `setup:` with `gather_subset: '!all,min'` for
the rest.

The `gather_facts` module respects per-play `become`. So if the play has
`become: true` and `gather_facts: true`, fact gathering itself runs as the
escalated user.

### 2.7 Runtime concerns for runsible's become layer

- **Password channel**: never put the password in an env var or argv.
  Stream it on a fresh fd, point sudo at that fd (`sudo -A` plus
  `SUDO_ASKPASS=<fd-helper>`), or write to the shell's stdin alongside the
  command. Zero on drop.
- **TTY**: detect `requiretty` flagging (sudo exits with "sudo: a terminal
  is required") and either reconnect with `-tt` or recommend pipelining.
  Track this per-host so we don't re-discover it every run.
- **Prompt detection for `su`/`enable`**: keep a small DFA that watches the
  channel for a configurable prompt regex, then writes the password and
  swallows the echoed line.
- **Audit logging**: emit `(host, become_method, become_user, success,
  duration)` so `runsible` can produce sudo-style audit traces.
- **Per-method capability matrix**: not every connection plugin supports
  every become method. `local` + `sudo` is fine; `winrm` only supports
  `runas`; `kubectl` ignores most because the container has its own user
  model. Encode this matrix at the trait level.
- **Don't double-escalate**: if the connection user is already root,
  `become: true` should be a no-op rather than a no-cost `sudo -u root`.
  Ansible's `become` plugin has this short-circuit; runsible should too.

---

## 3. Templating

Ansible's templating layer is Jinja2 plus a large library of additional
filters/tests/lookups, plus a few Ansible-specific magic values.

### 3.1 Jinja2 features Ansible relies on

- **Variables**: `{{ name }}`, attribute `{{ user.name }}`, item
  `{{ d['key'] }}`.
- **Filters** (`|`): `{{ value | upper }}`, chainable.
- **Tests** (`is`): `{% if x is defined %}`, `{% if path is file %}`.
- **Control structures**: `{% for %}`/`{% endfor %}` with `loop.index`,
  `loop.first`, `loop.last`, `loop.length`, `loop.previtem`,
  `loop.nextitem`; `{% if %}`/`{% elif %}`/`{% else %}`; `{% set %}`;
  `{% block %}`/`{% extends %}` for inheritance; `{% include %}`,
  `{% import %}`; `{% macro %}` with `caller`, `varargs`, `kwargs`;
  `{% call %}`; `{% filter %}`/`{% endfilter %}`; `{% raw %}`/`{% endraw %}`
  to escape Jinja syntax (handy for shipping Helm/Kustomize templates).
- **Whitespace control**: `{%-`, `-%}`, `{{-`, `-}}` strip surrounding
  whitespace. Engine-level: `trim_blocks` (default ON in Ansible),
  `lstrip_blocks` (default OFF), `keep_trailing_newline` (default OFF in
  Jinja, configurable per-template via the `template` module).
- **Expressions**: arithmetic (`+ - * / // % **`), comparison
  (`== != < <= > >=`), boolean (`and or not`), membership (`in`, `not in`),
  string concat (`~`), inline conditional (`'a' if x else 'b'`).
- **Lookups**: `{{ lookup('file', '/etc/hostname') }}`,
  `{{ query('fileglob', '/etc/conf.d/*.conf') }}` (q is shorthand for
  `lookup(..., wantlist=True)`).

### 3.2 The `omit` magic value

`omit` is an Ansible sentinel that removes a parameter from the module
invocation entirely. Pattern:

```yaml
user:
  name: alice
  uid: "{{ user_uid | default(omit) }}"
  shell: "{{ user_shell | default(omit) }}"
```

If `user_uid` is undefined, the `uid` key vanishes from the module call,
letting the module's own default apply. Without `omit`, you'd pass
`uid: None` (or `""`) and the module would either fail or set a bad value.

Implementation note for runsible: `omit` is not a real value, it's a
post-render filter on the dict that drops keys whose value equals the
sentinel. We need an equivalent token in the templating context that the
playbook layer scrubs before dispatch.

### 3.3 `default(value, true)` and friends

- `default(x)` — returns `x` if the variable is *undefined*. Defined-but-
  empty (`""`, `0`, `[]`) is left alone.
- `default(x, true)` — second positional `boolean` flag. Returns `x` if the
  variable is *falsy* (undefined OR empty OR zero OR false). Use this when
  you want "fall back to a default for empty strings as well."
- `default(omit)` — combine with §3.2 to skip the parameter entirely.
- `mandatory` — opposite of default: raise if undefined. With message:
  `{{ var | mandatory("var must be set") }}`.

### 3.4 Type coercion gotchas

This is where Ansible bites people:

- **Strings that look like booleans** — `enabled: yes`, `enabled: "yes"`,
  `enabled: True`, `enabled: "True"` may all behave differently depending
  on whether YAML parses them as booleans or strings, and whether the
  module re-coerces. Use `| bool` to be explicit:
  `when: my_var | bool`. The `bool` filter accepts
  `True/true/yes/on/1` and `False/false/no/off/0`.
- **Strings that look like numbers** — `port: "8080"` vs `port: 8080`.
  YAML unquotes; Jinja preserves the type from the source. `int` filter
  forces coercion; `string` filter forces back. Modules sometimes accept
  either, sometimes don't.
- **Leading-zero literals** — YAML 1.1 parsed `0123` as octal; YAML 1.2
  doesn't. Ansible uses 1.1 semantics in places and 1.2 in others. Always
  quote things like ZIP codes or octal modes.
- **File modes** — `mode: 0644` is the integer 420 (octal in YAML 1.1).
  `mode: "0644"` is the string `"0644"`. Modules want one or the other.
  Use `mode: "0644"` consistently and tell users to quote.
- **Jinja stringification** — Anything wrapped in `{{ … }}` returns a
  string by default unless the result is itself a dict/list. If you do
  `port: "{{ base_port + 1 }}"`, you get `"8081"` (string). If you do
  `port: "{{ base_port + 1 | int }}"`, still string. Drop the quotes:
  `port: "{{ base_port + 1 }}"` *with* `port: "{{ base_port + 1 | int }}"`
  doesn't help. Use the `!!int` YAML tag or rely on `ansible_facts` types.
- **Undefined vs None vs empty** — `{{ x }}` raises if `x` is undefined,
  but only on first access; deeply nested chains may render `''` for
  intermediate undefineds depending on `DEFAULT_UNDEFINED_VAR_BEHAVIOR`.

For runsible (Rust + TOML): TOML has stricter typing — strings are quoted,
ints and bools are unambiguous — so most of these go away by construction.
But the templating engine still needs to decide what `{{ a + b }}` returns
when `a` is int and `b` is string, and we should refuse implicit coercion
loudly.

### 3.5 Regex filter family

- `regex_search(pat, *groups, multiline=False, ignorecase=False)` — first
  match, returns the matched substring or named/numbered groups.
- `regex_findall(pat, multiline=False, ignorecase=False)` — list of all
  matches.
- `regex_replace(pat, repl, multiline=False, ignorecase=False, count=0)` —
  substitute; `\1`/`\g<name>` backrefs in `repl`.
- `regex_escape(re_type='python')` — escape regex metachars in a literal
  string. `re_type` can be `python` or `posix_basic`.

These are Python `re`-flavored, so for runsible we should pick a Rust
regex crate (`regex` for full ECMA-ish, or `pcre2` for true PCRE) and
document the dialect difference up front.

### 3.6 Filter library — every shipped Ansible filter, one-liner

Data conversion:
- `to_json` — render to JSON.
- `to_nice_json` — pretty-printed JSON (indent=4 default).
- `from_json` — parse JSON string.
- `to_yaml` — render to YAML.
- `to_nice_yaml` — pretty YAML.
- `from_yaml` — parse a single YAML doc.
- `from_yaml_all` — parse a multi-document YAML stream.

Variable handling:
- `default(val[, boolean])` — fallback for undefined (or falsy if
  `boolean=True`).
- `mandatory[(msg)]` — raise if undefined.
- `omit` (sentinel, not a filter) — see §3.2.

Type:
- `bool` — coerce to boolean.
- `int` — coerce to integer.
- `float` — coerce to float.
- `string` — coerce to string.
- `type_debug` — return the underlying Python type name (debugging aid).

Dict / list shape:
- `dict2items` — `{a: 1, b: 2}` → `[{key: a, value: 1}, …]`.
- `items2dict` — inverse, with optional `key_name`/`value_name`.
- `combine(other, recursive=False, list_merge='replace')` — dict union.
- `subelements(path, skip_missing=False)` — Cartesian over a sub-list.
- `zip(other, …)` — pair lists into tuples.
- `zip_longest(other, fillvalue=None)` — same, padding short lists.
- `flatten(levels=None)` — un-nest lists.
- `unique` — deduplicate.
- `union(other)` — set union, dedup.
- `intersect(other)` — set intersection.
- `difference(other)` — set difference.
- `symmetric_difference(other)` — XOR.
- `min`/`max` (with optional `attribute=`).
- `sort(reverse=False, case_sensitive=False, attribute=None)`.
- `reverse`.
- `length` (Jinja built-in).
- `count` (alias).

Combinatorics:
- `permutations(n)`.
- `combinations(n)`.
- `product(other, …)`.
- `batch(size)` — group into chunks.

Strings:
- `split(sep)`.
- `splitext` — `("foo", ".tar.gz")` style.
- `join(sep)` (Jinja built-in).
- `quote` — shell-quote.
- `comment(decoration='#'|'C'|'C++'|'erlang'|'XML'|'cblock', prefix='', postfix='')`.
- `regex_search`, `regex_findall`, `regex_replace`, `regex_escape` (§3.5).
- `b64encode` / `b64decode`.
- `to_uuid(namespace=…)` — UUIDv5.
- `expanduser` — `~user` → home.
- `expandvars` — `$VAR` substitution.

Paths (POSIX):
- `basename` / `dirname` / `realpath` / `relpath`.
- `path_join` — like `os.path.join`.

Paths (Windows):
- `win_basename` / `win_dirname` / `win_splitdrive`.

URL / network:
- `urlencode` / `urlsplit(component='hostname'|'port'|'path'|…)`.
- `ipaddr(query='')` — Swiss army knife: validate, normalize, extract
  `network`, `prefix`, `version`, `host_format`, etc.
- `ipv4(query='')`, `ipv6(query='')` — typed wrappers.
- `ipsubnet(query, index=None)`, `ipmath(amount)`, `ipwrap`.
- `network_in_network`, `network_in_usable`, `reduce_on_network`,
  `nthhost`, `next_nth_usable`, `previous_nth_usable`, `cidr_merge`.
- `parse_cli`, `parse_cli_textfsm`, `parse_xml`, `vlan_parser` —
  network-device output parsing.
- `random_mac(prefix=None, seed=None)`.

Crypto / hashing:
- `hash(algo='sha1')`.
- `checksum` — alias of sha1 hash on a string.
- `password_hash(scheme='sha512', salt=None, rounds=None)`.
- `vault(secret, vault_id='default')` — encrypt a value.
- `unvault(secret)` — decrypt.

Random:
- `random(end=None, start=0, step=1, seed=None)` — random pick or number.
- `shuffle(seed=None)` — list permutation.

Date / time:
- `to_datetime(format='%Y-%m-%d %H:%M:%S')` — string → datetime.
- `strftime(format, second=None, utc=False)` — format an epoch.

Math:
- `abs` (Jinja).
- `round(precision=0, method='common')` (Jinja).
- `log(base=e)`.
- `pow(exp)`.
- `root(base=2)`.

Selection:
- `extract(container, morekeys=None)` — index lookup with chaining.
- `json_query(expr)` — JMESPath.
- `map(attribute=…)` / `select(test)` / `reject(test)` /
  `selectattr(attr, test)` / `rejectattr(attr, test)` (Jinja).
- `ternary(true_val, false_val=None, none_val=None)` — `cond | ternary(a,b)`.

Misc:
- `k8s_config_resource_name(hash_length=10)` — ConfigMap-style name suffixing.

### 3.7 Test library — every shipped Ansible test

(`is` operator: `{% if x is defined %}`.)

Type:
- `string`, `mapping`, `sequence`, `iterable`, `number`, `integer`, `float`,
  `boolean` — Python type tests.
- `defined` / `undefined` — Jinja built-in.
- `none` — value is `None`.
- `callable` (Jinja built-in).

Truth:
- `truthy(convert_bool=False)` — Python truthy semantics.
- `falsy(convert_bool=False)` — inverse.

String:
- `match(pat)` — anchored regex at start.
- `search(pat)` — regex anywhere.
- `regex(pat, ignorecase=False, multiline=False, match_type='search')` —
  general regex.

Math:
- `even`, `odd`, `divisibleby(n)` — Jinja built-ins.

Version:
- `version(other, op='==' , version_type='loose')` — full semver-ish
  comparison; ops `< <= == != >= >`. `version_type` can be `loose`,
  `strict`, `semver`, `pep440`.
- `version_compare` — alias.

Set:
- `subset` / `superset`, `contains(value)`.

Path (the value is a path string on the controller):
- `file`, `directory`, `link`, `exists`, `mount`, `same_file(other)`,
  `abs`.

Task result (apply to a registered variable):
- `failed`, `changed`, `succeeded`/`success`, `skipped`,
  `finished` (async), `started` (async).

Vault:
- `vault_encrypted` — value is a `!vault` inline ciphertext.

Distribution / facts (in some collections):
- `distribution`, `distribution_major_version`, `distribution_release`.

Misc:
- `nan`, `inf` — IEEE 754 specials.
- `any`/`all` — list-of-bools quantifiers.

### 3.8 Lookup plugins

Lookups run on the **controller**, not the target. Two entry points:

- `lookup('name', *args, **kwargs)` — returns a single result (or a
  comma-joined string for backwards compat) by default.
- `query('name', *args, **kwargs)` (alias `q`) — always returns a list.
  Equivalent to `lookup(..., wantlist=True)`.

`errors='ignore'` (default `strict`) makes a failing lookup return an empty
list instead of raising.

Pre-Ansible-2.5 the `with_<lookup>` syntax was the loop construct
(`with_items`, `with_fileglob`, …). Modern style is `loop:` plus an
explicit `query()`.

Every shipped builtin lookup, one-liner:

- `config` — read a resolved Ansible config option.
- `csvfile` — pull a value from a CSV/TSV file by row key.
- `dict` — turn a dict into iterable `(key, value)` items.
- `env` — read a controller-side environment variable.
- `file` — read the contents of a file on the controller.
- `fileglob` — glob a path pattern, return matching paths.
- `first_found` — return the first existing path from a list (handy for
  per-OS template selection).
- `indexed_items` — like `items` but yields `(idx, item)` tuples.
- `ini` — read a key from an INI file.
- `inventory_hostnames` — expand an Ansible host pattern to a list.
- `items` — yield each list element (legacy `with_items` equivalent).
- `lines` — run a controller-side command, yield each stdout line.
- `list` — return the input as-is (useful with `loop:` to make wantlist
  semantics explicit).
- `nested` — Cartesian product across multiple lists.
- `password` — read or generate a random password, persist it to a file.
- `pipe` — run a controller-side command, return stdout (entire blob).
- `random_choice` — pick one element at random.
- `sequence` — generate a numeric range (with format/start/end/step).
- `subelements` — pair items with a sub-list inside each.
- `template` — render a Jinja template on the controller, return the
  result (used to pre-render content before passing to a module).
- `together` — zip lists into one synchronized list.
- `unvault` — read a vault-encrypted file.
- `url` — HTTP GET the URL on the controller, return body.
- `varnames` — match variable names by regex, return the names.
- `vars` — fetch a variable's value by name.

Common community/galaxy lookups (out of scope for runsible's first cut but
worth noting they exist): `redis`, `dig`, `dnstxt`, `etcd`,
`hashi_vault`, `k8s`, `mongodb`, `manifold`, `aws_ssm`, `aws_secret`,
`gcp_secret_manager`, `consul_kv`, `keepass`, `passwordstore`.

### 3.9 vars_prompt, vars_files, encrypted variables

`vars_prompt`:
- Per-prompt fields: `name`, `prompt`, `default`, `private` (default true,
  no echo), `confirm` (re-enter), `unsafe` (allow `{`/`%` in input
  without templating), `encrypt` (apply a crypt scheme), `salt`,
  `salt_size` (default 8), `when` (condition).
- `encrypt` schemes: `sha512_crypt`, `sha256_crypt`, `bcrypt`,
  `md5_crypt`, `des_crypt`, `pbkdf2_sha512`, etc. — anything Passlib
  supports. Without Passlib, falls back to Python's `crypt`.
- Skipped automatically when `--extra-vars` already defines the variable
  or when run non-interactively (no TTY on stdin).

`vars_files`:
- Play-level `vars_files: [secrets.yml, vars/{{ env }}.yml]`. Templated
  per-host, so per-host file selection works.
- Files may be plain YAML or vault-encrypted; Ansible decrypts
  transparently if a vault password is available.

Encrypted variables (Ansible Vault):
- File-level: `ansible-vault encrypt path/to/file.yml`. Header is
  `$ANSIBLE_VAULT;1.1;AES256` (or `1.2` with vault IDs).
- Inline value: `ansible-vault encrypt_string 'secret' --name 'api_key'`
  emits a `!vault |` block you paste into a vars file. Mixes
  encrypted and plaintext keys in one file.
- Multiple vault IDs: `--vault-id dev@~/.vault-dev --vault-id prod@~/.vault-prod`.
  Ansible tries IDs in order until one decrypts.
- `ANSIBLE_VAULT_PASSWORD_FILE` env var for unattended runs.
- Limitation: inline encrypted strings cannot be re-keyed; whole files can.

For runsible, equivalents to plan:
- A `runsible-vault` CLI matching the `ansible-vault` verbs.
- File header that's TOML-friendly (top-of-file comment or magic key).
- Native `!vault` analogue in TOML: a string with a recognized
  `runsible-vault:` prefix or a separate `[secrets]` table that is
  decrypted lazily.
- Pluggable KMS backends from day one (file password, OS keychain,
  age/rage recipients, AWS KMS, GCP KMS, Vault).

---

## 4. Facts and gathering

### 4.1 The setup module and `gather_facts`

`gather_facts: true` (the default) makes Ansible run
`ansible.builtin.setup` against each target before the first task. The
result is a dict published as `ansible_facts` (and, by default, exploded
into top-level vars prefixed `ansible_*`).

Disable via `gather_facts: false` at the play level, or globally via
`gathering = explicit` in `ansible.cfg`. Then call `setup:` only where
needed.

### 4.2 `gather_subset`, `gather_timeout`, `fact_path`

- `gather_subset` — list of subset names. Recognized values:
  - `all` (default) — everything except `facter`/`ohai`.
  - `min` — only the cheapest facts (distribution, hostname, basic
    interface, env).
  - `hardware` — DMI, CPU details, memory, devices, mounts. Expensive.
  - `network` — full interface table, default routes, DNS.
  - `virtual` — virtualization role/type detection.
  - `facter` — shell out to Puppet's `facter`.
  - `ohai` — shell out to Chef's `ohai`.
  - Negation: `!hardware` excludes hardware. List form combines:
    `gather_subset: ['min', 'network', '!virtual']`.
- `gather_timeout` — per-subset timeout (default 10 s). Hardware on a
  spinning-rust SAN can blow this.
- `fact_path` — directory the *target* scans for `*.fact` files (custom
  facts; see §4.5). Default `/etc/ansible/facts.d`.

### 4.3 The fact tree (`ansible_facts.*` major keys)

Top-level keys you can rely on across modern Linux:

- **System identity**: `hostname`, `nodename`, `fqdn`, `domain`,
  `machine_id`.
- **OS / distribution**: `system` (Linux/FreeBSD/Darwin/…),
  `os_family` (RedHat/Debian/Suse/Archlinux/…), `distribution`,
  `distribution_release`, `distribution_version`,
  `distribution_major_version`, `lsb` (subdict).
- **Kernel**: `kernel`, `kernel_version`, `cmdline` (kernel command line as
  a dict), `architecture`, `machine`.
- **CPU**: `processor` (list), `processor_cores`, `processor_count`,
  `processor_nproc`, `processor_vcpus`, `processor_threads_per_core`.
- **Memory**: `memory_mb` (subdict with real/swap/nocache), `memtotal_mb`,
  `memfree_mb`, `swaptotal_mb`, `swapfree_mb`.
- **BIOS / firmware**: `bios_date`, `bios_version`, `bios_vendor`,
  `board_name`, `board_serial`, `board_vendor`, `board_version`,
  `chassis_serial`, `chassis_vendor`, `chassis_version`,
  `form_factor`, `product_name`, `product_serial`, `product_uuid`,
  `product_version`, `system_vendor`.
- **Networking**: `default_ipv4`, `default_ipv6` (subdicts:
  `address`, `netmask`, `network`, `broadcast`, `gateway`, `interface`,
  `macaddress`, `mtu`, `type`), `all_ipv4_addresses`,
  `all_ipv6_addresses`, `interfaces` (list of names), and per-interface
  subdicts (`eth0`, `wlan0`, `bond0`, …) with `device`, `ipv4`, `ipv6`,
  `macaddress`, `mtu`, `active`, `module`, `type`, `speed`. Plus `dns`
  (search/nameservers).
- **Storage**: `mounts` (list of `mount`/`device`/`fstype`/`options`/
  `size_total`/`size_available`/`uuid`), `devices` (subdict per block
  device with partitions, holders, vendor, model), `device_links`.
- **Service / packaging**: `service_mgr` (`systemd`/`upstart`/`sysvinit`/…),
  `pkg_mgr` (`apt`/`yum`/`dnf`/`zypper`/`pacman`/…).
- **Security**: `selinux` (subdict), `selinux_python_present`,
  `apparmor` (status), `fips` (bool), `iscsi_iqn`.
- **Virtualization**: `virtualization_role` (host/guest/NA),
  `virtualization_type` (kvm/xen/vmware/docker/lxc/…), `virtualization_tech_guest`,
  `virtualization_tech_host`.
- **Runtime user**: `user_id`, `user_uid`, `user_gid`, `user_dir`,
  `user_shell`, `user_gecos`, `effective_user_id`, `effective_group_id`.
- **Environment**: `env` (full env dict), `python` (subdict with
  `executable`, `version`, `version_info`, `has_sslcontext`),
  `python_version`.
- **Time**: `date_time` (subdict with `year`, `month`, `day`, `hour`,
  `minute`, `second`, `epoch`, `iso8601`, `iso8601_basic`,
  `iso8601_basic_short`, `iso8601_micro`, `tz`, `tz_offset`, `weekday`,
  `weeknumber`).
- **Uptime / boot**: `uptime_seconds`, `is_chroot`, `proc_cmdline`.
- **SSH host keys**: `ssh_host_key_rsa_public`,
  `ssh_host_key_ecdsa_public`, `ssh_host_key_ed25519_public`,
  `ssh_host_key_dsa_public` (and the `_keytype` companions).
- **Custom**: `local` — see §4.5.
- **Meta**: `gather_subset` (what was actually collected),
  `module_setup` (always `true`).

`hostvars[other_host].ansible_facts.x` works *only if* you've gathered
or cached facts for `other_host` already.

### 4.4 Fact caching backends

`fact_caching = <plugin>` plus `fact_caching_connection`,
`fact_caching_prefix`, `fact_caching_timeout`. Built-in backends:

- `memory` — default. In-process dict; vanishes between runs.
- `jsonfile` — JSON files under a directory (one per host).
- `yaml` — YAML files under a directory.
- `redis` — keys in Redis. `fact_caching_connection = host:port:db`.
- `memcached` — keys in memcached.
- `mongodb` — documents in a Mongo collection.

Caching lets you gather once (e.g. nightly cron'd `setup` run) then run
hundreds of small playbooks during the day with `gather_facts: false`,
relying on cached `ansible_facts`. `set_fact: cacheable: yes` writes a
custom fact into the same backend so it survives across plays.

For runsible: pick `memory` + `jsonfile` for the v1 surface, abstract the
backend behind a `FactCache` trait. Redis/Mongo are bolt-ons.

### 4.5 `ansible_local` — custom facts

Each target can ship its own facts:

- Drop files into `/etc/ansible/facts.d/<name>.fact` (path overridable
  via `fact_path` on the play).
- Static formats: INI or JSON. INI section/key pairs become nested dicts;
  keys are *lowercased* on parse.
- Executable formats: any `+x` script that prints valid JSON to stdout
  becomes a dynamic fact. Run as the connection user (or become user if
  the play has `become: true`).
- Exposed as `ansible_facts.local.<name>.<section>.<key>` (or
  `ansible_local['name']['section']['key']`).

Example `nginx.fact` (INI):

```
[server]
worker_processes = 4
worker_connections = 1024
```

Becomes `ansible_local.nginx.server.worker_processes = "4"` (note: still
a string from INI; user must coerce with `| int`).

To re-pick up changes mid-play, re-run `setup: filter=ansible_local`.

For runsible: support the same `/etc/ansible/facts.d` path *plus* a
runsible-native location like `/etc/runsible/facts.d/`. INI + JSON +
TOML for static, executable returning JSON or TOML for dynamic.

### 4.6 Fact precedence vs. variables

Ansible's full 22-tier precedence order (lowest → highest, the highest
wins) is approximately:

1. command-line values (e.g. `-u user`)
2. role defaults (`roles/*/defaults/main.yml`)
3. inventory file or script group vars
4. inventory `group_vars/all`
5. playbook `group_vars/all`
6. inventory `group_vars/*`
7. playbook `group_vars/*`
8. inventory file or script host vars
9. inventory `host_vars/*`
10. playbook `host_vars/*`
11. host facts / cached set_facts
12. play vars
13. play `vars_prompt`
14. play `vars_files`
15. role vars (`roles/*/vars/main.yml`)
16. block vars (only for tasks in the block)
17. task vars (only for that task)
18. include_vars
19. set_facts / registered vars
20. role (and include_role) params
21. include params
22. extra vars (`-e`) — always win

Key consequences for facts:
- Cached / freshly-gathered facts (level 11) outrank inventory and group
  vars but are beaten by play vars, role vars, set_fact, include params,
  and -e.
- A `set_fact` inside a play *will* override a gathered fact for the
  remainder of the run — but the cached version on disk doesn't change
  unless `cacheable: yes`.
- Gotcha: `ansible_user` set in inventory beats a fact named
  `ansible_user_id`; they're different keys but easy to confuse.

For runsible's design: ship the same precedence chain (renumbered if we
collapse a few levels), document it loudly, and provide a
`runsible debug vars --host h --task t` command that prints, per
variable, *which layer* won.

---

## 5. Implications for runsible (synthesis)

What this research implies for the two crates that triggered it:

### 5.1 `runsible-connection`

- Define a `Connection` trait covering `connect`, `exec`, `put`, `fetch`,
  `close`, with optional `tty`, `env`, `cwd`, `timeout` per-call.
- Ship `ssh` (via `openssh` crate, ControlMaster-backed), `local`, and
  `docker`/`kubectl`/`podman` (via subprocess) as the v1 set. `winrm` and
  `lxd` are stretch goals.
- Per-host `Session` pool keyed by `(plugin, user, host, port)` with a
  configurable TTL (default 60 s, mirroring ControlPersist). Sessions
  drop on idle, on explicit close, or on classified-transient failure.
- A `BecomeSpec { method, user, password, exe, flags }` resolved per task
  via the precedence chain in §2.3. `Connection::exec` accepts
  `Option<&BecomeSpec>` and applies it inside the existing channel.
- Password handling: a `Secret` newtype with `Drop` zeroing, `Debug`/
  `Display` impls that print `***`, and a one-shot `expose(&self) ->
  &[u8]` that requires explicit unlocking. Never serialized.
- Pipelining: enabled by default; disabled per-task when the module
  payload exceeds an OS-specific stdin buffer or when the user asks.
- TTY: default off, on-demand for `become_method = sudo` when the host
  has been observed to require it (cache the observation in
  `runsible-state`).
- Reconnection: classify `io::Error` and `russh::Error` /
  `openssh::Error` into `Transient | Auth | Fatal`; retry only
  `Transient` with capped exponential backoff.
- Structured audit log: every exec emits
  `(timestamp, host, plugin, user, become, command_summary, duration,
  exit, bytes_in, bytes_out)`.

### 5.2 `runsible-playbook`

- Templating engine: pick `minijinja` (closest Jinja2 superset for Rust)
  or build a TOML-native expression language. minijinja gives us 90% of
  the Jinja2 surface for free, including `for`/`if`/`set`/`macro`/
  `include`/`extends`/`raw` and the `is`/`|` operators. Add custom
  filters (§3.6) and tests (§3.7) on top.
- Implement `omit` as an internal sentinel (`OmitMarker`) that the
  task-dispatch layer scrubs from the rendered argument map before
  encoding the module call.
- Implement `default(value, true)` semantics by adding a custom
  `default` filter that takes an optional `boolean` flag. minijinja's
  built-in `default` does not match Ansible's two-arg form.
- Vault: `runsible-vault` crate. File-level + inline string formats. Age
  for v1 (simpler than AES-256-CTR with HMAC), AES-256-GCM if we want to
  match Ansible byte-for-byte. Pluggable secret source (file, env,
  keychain, KMS).
- Facts: a `gather` task that runs a single shell snippet on the target
  (uname, /etc/os-release, /proc, ip, etc.) and returns a flat TOML
  table, cached via the `FactCache` trait. Subset support mirroring
  `min`/`hardware`/`network`/`virtual`. Custom facts via
  `/etc/runsible/facts.d/` plus a fallback to `/etc/ansible/facts.d/`.
- Lookups: `runsible-lookups` crate with the builtin set in §3.8. Make
  them async from the start so a `query('url', …)` doesn't block the
  task scheduler.
- Variable precedence: a `VarLayer` enum with explicit ordinal, and a
  resolver that walks layers high → low. Expose
  `runsible debug vars --host H --task T --var V` to print the winning
  layer.
- Type coercion: by default refuse implicit string↔int↔bool coercion;
  require `| int` / `| bool` / `| string` filters. TOML's strict typing
  makes this much less painful than YAML.
- delegate_to: model task execution as
  `TaskInstance { vars_host: HostId, exec_host: HostId, … }`. Connection
  resolution uses `exec_host`'s connection vars; everything else uses
  `vars_host`. `delegate_facts: true` reroutes the post-task fact
  ingestion to `exec_host`.

---

## 6. Open questions / future research

- **Async fact gathering**: Ansible serializes `setup` per host; with a
  Tokio runtime we can fan it out. Need to confirm the upper bound of
  parallel SSH sessions before the controller's fd limit / kernel
  control-socket cap bites.
- **Fact-caching schema**: do we lock to JSON for portability with
  `ansible-playbook`'s `jsonfile` cache, or pick a binary format
  (postcard, bincode) for our own?
- **Vault file format**: byte-compatible with Ansible Vault, or
  age-based (modern, ed25519 recipients, much simpler crypto)? Probably
  age for `runsible`-native files, with a one-way importer for Ansible
  vault files.
- **Network device support**: do we need `network_cli`/`netconf`/
  `httpapi` analogs in v1, or is "use SSH and an exec module" enough
  initially?
- **Windows**: `winrm` is a substantial project (NTLM, Kerberos SPNEGO,
  CredSSP, encrypted message envelopes). Defer past v1 unless there's a
  user pulling for it.
- **Connection plugin third-party API**: do we want to expose a stable
  trait so out-of-tree connection plugins can ship as separate crates,
  or keep it internal until the design settles?
