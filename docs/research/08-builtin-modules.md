# 08 — `ansible.builtin` Module Catalog

> Source of record:
> `https://docs.ansible.com/ansible/latest/collections/ansible/builtin/index.html`
> plus each module's individual docs page.
>
> Scope: every module that ships in the `ansible.builtin` namespace as of
> ansible-core 2.19+ (the version matrix the docs currently default to).
> Anything outside `ansible.builtin` is explicitly out of scope; runsible
> ships the built-ins, third parties can ship the rest.
>
> Format per entry:
> ```
> ### <name>
> - **Purpose**: one-liner
> - **Required params**: param (type)
> - **Key optional params**: list with type and default
> - **Returns**: key fields
> - **Idempotent**: yes/no, why
> - **Check-mode supported**: yes/no/partial
> - **Notes**: quirks
> ```
>
> "Idempotent" answers the question: if I run this task twice in a row with
> the same inputs, does the second run report `changed=false`? "Check-mode
> supported" maps to Ansible's check_mode attribute (full / partial / none).
>
> Module count: ~70 modules. Network-device redirects (aireos, asa, ce,
> cnos, dellos*, enos, eos, exos, ios, iosxr, ironware, junos, net_base,
> netconf, network, nxos, slxos, sros, voss, vyos, bigip*, bigiq, aruba)
> are *not* implementations — they all redirect to ansible.netcommon or the
> matching vendor collection. Those are out of scope for runsible's first-tier.
>
> Aliases / redirects worth flagging:
> - `ansible.builtin.yum` → redirect to `ansible.builtin.dnf`
> - `ansible.builtin.systemd` → redirect to `ansible.builtin.systemd_service`
> - `ansible.builtin.include` → REMOVED (was deprecated in favour of
>   `include_tasks`/`import_tasks`)
>
> Modules the user requested that **do not exist in ansible.builtin** (and
> therefore runsible should not implement under that namespace):
> - `archive` → `community.general.archive`
> - `at` → `ansible.posix.at`
> - `mount` → `ansible.posix.mount`
> - `sysctl` → `ansible.posix.sysctl`
> - `mail` → `community.general.mail`
> - `crypto` → `community.crypto.*` family
> - `pacman` → `community.general.pacman`
> - `zypper` → `community.general.zypper`
> - `homebrew` → `community.general.homebrew`
> - `validate` → not a module; `validate:` is a parameter on copy/template/etc.
> - `debugger` → not a module; the debugger keyword is a play-level setting
>
> Those should be on the v2 list (community-tier ports), not v1.

---

## A. Command execution

### command
- **Purpose**: Execute a command on the target without invoking a shell.
- **Required params**: one of `cmd` (string), `argv` (list of strings), or the free-form invocation `command: ls /tmp`.
- **Key optional params**:
  - `chdir` (path) — cd to this dir first
  - `creates` (path) — skip if file exists
  - `removes` (path) — skip if file does not exist
  - `stdin` (string) — feed to stdin
  - `stdin_add_newline` (bool, default `true`)
  - `strip_empty_ends` (bool, default `true`) — trim blank lines off stdout/stderr
  - `expand_argument_vars` (bool, default `true`) — Python-side `$VAR` expansion
- **Returns**: `cmd` (list), `rc`, `stdout`, `stdout_lines`, `stderr`, `stderr_lines`, `start`, `end`, `delta`.
- **Idempotent**: No, unless `creates`/`removes` are used as guards.
- **Check-mode supported**: Partial — only when `creates` or `removes` is set; otherwise the task is skipped in check mode.
- **Notes**: No shell, so `*`, `<`, `>`, `|`, `&`, `;` are NOT interpreted. For pipes/redirection use `shell`. `argv:` is the safer form because it bypasses any quote/space hazards. The deprecated `executable` parameter was removed in 2.4 — for non-`/bin/sh` interpreters use `shell` with `executable:`.

### shell
- **Purpose**: Execute a command through `/bin/sh` (or another shell) on the target.
- **Required params**: one of `cmd` (string) or the free-form `shell: cat /tmp/foo | grep bar`.
- **Key optional params**:
  - `chdir` (path)
  - `creates` (path) — skip if file exists
  - `removes` (path) — skip if file does not exist
  - `executable` (path) — alternate shell, e.g. `/bin/bash`
  - `stdin` (string)
  - `stdin_add_newline` (bool, default `true`)
- **Returns**: `cmd`, `rc`, `stdout`, `stdout_lines`, `stderr`, `stderr_lines`, `start`, `end`, `delta`.
- **Idempotent**: No — same as `command`.
- **Check-mode supported**: Partial (with `creates`/`removes`).
- **Notes**: The doc's official line: "If you want to execute a command securely and predictably, it may be better to use the `ansible.builtin.command` module instead." Use the `quote` filter (`{{ var | quote }}`) on any templated input to prevent injection. Shell vs. command: shell interprets metacharacters; command does not. Shell also gets you environment variable expansion via the shell, not Python. For multi-line scripts prefer `script:` (transfer + execute) or `template:` + `script:`.

### raw
- **Purpose**: Execute a command directly via the connection plugin, bypassing the module subsystem entirely.
- **Required params**: free-form command string (`raw: dnf install -y python3`).
- **Key optional params**:
  - `executable` (string) — abs path to the shell
- **Returns**: `stdout`, `stderr`, `rc` (when available).
- **Idempotent**: No (and there is no `creates`/`removes` to fake it).
- **Check-mode supported**: None — cannot predict effects without running.
- **Notes**: The only module that does not require Python on the managed node, which makes it the canonical bootstrap module ("install Python so we can run other modules"). Also useful for network devices that don't have Python at all. Disable fact gathering (`gather_facts: false`) when bootstrapping or `setup` will fail before raw can install Python. The `environment:` keyword needs either an explicit `executable` or `become:` to take effect. Use the `quote` filter to template safely.

### script
- **Purpose**: Copy a local script file to the managed node and execute it.
- **Required params**: one of `cmd` (string) or the free-form `script: ./bin/setup.sh --foo`.
- **Key optional params**:
  - `chdir` (string)
  - `creates` (string) — skip if remote file exists
  - `removes` (string) — skip if remote file does not exist
  - `executable` (string) — interpreter to invoke the script with
  - `decrypt` (bool, default `true`) — auto-decrypt vault-encrypted source
- **Returns**: `stdout`, `stderr`, `rc`.
- **Idempotent**: No (with `creates`/`removes` workaround).
- **Check-mode supported**: Partial.
- **Notes**: Like `raw`, does NOT require Python on the remote (the script runs in whatever interpreter you specify or the default shell). SSH connections force `-tt`, which merges stderr into stdout. Quote paths that contain spaces. The doc's recommendation: convert one-shot scripts into proper modules when they're going to live longer than a sprint.

### expect
- **Purpose**: Run a command and respond to interactive prompts.
- **Required params**: `command` (string), `responses` (dict — regex pattern → answer string or list of strings).
- **Key optional params**:
  - `chdir` (path)
  - `creates` (path)
  - `removes` (path)
  - `echo` (bool, default `false`) — echo response strings
  - `timeout` (int, default 30) — seconds to wait for each pattern; null disables
- **Returns**: `stdout`, `stderr`, `rc`.
- **Idempotent**: No.
- **Check-mode supported**: None.
- **Notes**: Requires `pexpect >= 3.3` on the managed node. Case-insensitive regex via `(?i)` prefix. The pexpect search window is 2000 bytes and does not span newlines — long prompts can fail to match. If you need a real shell you must specify it in the command string. Largely superseded by `community.general` modules for its specific use cases (interactive package install prompts, etc.).

---

## B. Files & content

### copy
- **Purpose**: Copy a file (or directory tree) from controller to managed node, with permissions/ownership/SELinux management.
- **Required params**: `dest` (path), plus one of `src` (path) or `content` (string).
- **Key optional params**:
  - `src` (path)
  - `content` (string) — inline content; mutually exclusive with `src`
  - `owner` (string), `group` (string), `mode` (any: octal `'0644'` / symbolic `u+rwx` / `preserve`)
  - `directory_mode` (any) — only applies to newly created dirs; existing dirs untouched
  - `backup` (bool, default `false`) — timestamped backup of existing dest
  - `force` (bool, default `true`) — overwrite if differs
  - `validate` (string) — command with `%s` placeholder; runs against staged temp file
  - `remote_src` (bool, default `false`) — copy from a path *on the managed node*
  - `checksum` (string) — SHA1 to verify
  - `decrypt` (bool, default `true`) — decrypt vaulted source
  - `follow` (bool, default `false`) — follow symlinks at dest
  - `local_follow` (bool) — follow symlinks at src
  - `unsafe_writes` (bool, default `false`)
  - `attributes` (string) — chattr flags
  - `seuser`, `serole`, `setype`, `selevel` — SELinux context
- **Returns**: `dest`, `src`, `checksum`, `md5sum`, `backup_file`, `state`, `mode`, `owner`, `group`, `uid`, `gid`, `size`.
- **Idempotent**: Yes — checksum compared, file rewritten only if it differs (or `force: false` and dest absent).
- **Check-mode supported**: Full. Diff supported full.
- **Notes**: For variable interpolation use `template:` instead. For copying *from* the managed node back to the controller use `fetch:`. The recursive copy facility "does not scale efficiently for hundreds of files or greater" — for big trees, archive on controller, transfer once with copy, then `unarchive` on the remote (or use `synchronize` from community.general). `safe_file_operations: full` — uses atomic write + chmod.

### file
- **Purpose**: Set state, ownership, mode, and SELinux context of a filesystem object; create directories, symlinks, hardlinks, or empty files; delete objects.
- **Required params**: `path` (string; aliases `dest`, `name`).
- **Key optional params**:
  - `state` (string, default `file`) — one of `absent`, `directory`, `file`, `hard`, `link`, `touch`
  - `owner`, `group`, `mode`
  - `recurse` (bool, default `false`) — apply ownership/mode recursively under a directory
  - `src` (path) — link target, required for `state=link` / `state=hard`
  - `follow` (bool, default `true`) — follow symlinks when applying changes
  - `force` (bool, default `false`) — replace existing objects to make a link
  - `attributes` (string) — chattr flags
  - `access_time` / `modification_time` (string: `preserve`, `now`, or `YYYYMMDDHHMM.SS`)
  - `access_time_format` / `modification_time_format` (string, default `%Y%m%d%H%M.%S`)
  - `seuser`, `serole`, `setype`, `selevel`
  - `unsafe_writes` (bool, default `false`)
- **Returns**: `path` or `dest` (depending on state).
- **Idempotent**: Yes for every state. `state=touch` is the one wrinkle — it always reports `changed: true` unless you fix the times explicitly via `access_time: preserve`/`modification_time: preserve`.
- **Check-mode supported**: Full. Diff support is partial — permission/ownership shown, contents on absent/touch omitted.
- **Notes**: `state=file` will *not* create a missing file; use `state=touch` (creates empty) or `copy:`/`template:` (creates with content). `state=absent` recursively removes directories — be careful. The doc explicitly distinguishes the six state values; runsible needs exactly the same six.

### template
- **Purpose**: Render a Jinja2 template on the controller and write the result to the managed node.
- **Required params**: `src` (path), `dest` (path).
- **Key optional params**:
  - `owner`, `group`, `mode`
  - `backup` (bool, default `false`)
  - `validate` (string) — `%s` placeholder; e.g. `nginx -t -c %s`
  - `newline_sequence` (string, default `\n`) — `\n`/`\r`/`\r\n`
  - `block_start_string` (default `{%`), `block_end_string` (default `%}`)
  - `variable_start_string` (default `{{`), `variable_end_string` (default `}}`)
  - `comment_start_string`, `comment_end_string` (since 2.12)
  - `trim_blocks` (bool, default `true`), `lstrip_blocks` (bool, default `false`)
  - `force` (bool, default `true`)
  - `follow` (bool, default `false`)
  - `output_encoding` (string, default `utf-8`) — since 2.7
  - `attributes` (string)
  - `seuser`, `serole`, `setype`, `selevel`
  - `unsafe_writes` (bool, default `false`)
- **Returns**: `checksum`, `dest`, `gid`, `group`, `md5sum`, `mode`, `owner`, `size`, `src`, `uid`.
- **Idempotent**: Yes — render then checksum-compare.
- **Check-mode supported**: Full. Diff full.
- **Notes**: Implicit template variables provided by Ansible: `ansible_managed`, `template_host`, `template_uid`, `template_path`, `template_fullpath`, `template_run_date`. `validate:` runs against a staged temp file before promoting it — use this for nginx/sshd/etc. so a bad template never lands. Source must be UTF-8. The non-default Jinja2 delimiter overrides exist specifically for templates that themselves contain `{{ }}` (e.g. ERB or Mustache files).

### fetch
- **Purpose**: Copy a file *from* the managed node *to* the controller.
- **Required params**: `src` (string) — file on remote, must be a single file (no recursion); `dest` (string) — local directory.
- **Key optional params**:
  - `flat` (bool, default `false`) — when true, dest treated as a path; `dest: /tmp/foo` writes the file there directly with no per-host nesting
  - `fail_on_missing` (bool, default `true`)
  - `validate_checksum` (bool, default `true`)
- **Returns**: standard file return fields.
- **Idempotent**: Yes — checksum compared.
- **Check-mode supported**: Full. Diff full.
- **Notes**: Default destination layout is `<dest>/<inventory_hostname>/<src_path>` so per-host files don't collide. With `become: true`, fetch may double the transfer size in memory — caution on large files. There is no recursive fetch; use `find` + `fetch` in a loop, or archive-on-remote then fetch the archive.

### slurp
- **Purpose**: Read a file from the managed node and return it base64-encoded (so binary content survives JSON transport).
- **Required params**: `src` (path; alias `path`).
- **Key optional params**: none.
- **Returns**: `content` (base64 string), `encoding` (`"base64"`), `source` (path).
- **Idempotent**: Yes (read-only).
- **Check-mode supported**: Full. Diff none.
- **Notes**: Uses approximately 2x the file size in RAM during transport (the encoded copy). For large files prefer `fetch:`. Decode on the controller with `b64decode` filter (or for text, `(result.content | b64decode)`).

### stat
- **Purpose**: Return facts about a filesystem object (size, mode, owner, mtime, checksum, MIME type, etc.).
- **Required params**: `path` (path).
- **Key optional params**:
  - `follow` (bool, default `false`) — dereference symlinks
  - `get_attributes` (bool, default `true`) — query lsattr if available
  - `get_checksum` (bool, default `true`)
  - `get_mime` (bool, default `true`)
  - `get_selinux_context` (bool, default `false`) — since 2.20
  - `checksum_algorithm` (string, default `sha1`) — `md5`/`sha1`/`sha224`/`sha256`/`sha384`/`sha512`
- **Returns**: `stat` dict with `exists`, `isdir`, `isreg`, `islnk`, `size`, `mode`, `uid`/`gid`, `pw_name`/`gr_name`, `mtime`/`atime`/`ctime`, `checksum`, `readable`/`writeable`/`executable`, plus many more.
- **Idempotent**: Yes (read-only).
- **Check-mode supported**: Full. Diff none.
- **Notes**: The standard "does this exist?" probe pattern is `stat:` + `when: result.stat.exists`. Run it before `file:` / `copy:` if your control flow needs to differ on existence. Setting `get_checksum: false` significantly speeds up scans on large files.

### lineinfile
- **Purpose**: Ensure a single line exists (or doesn't) in a text file, optionally matching by regex or literal string.
- **Required params**: `path` (string; aliases `dest`/`destfile`/`name`).
- **Key optional params**:
  - `regexp` (string, alias `regex`)
  - `search_string` (string) — literal, mutually exclusive with `regexp`/`backrefs`
  - `line` (string, alias `value`) — required for `state=present`
  - `state` (string, default `present`) — `present`/`absent`
  - `insertafter` (string, default `EOF`) — regex or `EOF`
  - `insertbefore` (string) — regex or `BOF`
  - `create` (bool, default `false`) — create file if missing
  - `backup` (bool, default `false`)
  - `backrefs` (bool, default `false`) — enable `\1`/`\g<1>` in `line`; mutually exclusive with `search_string`
  - `firstmatch` (bool, default `false`) — first vs. last match for insertafter/insertbefore
  - `validate` (string) — `%s` placeholder
  - `mode`, `owner`, `group`, `attributes`
  - `encoding` (string, default `utf-8`) — since 2.20
  - SELinux context fields
  - `unsafe_writes` (bool, default `false`)
- **Returns**: `backup` (filename), `changed`, `msg`.
- **Idempotent**: Yes when the regexp matches both the pre-state and post-state. Common bug: the regex matches your replacement, so the second run also "matches" — the result is correct but `changed=false`. The opposite bug: regex doesn't match the replacement, so each run rewrites the line and reports `changed: true` forever — fix by widening the regex or using `search_string`.
- **Check-mode supported**: Full. Diff full.
- **Notes**: Only the *last* matching line is replaced (or *first* if `firstmatch: true`). With `backrefs: yes`, the file is left unchanged if regex finds no match (silent no-op). For multi-line edits use `blockinfile`; for global pattern replacement use `replace`. Vault auto-decryption is **not** supported on the file (you'd need to use template/copy from a vaulted source).

### blockinfile
- **Purpose**: Insert/update/remove a multi-line block, bracketed by marker lines, in a text file.
- **Required params**: `path` (path).
- **Key optional params**:
  - `block` (string, default `""`) — the contents; empty string removes the block when state=present
  - `marker` (string, default `# {mark} ANSIBLE MANAGED BLOCK`)
  - `marker_begin` (string, default `BEGIN`) — substituted for `{mark}` on the opening line
  - `marker_end` (string, default `END`) — substituted for `{mark}` on the closing line
  - `state` (string, default `present`)
  - `insertafter` (string) — regex or `EOF`
  - `insertbefore` (string) — regex or `BOF`
  - `create` (bool, default `false`)
  - `backup` (bool, default `false`)
  - `validate` (string)
  - `append_newline` (bool, default `false`) — since 2.16
  - `prepend_newline` (bool, default `false`) — since 2.16
  - `encoding` (string, default `utf-8`) — since 2.20
  - `mode`, `owner`, `group`, `attributes`, SELinux fields
  - `unsafe_writes` (bool, default `false`)
- **Returns**: `changed`, `msg`, `backup` (filename).
- **Idempotent**: Yes — markers make detection unambiguous.
- **Check-mode supported**: Full. Diff full.
- **Notes**: Critical pitfall: when calling blockinfile inside a loop, supply a unique `marker` per iteration or every iteration will overwrite the previous block. Avoid multi-line markers; the parser can re-insert the block forever. The default comment-style markers work for shell/conf/yaml — but use `marker: "// {mark} ANSIBLE MANAGED BLOCK"` for C-style files, etc.

### replace
- **Purpose**: Substitute every occurrence of a regex in a file.
- **Required params**: `path` (string; aliases `dest`/`destfile`/`name`), `regexp` (string).
- **Key optional params**:
  - `replace` (string, default `""`) — supports `\1`/`\g<1>` backrefs
  - `after` (string) — only edit content after this regex (DOTALL)
  - `before` (string) — only edit content before this regex (DOTALL)
  - `encoding` (string, default `utf-8`)
  - `backup` (bool, default `false`)
  - `validate` (string)
  - `mode`, `owner`, `group`, `attributes`, SELinux fields
  - `unsafe_writes` (bool, default `false`)
- **Returns**: `changed`, `msg`, `backup` (filename).
- **Idempotent**: Conditional — must not match its own output. The doc explicitly warns: "Users must maintain idempotence by ensuring replacement patterns don't match their own output."
- **Check-mode supported**: Full. Diff full.
- **Notes**: Uses MULTILINE (so `^` and `$` are line-anchored). `before` and `after` use DOTALL. Combined `before` + `after` works correctly only since 2.7.10. Short-form task syntax requires escaping backslashes; long-form does not. For per-line edits use `lineinfile`.

### find
- **Purpose**: Walk one or more directories and return matching files based on size/age/pattern/contents.
- **Required params**: `paths` (list of paths; alias `name`/`path`).
- **Key optional params**:
  - `patterns` (list, default `*`) — shell-glob or regex (when `use_regex: true`); applied to basenames
  - `excludes` (list)
  - `file_type` (string, default `file`) — `file`/`directory`/`link`/`any`
  - `age` (string, e.g. `2d`, `4w`, prefix `-` for "less than")
  - `age_stamp` (string, default `mtime`) — `atime`/`ctime`/`mtime`
  - `size` (string, e.g. `10m`, `-1g`) — suffixes b/k/m/g/t
  - `contains` (string) — regex matched against file contents
  - `read_whole_file` (bool, default `false`) — relevant when `contains` is multi-line
  - `recurse` (bool, default `false`)
  - `depth` (int) — max recursion depth (unlimited if unset)
  - `hidden` (bool, default `false`)
  - `follow` (bool, default `false`)
  - `use_regex` (bool, default `false`)
  - `get_checksum` (bool, default `false`)
  - `checksum_algorithm` (string, default `sha1`)
  - `mode` (any) — match exact or minimum permissions (paired with `exact_mode`)
  - `exact_mode` (bool, default `true`)
  - `encoding` (string)
  - `limit` (int) — stop after N matches
- **Returns**: `files` (list of stat-like dicts), `matched` (int), `examined` (int), `skipped_paths` (dict).
- **Idempotent**: Yes (read-only).
- **Check-mode supported**: Full. Diff none.
- **Notes**: This is the Pythonic `find`, not a wrapper around the `find(1)` binary; complex predicates beyond what's listed will need either a `command:` to system find or a custom module. Multiple criteria are AND'd together. Pair with a loop over `result.files` for downstream actions (delete-old-logs, archive-and-fetch, etc.).

### unarchive
- **Purpose**: Extract a `.tar`/`.tar.gz`/`.tar.bz2`/`.tar.xz`/`.tar.zst`/`.zip` archive on the managed node, optionally copying it from controller first.
- **Required params**: `src` (path or URL), `dest` (path; must already exist).
- **Key optional params**:
  - `remote_src` (bool, default `false`) — true = src is on managed node or a URL; false = src is on controller
  - `copy` (bool, default `true`) — deprecated, prefer `remote_src`
  - `creates` (path) — skip if this exists under dest
  - `list_files` (bool, default `false`) — populate `files` return value
  - `exclude` (list) — basenames/dirs to skip
  - `include` (list, since 2.11) — only these
  - `keep_newer` (bool, default `false`)
  - `extra_opts` (list of strings) — passed verbatim to the underlying tool
  - `owner`, `group`, `mode`, `attributes`
  - `decrypt` (bool, default `true`)
  - `validate_certs` (bool, default `true`) — for URL src
  - `unsafe_writes` (bool, default `false`)
  - `io_buffer_size` (int, default 65536) — since 2.12
  - SELinux context fields
- **Returns**: `dest`, `src`, `state`, `owner`, `group`, `uid`, `gid`, `mode`, `size`, `handler` (e.g. `TgzArchive`), `files` (when `list_files: true`).
- **Idempotent**: Yes when `creates` is set; otherwise it always re-extracts.
- **Check-mode supported**: Partial — full except for gzipped tar. Diff partial — uses `gtar --diff` when available.
- **Notes**: Requires `gtar` (or `tar`), `unzip`, `zstd` etc. on the managed node (not BusyBox tar — gnu tar is required for diff and some flag combinations). Does NOT decompress standalone `.gz` / `.bz2` / `.xz` / `.zst` files (those need `community.general` modules or a `command:` to gunzip). For URL sources, the doc recommends using `get_url:` or `uri:` first if you need real checksum validation. There is **no `archive` module** in `ansible.builtin` (it lives in `community.general`).

### assemble
- **Purpose**: Concatenate file fragments from a source directory into a single destination file (in lexical order).
- **Required params**: `src` (path), `dest` (path).
- **Key optional params**:
  - `delimiter` (string) — inserted between fragments
  - `regexp` (string) — only fragments whose name matches are included
  - `ignore_hidden` (bool, default `false`) — skip dotfiles
  - `backup` (bool, default `false`)
  - `validate` (string)
  - `remote_src` (bool, default `true`) — src is on managed node by default
  - `owner`, `group`, `mode`, `attributes`
  - `decrypt` (bool, default `true`)
  - `unsafe_writes` (bool, default `false`)
  - SELinux context fields
- **Returns**: standard file return fields.
- **Idempotent**: Yes — checksum-compared like copy.
- **Check-mode supported**: Full. Diff full.
- **Notes**: The classic "snippet directory → single config file" pattern (`/etc/ssh/conf.d/*` → `/etc/ssh/sshd_config`). Notably defaults `remote_src: true`, opposite of copy/unarchive. Use `validate:` to run `sshd -t -f %s` etc. before promoting.

### tempfile
- **Purpose**: Create a temporary file or directory with optional prefix/suffix.
- **Required params**: none.
- **Key optional params**:
  - `state` (string, default `file`) — `file` or `directory`
  - `path` (string) — parent directory (defaults to system temp)
  - `prefix` (string, default `ansible.`)
  - `suffix` (string, default `""`)
- **Returns**: `path` (the created path).
- **Idempotent**: No — creates a new tempfile every run.
- **Check-mode supported**: None.
- **Notes**: Permissions are owner-only by default; use `file:` to broaden them. Useful as a safe staging area for compose-then-validate-then-rename patterns.

---

## C. Network downloads & probes

### get_url
- **Purpose**: Download a file from HTTP/HTTPS/FTP to the managed node.
- **Required params**: `url` (string), `dest` (path).
- **Key optional params**:
  - `force` (bool, default `false`) — re-download even if dest exists
  - `checksum` (string, format `<algo>:<value>` or `<algo>:<url>`) — verify after download; skip if matches
  - `headers` (dict)
  - `http_agent` (string, default `ansible-httpget`)
  - `url_username` / `url_password` — basic auth
  - `force_basic_auth` (bool, default `false`)
  - `use_proxy` (bool, default `true`)
  - `validate_certs` (bool, default `true`)
  - `use_gssapi` (bool, default `false`) — Kerberos
  - `use_netrc` (bool, default `true`)
  - `client_cert` (path), `client_key` (path)
  - `ciphers` (list) — OpenSSL spec
  - `decompress` (bool, default `true`)
  - `tmp_dest` (path)
  - `timeout` (int, default 10)
  - `owner`, `group`, `mode`, `attributes`, `backup`
  - `unsafe_writes` (bool, default `false`)
  - `unredirected_headers` (list, default `[]`)
  - SELinux context fields
- **Returns**: `url`, `dest`, `src`, `checksum_src`, `checksum_dest`, `md5sum`, `backup_file`, `msg`, `status_code`, `elapsed`, `size`, plus standard file fields.
- **Idempotent**: Yes when `checksum:` is supplied; otherwise yes-ish (skip if dest exists and `force: false`).
- **Check-mode supported**: Partial — does a HEAD to validate URL.
- **Notes**: Set `checksum:` for any production download — without it you trust the upstream URL to be unchanged forever. `decompress: true` will silently decompress gzip-encoded responses, which can be surprising; turn off if you want the raw bytes. Proxies obey `http_proxy`/`https_proxy` env vars unless `use_proxy: false`.

### uri
- **Purpose**: Make an arbitrary HTTP/HTTPS request and return the response (or save it to disk).
- **Required params**: `url` (string).
- **Key optional params**:
  - `method` (string, default `GET`)
  - `body` (any)
  - `body_format` (string, default `raw`) — `raw`/`json`/`form-urlencoded`/`form-multipart`
  - `headers` (dict, default `{}`)
  - `status_code` (list, default `[200]`)
  - `return_content` (bool, default `false`)
  - `validate_certs` (bool, default `true`)
  - `force_basic_auth` (bool, default `false`)
  - `follow_redirects` (string, default `safe`) — `all`/`safe`/`none`/`urllib2`
  - `timeout` (int, default 30)
  - `url_username`, `url_password`
  - `dest` (path) — save body to file
  - `src` (path) — submit a file (mutually exclusive with `body`)
  - `remote_src` (bool, default `false`)
  - `creates` (path), `removes` (path)
  - `http_agent` (string, default `ansible-httpget`)
  - `use_proxy` (bool, default `true`)
  - `use_gssapi` (bool, default `false`)
  - `use_netrc` (bool, default `true`)
  - `ca_path` (path), `client_cert` (path), `client_key` (path), `ciphers` (list)
  - `decompress` (bool, default `true`)
  - `unix_socket` (path)
  - `unredirected_headers` (list, default `[]`)
  - `unsafe_writes` (bool, default `false`)
  - `force` (bool, default `false`)
  - `owner`, `group`, `mode`, `attributes`, SELinux fields
- **Returns**: `status` (int), `content` (string when `return_content: true` or status mismatched), `json` (dict when content-type is JSON), `msg`, `url`, `elapsed`, `cookies` (dict), `cookies_string` (string), `redirected` (bool), `path` (when `dest` is set).
- **Idempotent**: No (it's an HTTP call) — use `creates`/`removes` and/or your status_code allowlist.
- **Check-mode supported**: None.
- **Notes**: The "talk to a JSON API" module. Pair with `register:` and `until:` for poll loops. Status_code defaults to `[200]` only — if the API also returns 201 / 204 / 409 normally, you must add them or the task fails. JSON response body is auto-parsed into the `json` return when `Content-Type: application/json`. For Windows, use `ansible.windows.win_uri`.

### wait_for
- **Purpose**: Block until a TCP port opens/closes, a file appears/disappears, a regex appears in a file, or a socket goes idle.
- **Required params**: typically one of `port` (int), `path` (path), or just a `timeout` for a fixed sleep.
- **Key optional params**:
  - `host` (string, default `127.0.0.1`)
  - `state` (string, default `started`) — `absent`/`drained`/`present`/`started`/`stopped`
  - `delay` (int, default 0) — pre-wait
  - `timeout` (int, default 300)
  - `sleep` (int, default 1) — poll interval
  - `search_regex` (string)
  - `exclude_hosts` (list) — for connection draining
  - `connect_timeout` (int, default 5)
  - `msg` (string) — custom failure message
  - `active_connection_states` (list, default `[ESTABLISHED, FIN_WAIT1, FIN_WAIT2, SYN_RECV, SYN_SENT, TIME_WAIT]`)
- **Returns**: `elapsed` (int), `match_groupdict` (dict, when search_regex matched), `match_groups` (list).
- **Idempotent**: Yes (it's a probe — no side effects).
- **Check-mode supported**: None.
- **Notes**: When tailing log files for `search_regex`, beware self-match — Ansible's own log line may contain the pattern; obfuscate it (`'this t\[h\]ing'`). For SELinux/AppArmor systems, paths can appear absent due to access controls. State `drained` waits for all connections in `active_connection_states` to drop below the threshold — useful before yanking a node out of LB.

### wait_for_connection
- **Purpose**: Wait until Ansible itself can connect to the host (post-reboot, post-network-restart, etc.).
- **Required params**: none.
- **Key optional params**:
  - `delay` (int, default 0)
  - `sleep` (int, default 1)
  - `timeout` (int, default 600)
  - `connect_timeout` (int, default 5)
- **Returns**: `elapsed` (float).
- **Idempotent**: Yes.
- **Check-mode supported**: None.
- **Notes**: Uses the same connection plugin and ping module Ansible uses for everything else, so it actually validates "I can run a real task," not just "TCP is up." This is the right hammer for "I just rebooted, wait for me to be able to push more changes."

---

## D. Package management

### package
- **Purpose**: OS-agnostic dispatcher that picks the right package manager (apt/dnf/zypper/pacman/etc.) based on facts and forwards.
- **Required params**: `name` (string or list), `state` (string).
- **Key optional params**:
  - `use` (string, default `auto`) — force a specific backend; can also be set via the `ansible_package_use` host var since 2.17
- **Returns**: depends on the underlying module.
- **Idempotent**: Yes (delegates).
- **Check-mode supported**: Conditional — depends on backend.
- **Notes**: Gives you portable plays at the cost of cross-distro abstraction leakage (package names differ per OS, so you usually still need `vars: nginx_pkg: nginx | apache2 | ...` keyed off `ansible_facts.os_family`). `latest` is only supported when the underlying manager supports it. For Windows, `ansible.windows.win_package`.

### apt
- **Purpose**: Manage Debian/Ubuntu packages via apt.
- **Required params**: typically `name` (string or list) plus a `state`, OR `update_cache: true` for a cache-only run, OR `upgrade:` for a system upgrade.
- **Key optional params**:
  - `state` (string, default `present`) — `absent`/`build-dep`/`latest`/`present`/`fixed`
  - `update_cache` (bool, default `false`)
  - `cache_valid_time` (int, default 0) — seconds; skip update_cache if cache is younger
  - `upgrade` (string, default `no`) — `no`/`yes`/`safe`/`full`/`dist`
  - `force_apt_get` (bool, default `false`) — bypass aptitude detection
  - `install_recommends` (bool) — defaults to apt's own default
  - `allow_downgrade` (bool, default `false`)
  - `allow_unauthenticated` (bool, default `false`)
  - `allow_change_held_packages` (bool, default `false`)
  - `only_upgrade` (bool, default `false`) — install only if already present
  - `autoclean` (bool, default `false`), `autoremove` (bool, default `false`)
  - `purge` (bool, default `false`)
  - `clean` (bool, default `false`)
  - `deb` (path) — install a local or remote .deb
  - `default_release` (string) — `-t` flag
  - `dpkg_options` (string, default `force-confdef,force-confold`)
  - `fail_on_autoremove` (bool, default `false`)
  - `force` (bool, default `false`) — disables sig/cert verification
  - `lock_timeout` (int, default 60)
  - `policy_rc_d` (int)
  - `update_cache_retries` (int, default 5), `update_cache_retry_max_delay` (int, default 12)
  - `auto_install_module_deps` (bool, default `true`) — install python3-apt automatically
- **Returns**: `stdout`, `stderr`, `cache_updated`, `cache_update_time`.
- **Idempotent**: Yes — checks installed state before acting. `state: latest` is idempotent only if no upstream update lands between runs.
- **Check-mode supported**: Full. Diff full.
- **Notes**: Pass `name:` as a list for batched operations; loops are slower because each iteration re-runs apt. Newly installed services typically auto-start on Debian/Ubuntu — write `policy-rc.d` if you need to suppress that. The `aptitude` requirement was dropped in 2.4. `update_cache: true` + `cache_valid_time: 3600` is the recommended idiom (refresh every hour at most).

### yum
- **Purpose**: Redirect to `ansible.builtin.dnf` (yum binary fronts dnf on modern RHEL).
- **Notes**: For runsible's purposes, treat `yum` as an alias of `dnf` when the target system is RHEL ≥ 8.

### dnf
- **Purpose**: Manage RHEL/Fedora packages via dnf.
- **Required params**: `name` (string or list).
- **Key optional params**:
  - `state` (string) — `absent`/`present`/`installed`/`removed`/`latest` (defaults to `present` unless `autoremove: true`)
  - `list` (string) — non-idempotent query mode (deprecated; use `package_facts`)
  - `autoremove` (bool, default `false`)
  - `bugfix` (bool, default `false`) — with `state=latest`, only bugfix advisories
  - `security` (bool, default `false`) — with `state=latest`, only security advisories
  - `disable_excludes` (string) — `all`/`main`/`<repoid>`
  - `disable_gpg_check` (bool, default `false`)
  - `exclude` (list)
  - `install_weak_deps` (bool, default `true`)
  - `allowerasing` (bool, default `false`)
  - `update_only` (bool, default `false`)
  - `update_cache` (bool, default `false`; alias `expire-cache`)
  - `download_only` (bool, default `false`), `download_dir` (string)
  - `skip_broken` (bool, default `false`)
  - `allow_downgrade` (bool, default `false`)
  - `best` (bool), `nobest` (bool) — defaults follow distro
  - `cacheonly` (bool, default `false`)
  - `conf_file` (string)
  - `disable_plugin` (list), `enable_plugin` (list)
  - `disablerepo` (list), `enablerepo` (list)
  - `installroot` (string, default `/`)
  - `releasever` (string)
  - `lock_timeout` (int, default 30)
  - `sslverify` (bool, default `true`)
  - `validate_certs` (bool, default `true`)
  - `use_backend` (string, default `auto`) — `auto`/`dnf`/`dnf4`/`dnf5`/`yum`/`yum4`
- **Returns**: depends; `rc`, `msg`, `results` (list of strings) typical.
- **Idempotent**: Yes for `present`/`absent`; `latest` follows whatever's upstream.
- **Check-mode supported**: Full. Diff full.
- **Notes**: YUM backend dropped in ansible-core 2.17. Group removal can fail if the group was originally installed by Ansible. Pass `name:` as a list rather than looping. `releasever:` lets you target a different RHEL minor without changing the host.

### dnf5
- **Purpose**: Manage packages via libdnf5 (dnf5 — successor to dnf4).
- **Required params**: `name` (string or list).
- **Key optional params**: largely the same as `dnf` plus:
  - `auto_install_module_deps` (bool, default `true`) — auto-install python3-libdnf5 (since 2.19)
- **Returns**: `rc` (int), `msg` (string), `results` (list of strings), `failures` (list).
- **Idempotent**: Yes.
- **Check-mode supported**: Full. Diff full.
- **Notes**: Still under active development; not full feature parity with `dnf`. `lock_timeout` is currently a no-op (dnf5 lacks the option). `disable_plugin`/`enable_plugin` need libdnf5 ≥ 5.2.0.0. Targeted at Fedora 39+, RHEL 10+.

### pip
- **Purpose**: Manage Python packages via pip (in a virtualenv or system-wide).
- **Required params**: one of `name` (list) or `requirements` (path).
- **Key optional params**:
  - `version` (string)
  - `virtualenv` (path) — created if missing; mutually exclusive with `executable`
  - `virtualenv_command` (path, default `virtualenv`)
  - `virtualenv_python` (string)
  - `virtualenv_site_packages` (bool, default `false`)
  - `state` (string, default `present`) — `present`/`absent`/`latest`/`forcereinstall`
  - `extra_args` (string)
  - `editable` (bool, default `false`)
  - `executable` (path) — alternate pip binary
  - `chdir` (path)
  - `umask` (string) — octal
  - `break_system_packages` (bool, default `false`) — since 2.17, for PEP 668 systems
- **Returns**: `cmd`, `name`, `requirements`, `version`, `virtualenv`.
- **Idempotent**: Yes for `present`/`absent`; `latest` depends on upstream.
- **Check-mode supported**: Full. Diff none.
- **Notes**: On PEP 668 systems (Debian 12+, Ubuntu 24.04+, recent Fedora), system pip refuses to install. Either set `break_system_packages: true` (with pip ≥23.0.1) or — preferred — use `virtualenv:`. `name:` accepts URLs and VCS specs (`git+https://...`, `bzr+...`, `hg+...`, `svn+...`). The Ansible Python interpreter on the controller must have setuptools regardless.

### dpkg_selections
- **Purpose**: Set apt selection state (install/hold/deinstall/purge) on a Debian package.
- **Required params**: `name` (string), `selection` (string).
- **Key optional params**: none.
- **Returns**: standard.
- **Idempotent**: Yes.
- **Check-mode supported**: Full. Diff full.
- **Notes**: This module sets selection state; it does NOT install/remove. Use `apt:` for the actual operation. Most common use is `selection: hold` to pin a package version.

### debconf
- **Purpose**: Pre-seed Debian package configuration via debconf.
- **Required params**: `name` (string).
- **Key optional params**:
  - `question` (string)
  - `value` (any) — list for multiselect since 2.17
  - `vtype` (string) — `boolean`/`error`/`multiselect`/`note`/`password`/`seen`/`select`/`string`/`text`/`title`
  - `unseen` (bool, default `false`)
- **Returns**: standard.
- **Idempotent**: Yes (sets values; safe to repeat).
- **Check-mode supported**: Full. Diff full.
- **Notes**: Updates the debconf database only — does NOT reconfigure the package. Run `dpkg-reconfigure -fnoninteractive <pkg>` separately. Always set `no_log: true` for password values. Discover questions with `debconf-show <pkg>`.

### apt_repository
- **Purpose**: Add or remove an apt source line in `/etc/apt/sources.list.d/`.
- **Required params**: `repo` (string).
- **Key optional params**:
  - `state` (string, default `present`)
  - `update_cache` (bool, default `true`)
  - `update_cache_retries` (int, default 5)
  - `update_cache_retry_max_delay` (int, default 12)
  - `filename` (string) — appends `.list`
  - `mode` (any)
  - `codename` (string)
  - `install_python_apt` (bool, default `true`)
  - `validate_certs` (bool, default `true`)
- **Returns**: `repo`, `sources_added` (list), `sources_removed` (list).
- **Idempotent**: Yes.
- **Check-mode supported**: Full. Diff full.
- **Notes**: Triggers `apt-get update` automatically by default. Requires `python3-apt` and `apt-key` or `gpg`. For deb822-format sources (modern Debian), use `deb822_repository` instead.

### deb822_repository
- **Purpose**: Manage `.sources` files in `/etc/apt/sources.list.d/` using the deb822 format.
- **Required params**: `name` (string).
- **Key optional params**:
  - `types` (list, default `["deb"]`)
  - `uris` (list)
  - `suites` (list)
  - `components` (list)
  - `signed_by` (string) — URL, file path, fingerprint, or armored key block
  - `state` (string, default `present`)
  - `architectures` (list)
  - `enabled` (bool)
  - `trusted` (bool)
  - `allow_insecure` (bool), `allow_weak` (bool), `allow_downgrade_to_insecure` (bool)
  - `by_hash` (bool)
  - `check_valid_until` (bool), `check_date` (bool), `date_max_future` (int)
  - `inrelease_path` (string)
  - `pdiffs` (bool)
  - `languages` (list)
  - `targets` (list)
  - `mode` (any, default `0644`)
  - `install_python_debian` (bool, default `false`)
- **Returns**: `dest`, `key_filename`, `repo`.
- **Idempotent**: Yes.
- **Check-mode supported**: Full. Diff full.
- **Notes**: Does NOT auto-update the apt cache — chain with `apt: update_cache=true`. Requires `python3-debian`. The modern replacement for `apt_key` + `apt_repository` (which use the deprecated apt-key tooling).

### apt_key
- **Purpose**: Add/remove keys from the legacy apt keyring.
- **Required params**: depends on action.
- **Key optional params**:
  - `id` (string) — required for `state: absent`; helps check-mode for present
  - `data` (string) — armored block
  - `file` (path) — local key file
  - `keyring` (path) — explicit `/etc/apt/trusted.gpg.d/<file>` target
  - `url` (string)
  - `keyserver` (string)
  - `state` (string, default `present`)
  - `validate_certs` (bool, default `true`)
- **Returns**: `id`, `key_id`, `short_id`, `fp`, `before` (list), `after` (list).
- **Idempotent**: Yes.
- **Check-mode supported**: Full. Diff none.
- **Notes**: **DEPRECATED** — `apt-key` is being removed from modern Debian. Use `deb822_repository` with `signed_by:` (or write keyring files via `copy:`) on Debian 12+ / Ubuntu 22.04+. Maintained for backward compatibility only.

### rpm_key
- **Purpose**: Import or remove a GPG key from the rpm database.
- **Required params**: `key` (string) — URL, file path, or already-imported keyid/fingerprint.
- **Key optional params**:
  - `state` (string, default `present`)
  - `fingerprint` (list of strings) — verify before importing (since 2.9)
  - `validate_certs` (bool, default `true`)
- **Returns**: standard.
- **Idempotent**: Yes (checks rpm database).
- **Check-mode supported**: Full. Diff none.
- **Notes**: For deletion, `key:` can be the keyid or fingerprint of an already-installed key. Pair with `yum_repository: gpgkey: ...` so installs verify against the imported key.

### yum_repository
- **Purpose**: Manage `.repo` files in `/etc/yum.repos.d/`.
- **Required params**: `name` (string) — becomes both `[name]` section and filename.
- **Key optional params** (truncated; the full set is huge):
  - `description` (string) — required when `state: present`
  - `state` (string, default `present`)
  - `baseurl` (list)
  - `mirrorlist` (string)
  - `metalink` (string)
  - `enabled` (bool, default `true`)
  - `gpgcheck` (bool), `gpgkey` (list), `repo_gpgcheck` (bool)
  - `file` (string) — filename without `.repo`
  - `reposdir` (path, default `/etc/yum.repos.d`)
  - `includepkgs` (list), `exclude` (list)
  - `cost` (string), `priority` (string)
  - `failovermethod` (string) — `roundrobin`/`priority`
  - `skip_if_unavailable` (bool)
  - `proxy` (string), `proxy_username`, `proxy_password`
  - `username` (string), `password` (string)
  - `sslverify` (bool), `sslcacert` (string), `sslclientcert` (string), `sslclientkey` (string)
  - `timeout` (string), `retries` (string)
  - `bandwidth` (string), `throttle` (string)
  - `metadata_expire` (string, default 6h), `metadata_expire_filter` (string)
  - `mirrorlist_expire` (string, default 6h)
  - `enablegroups` (bool)
  - `module_hotfixes` (bool)
  - `countme` (bool)
  - `include` (string)
  - `s3_enabled` (bool)
  - file attribute fields (`owner`, `group`, `mode`, `attributes`)
  - SELinux context fields
  - `unsafe_writes` (bool, default `false`)
  - DEPRECATED (removal in 2.22): `async`, `deltarpm_metadata_percentage`, `deltarpm_percentage`, `gpgcakey`, `http_caching`, `keepalive`, `protect`, `ssl_check_cert_permissions`, `ui_repoid_vars`, `mirrorlist_expire`
- **Returns**: `repo`, `state`.
- **Idempotent**: Yes.
- **Check-mode supported**: Full. Diff full.
- **Notes**: All comments in the existing `.repo` file are removed when modifying. When the last repo is removed, the file is deleted. Repository metadata cache may persist on disk after removal — `yum clean all` to flush.

---

## E. Service & init management

### service
- **Purpose**: Generic dispatcher for service control across init systems.
- **Required params**: `name` (string), plus at least one of `state` or `enabled`.
- **Key optional params**:
  - `state` (string) — `started`/`stopped`/`restarted`/`reloaded`
  - `enabled` (bool)
  - `arguments` (string, default `""`) — extra args appended to the service command
  - `pattern` (string) — substring searched in `ps` output when no native status is available
  - `runlevel` (string, default `default`) — OpenRC only
  - `sleep` (int) — seconds between stop and start during restart
  - `use` (string, default `auto`) — force a specific backend
- **Returns**: depends on backend.
- **Idempotent**: Yes for `started`/`stopped`/`enabled`; `restarted`/`reloaded` always changed.
- **Check-mode supported**: Conditional (depends on backend).
- **Notes**: Supports BSD init, OpenRC, SysV, Solaris SMF, systemd, upstart. AIX understands group subsystem names. `restarted` always reports change; `started` is the idempotent option. For Windows, `ansible.windows.win_service`.

### systemd
- **Purpose**: Redirect to `systemd_service` (kept for compatibility).
- **Notes**: For runsible, treat as alias.

### systemd_service
- **Purpose**: Manage systemd units (services, timers, sockets, etc.).
- **Required params**: typically `name` plus `state` and/or `enabled`/`masked`.
- **Key optional params**:
  - `name` (string; aliases `service`/`unit`) — include `.service` etc. in chroots
  - `state` (string) — `started`/`stopped`/`restarted`/`reloaded`
  - `enabled` (bool)
  - `masked` (bool)
  - `daemon_reload` (bool, default `false`) — runs `systemctl daemon-reload` first
  - `daemon_reexec` (bool, default `false`)
  - `scope` (string, default `system`) — `system`/`user`/`global`; user requires active dbus + XDG_RUNTIME_DIR
  - `no_block` (bool, default `false`) — async dispatch
  - `force` (bool) — overrides existing symlinks
- **Returns**: `status` (dict — full `systemctl show` output, often 100+ keys including `ActiveState`, `ExecStart`, `RestartUSec`, etc.).
- **Idempotent**: `started`/`stopped`/`enabled`/`disabled`/`masked`/`unmasked` are all idempotent. `restarted` always reports changed. `reloaded` always reloads (and starts if inactive).
- **Check-mode supported**: Full. Diff none.
- **Notes**: Sequence is enable/disable → mask/unmask → state change. Glob patterns in unit names not supported. `name:` need extension only in chroots; otherwise `crond` works fine for `crond.service`.

### sysvinit
- **Purpose**: Manage SysV init scripts (chkconfig/update-rc.d-style).
- **Required params**: `name` (string) plus at least one of state/enabled.
- **Key optional params**:
  - `state` (string)
  - `enabled` (bool)
  - `sleep` (int, default 1)
  - `pattern` (string)
  - `runlevels` (list)
  - `arguments` (string)
  - `daemonize` (bool, default `false`) — for misbehaving init scripts
- **Returns**: `name`, `status` (dict), `results` (list).
- **Idempotent**: Yes for state.
- **Check-mode supported**: Full. Diff none.
- **Notes**: Service names vary across distributions. Requires init scripts in `/etc/init.d/`. Largely legacy in 2026 — runsible should still ship it for older RHEL 6/7 / Ubuntu 14.04 LTS / embedded.

### service_facts
- **Purpose**: Gather state of all services into facts.
- **Required params**: none.
- **Key optional params**: none.
- **Returns**: `ansible_facts.services` (dict keyed by service name) with per-service `name`, `source` (systemd/sysv/upstart/openrc/rcctl/src), `state` (running/stopped/failed/unknown), `status` (enabled/disabled/static/indirect/unknown).
- **Idempotent**: Yes (read-only).
- **Check-mode supported**: Full. Diff none.
- **Notes**: Services with hyphens require bracket access: `ansible_facts.services['zuul-gateway']`. Useful for "is X installed?" without forcing an explicit `service:` query.

---

## F. Identity, time, and host

### user
- **Purpose**: Create, modify, or remove user accounts.
- **Required params**: `name` (string).
- **Key optional params** (large surface):
  - `state` (string, default `present`)
  - `uid` (int)
  - `group` (string), `groups` (list), `append` (bool, default `false`)
  - `comment` (string)
  - `home` (path)
  - `shell` (path)
  - `password` (string) — hashed on Linux, plaintext on macOS
  - `create_home` (bool, default `true`)
  - `move_home` (bool, default `false`)
  - `system` (bool, default `false`)
  - `force` (bool, default `false`), `remove` (bool, default `false`)
  - `password_lock` (bool) — Linux/BSD
  - `expires` (float) — epoch
  - `generate_ssh_key` (bool, default `false`), `ssh_key_file` (path, default `.ssh/id_rsa`),
    `ssh_key_type` (string, default `rsa`), `ssh_key_bits` (int),
    `ssh_key_comment` (string), `ssh_key_passphrase` (string)
  - `local` (bool, default `false`) — use luseradd etc. to bypass nss
  - `password_expire_max`, `password_expire_min`, `password_expire_warn`,
    `password_expire_account_disable` (int) — Linux
  - `update_password` (string, default `always`) — `always`/`on_create`
  - `login_class` (string) — BSD
  - `skeleton` (string)
  - `umask` (string) — Linux
  - `uid_min`, `uid_max` (int) — Linux
  - `seuser` (string) — SELinux
  - `authorization`, `profile`, `role` (string) — Illumos/Solaris
  - `non_unique` (bool, default `false`)
  - `hidden` (bool) — macOS
- **Returns**: `name`, `uid`, `group`, `groups`, `home`, `shell`, `comment`, `password` (masked), `ssh_key_file`, `ssh_public_key`, `ssh_fingerprint`, plus the input flags echoed back.
- **Idempotent**: Yes; with `update_password: always` the password hash is rewritten each run when comparing fails.
- **Check-mode supported**: Full. Diff none.
- **Notes**: Underlying tooling: `useradd`/`usermod`/`userdel` on Linux, `pw`/`chpass` on FreeBSD, `dscl`/`dseditgroup` on macOS. Shadow file is automatically backed up on SunOS; other OSes rely on the underlying tool's backup. Use `update_password: on_create` to prevent password churn on every run.

### group
- **Purpose**: Manage local groups.
- **Required params**: `name` (string).
- **Key optional params**:
  - `state` (string, default `present`)
  - `gid` (int)
  - `system` (bool, default `false`)
  - `local` (bool, default `false`) — use `lgroupadd`
  - `non_unique` (bool, default `false`) — needs `gid:`; not supported on macOS or BusyBox
  - `gid_min`, `gid_max` (int) — Linux only, override `/etc/login.defs`
  - `force` (bool, default `false`) — delete even if it's a user's primary group
- **Returns**: `name`, `gid`, `state`, `system`.
- **Idempotent**: Yes.
- **Check-mode supported**: Full. Diff none.
- **Notes**: Requires `groupadd`/`groupdel`/`groupmod` on the target.

### hostname
- **Purpose**: Set the system hostname (transient + persistent).
- **Required params**: `name` (string).
- **Key optional params**:
  - `use` (string) — auto-detected; `alpine`/`debian`/`freebsd`/`generic`/`macos`/`macosx`/`darwin`/`openbsd`/`openrc`/`redhat`/`sles`/`solaris`/`systemd`
- **Returns**: standard; updates `ansible_facts`.
- **Idempotent**: Yes.
- **Check-mode supported**: Full. Diff full.
- **Notes**: Does **not** modify `/etc/hosts`. Non-FQDN values save you from name resolution stalls. macOS uses `scutil` for HostName/ComputerName/LocalHostName. Not supported on Windows, HP-UX, AIX.

### cron
- **Purpose**: Manage entries (and env vars) in user or system crontabs.
- **Required params**: `name` (string) — used as the `#Ansible: <name>` marker comment.
- **Key optional params**:
  - `job` (string) — required when `state: present` and `env: false`
  - `minute`/`hour`/`day`/`weekday`/`month` (string, default `*`)
  - `special_time` (string) — `annually`/`daily`/`hourly`/`monthly`/`reboot`/`weekly`/`yearly`
  - `state` (string, default `present`)
  - `user` (string)
  - `cron_file` (path) — modify a system file in `/etc/cron.d` instead
  - `env` (bool, default `false`) — set an env var
  - `insertafter` (string), `insertbefore` (string) — for env vars
  - `disabled` (bool, default `false`) — comment-out the job
  - `backup` (bool, default `false`)
- **Returns**: `envs` (list), `jobs` (list).
- **Idempotent**: Yes — uses comment marker as identity.
- **Check-mode supported**: Full. Diff full.
- **Notes**: Requires Vixie cron-compatible `crontab` (cronie works). For environment variables (`env: true`), the `name:` becomes the variable name and `job:` becomes the value.

### known_hosts
- **Purpose**: Add/remove SSH host keys in a known_hosts file.
- **Required params**: `name` (string).
- **Key optional params**:
  - `key` (string) — required when state=present; full known_hosts-format line including hostname
  - `state` (string, default `present`)
  - `path` (path, default `~/.ssh/known_hosts`)
  - `hash_host` (bool, default `false`)
- **Returns**: standard.
- **Idempotent**: Yes — multiple entries allowed per host (one per key type).
- **Check-mode supported**: Full. Diff full.
- **Notes**: The hostname must be lower-case; include port for non-22 services as `[host.example.com]:2222`. For managing many keys, the `template:` module against a known_hosts file is more efficient.

---

## G. Diagnostics & control flow

### debug
- **Purpose**: Print a message or variable.
- **Required params**: none (but you usually want one of `msg` or `var`).
- **Key optional params**:
  - `msg` (string, default `Hello world!`) — mutually exclusive with `var`
  - `var` (string) — variable name (do NOT wrap in `{{ }}` — implicit template)
  - `verbosity` (int, default 0) — only emit when `-v` count is at least this
- **Returns**: `msg`.
- **Idempotent**: Yes (read-only).
- **Check-mode supported**: Full. Diff none.
- **Notes**: `var:` runs through Jinja2 with implicit `{{ }}` wrapping — passing `{{ foo }}` will double-template. `verbosity:` lets you ship debug tasks in production playbooks that only fire under `-vv` etc.

### assert
- **Purpose**: Fail the task if any expression in `that:` evaluates falsy.
- **Required params**: `that` (list of strings).
- **Key optional params**:
  - `fail_msg` (string; alias `msg`) — printed on failure
  - `success_msg` (string) — printed on success
  - `quiet` (bool, default `false`) — suppress per-assertion output
- **Returns**: standard.
- **Idempotent**: Yes (no side effects).
- **Check-mode supported**: Full. Diff none.
- **Notes**: Expressions use the same syntax as `when:`. Use early in roles to validate required vars (`assert: that: foo is defined`).

### fail
- **Purpose**: Stop the play (or current host's play) with a custom error.
- **Required params**: none.
- **Key optional params**:
  - `msg` (string, default `Failed as requested from task`)
- **Returns**: standard.
- **Idempotent**: Yes (errors don't accumulate).
- **Check-mode supported**: Full. Diff none.
- **Notes**: Almost always paired with `when:`. Use `meta: end_play` instead if you want to short-circuit *successfully*.

### set_fact
- **Purpose**: Define one or more variables scoped to the host (and persisted to the fact cache when `cacheable: true`).
- **Required params**: at least one `key=value` pair (free-form) or YAML keys.
- **Key optional params**:
  - `cacheable` (bool, default `false`)
- **Returns**: `ansible_facts` dict.
- **Idempotent**: Yes (always reassigns; second run produces the same result).
- **Check-mode supported**: Full.
- **Notes**: Values are evaluated *eagerly* at assignment time (unlike normal "lazy" Ansible variables). Boolean strings (`yes`/`true`/etc.) auto-convert in `key=value` syntax — use YAML notation for explicit types. With `cacheable: true`, two copies live: a higher-precedence host var and a lower-precedence ansible_fact (the `meta: clear_facts` action will wipe only the latter — confusing!). Don't mix `key=value` and YAML notation in one task.

### set_stats
- **Purpose**: Track custom statistics across a run, optionally per-host or aggregated.
- **Required params**: `data` (dict).
- **Key optional params**:
  - `per_host` (bool, default `false`)
  - `aggregate` (bool, default `true`) — combine with existing values
- **Returns**: standard; values surface in `PLAY RECAP` when `show_custom_stats=True` is set in `ansible.cfg` (or via env `ANSIBLE_SHOW_CUSTOM_STATS=true`).
- **Idempotent**: Conditional (if `aggregate: true`, repeated runs accumulate).
- **Check-mode supported**: Full.
- **Notes**: Mostly used by callback plugins / CI integrations — Tower/AWX surfaces these stats on job runs.

### add_host
- **Purpose**: Add a host (and optionally a group) to the *in-memory* inventory for use in later plays of the same run.
- **Required params**: `name` (string; aliases `host`/`hostname`) — can include `:port`.
- **Key optional params**:
  - `groups` (list or string; aliases `group`/`groupname`)
  - any number of free-form `key=value` host vars
- **Returns**: standard; mutates inventory.
- **Idempotent**: Yes (subsequent calls update vars rather than duplicating).
- **Check-mode supported**: Partial — still mutates in-memory inventory in check mode.
- **Notes**: The added host respects CLI `--limit`. Useful pattern: provision a VM in play 1 with a cloud module, `add_host:` the new IP, then play 2 targets the new group. `bypass_host_loop: full` — runs once even when invoked under a host pattern.

### group_by
- **Purpose**: Create dynamic inventory groups based on facts on each host.
- **Required params**: `key` (string) — usually templated, e.g. `os_{{ ansible_facts['distribution'] }}`.
- **Key optional params**:
  - `parents` (list, default `["all"]`)
- **Returns**: standard.
- **Idempotent**: Yes.
- **Check-mode supported**: Partial — mutates in-memory inventory.
- **Notes**: Spaces in group names are auto-converted to hyphens. The classic OS/distro fan-out pattern.

---

## H. Imports & includes

### include_tasks
- **Purpose**: Dynamically include a list of tasks from another file at runtime.
- **Required params**: one of `file` (string, since 2.7) or the free-form `include_tasks: stuff.yml`.
- **Key optional params**:
  - `apply` (dict, since 2.7) — task keywords (tags, become, etc.) applied to each included task
- **Returns**: standard.
- **Idempotent**: depends on what's included.
- **Check-mode supported**: None at the include level (skipped).
- **Notes**: **Dynamic** — `when:`, loops, and templated filenames work. Tags on the include statement are NOT auto-inherited; use `apply:` for that. The `do-until` loop is not supported here.

### import_tasks
- **Purpose**: Statically pre-include tasks at parse time (before play runs).
- **Required params**: `file` (string) or free-form.
- **Key optional params**: none.
- **Returns**: standard.
- **Idempotent**: depends on what's imported.
- **Check-mode supported**: Inherited.
- **Notes**: **Static** — conditionals and loops on the import statement apply to the imported tasks individually, not to the import itself. Cannot template the filename. Choose this when you want simple, predictable parsing; choose `include_tasks` when you need runtime conditionals or dynamic file selection.

### include_role
- **Purpose**: Dynamically execute a role as if it were a task (in `pre_tasks`/`tasks`/`post_tasks` or inside a role's tasks).
- **Required params**: `name` (string).
- **Key optional params**:
  - `tasks_from` (string, default `main`)
  - `vars_from` (string, default `main`)
  - `defaults_from` (string, default `main`)
  - `handlers_from` (string, default `main`)
  - `allow_duplicates` (bool, default `true`)
  - `public` (bool, default `false`) — expose role vars/defaults to subsequent tasks
  - `apply` (dict) — task keywords applied to each role task
  - `rolespec_validate` (bool, default `true`)
- **Returns**: standard.
- **Idempotent**: depends on the role.
- **Check-mode supported**: Full at the include statement.
- **Notes**: Cannot be used in handlers. Conditionals and loops on the `include_role` statement apply to the include, not the role tasks (use `apply:` to fan-out). The dynamic counterpart of `import_role`.

### import_role
- **Purpose**: Statically import a role at parse time.
- **Required params**: `name` (string).
- **Key optional params**:
  - `tasks_from`, `vars_from`, `defaults_from`, `handlers_from` (string, default `main`)
  - `allow_duplicates` (bool, default `true`)
  - `public` (bool, default `true` since 2.17)
  - `rolespec_validate` (bool, default `true`)
- **Returns**: standard.
- **Idempotent**: depends.
- **Check-mode supported**: Full.
- **Notes**: Cannot be used in handlers. Like `import_tasks`, conditionals/loops apply to the imported tasks individually. Defaults and vars are exposed to the rest of the play because of how parse-time inclusion works.

### import_playbook
- **Purpose**: Pull another playbook into the current playbook at parse time.
- **Required params**: free-form filename.
- **Key optional params**: none.
- **Returns**: standard.
- **Idempotent**: depends.
- **Check-mode supported**: Full at the import.
- **Notes**: Can ONLY appear at the top level of a playbook (not inside a play). Tags applied to the import statement propagate to the imported plays' tasks. Conditionals on the import are inherited.

### include_vars
- **Purpose**: Load variables from a YAML/JSON file or directory at task runtime.
- **Required params**: one of `file` (path), `dir` (path), or free-form filename.
- **Key optional params**:
  - `name` (string) — wrap variables under this key; omit for top-level
  - `depth` (int, default 0) — recursion limit when loading dirs
  - `extensions` (list, default `["json", "yaml", "yml"]`)
  - `files_matching` (string) — regex filter
  - `ignore_files` (list)
  - `ignore_unknown_extensions` (bool, default `false`) — allow READMEs etc.
  - `hash_behaviour` (string) — `replace` or `merge`
- **Returns**: `ansible_facts` dict.
- **Idempotent**: Yes (read-only).
- **Check-mode supported**: Full.
- **Notes**: Files in `dir:` are loaded alphabetically. Use `delegate_to: foo` + `delegate_facts: true` to push the loaded vars onto another host.

### include
- **Purpose**: REMOVED. Was a unified include-task-or-role variant deprecated mid-2018, removed mid-2023.
- **Notes**: Replace with `include_tasks` or `import_tasks` (or `include_role` / `import_role` for roles). runsible should not implement this name at all.

---

## I. Meta actions

### meta
- **Purpose**: Trigger an Ansible-engine-internal action (does not invoke a remote module).
- **Required params**: a free-form action name.
- **Available actions**:
  - `flush_handlers` — run all pending handlers right now (don't wait for end-of-play sync point).
  - `refresh_inventory` — reload dynamic inventory (e.g. after a cloud module created hosts).
  - `clear_facts` — wipe gathered facts on targeted hosts (also clears `cacheable: true` set_facts).
  - `clear_host_errors` — un-fail hosts so subsequent plays target them again.
  - `reset_connection` — drop the persistent SSH multiplex socket so connection-affecting changes (new user, new sudoers entry) take effect.
  - `end_play` — terminate the current play for ALL hosts (not a failure).
  - `end_host` — terminate the current play for THIS host only.
  - `end_batch` — end the current `serial:` batch (no-op if `serial` unset; equivalent to `end_play`).
  - `end_role` — stop executing the rest of the current role (handlers still fire).
  - `noop` — internal placeholder.
- **Returns**: standard.
- **Idempotent**: Yes (these are control-flow primitives, not state mutations).
- **Check-mode supported**: Partial.
- **Notes**: `meta` is the only module that doesn't run on the remote. Tag selection works since 2.11. `bypass_host_loop` is partial — most subactions ignore per-host iteration. Critical operationally: `flush_handlers` after a config write that needs immediate effect; `reset_connection` after `become_user:` changes; `clear_host_errors` is rarely the right answer (better: fix the underlying failure).

---

## J. Connection & fact gathering

### ping
- **Purpose**: Verify SSH (or whichever connection plugin) works AND that Python is usable on the managed node.
- **Required params**: none.
- **Key optional params**:
  - `data` (string, default `pong`) — value echoed back; setting to `crash` forces an exception
- **Returns**: `ping` (string, usually `"pong"`).
- **Idempotent**: Yes.
- **Check-mode supported**: Full.
- **Notes**: Not ICMP. This is a Python-runs-on-target probe. For Windows, `ansible.windows.win_ping`. For network gear, `ansible.netcommon.net_ping`. The canonical first-run smoke test.

### setup
- **Purpose**: Gather facts about a remote host into `ansible_facts.*`.
- **Required params**: none.
- **Key optional params**:
  - `gather_subset` (list, default `["all"]`) — restrict to subsets, prefix `!` for negation; `!all` minimal, `!all,!min` empty
  - `gather_timeout` (int, default 10) — per-collector timeout
  - `filter` (list, default `[]`) — fnmatch patterns; only matching first-level facts returned
  - `fact_path` (path, default `/etc/ansible/facts.d`) — directory of `*.fact` files (executable or static JSON/INI) for local custom facts
- **Returns**: `ansible_facts` dict (hundreds of keys: `ansible_distribution`, `ansible_distribution_version`, `ansible_default_ipv4`, `ansible_processor`, `ansible_memtotal_mb`, etc.).
- **Idempotent**: Yes (read-only).
- **Check-mode supported**: Full.
- **Notes**: Filter applies only to the first-level keys. Custom facts are simple: a `*.fact` file in `fact_path` becomes `ansible_local.<filename>.<key>`. Adds `facter_*` and `ohai_*` prefixed facts when those tools are installed. BSD systems require become for full gather. Windows facts come via PowerShell scripts.

### gather_facts
- **Purpose**: Wrapper around configured fact-gathering modules; the default fact module is `setup`, but config can swap it (e.g. `smart_facts` from a custom plugin).
- **Required params**: none.
- **Key optional params**:
  - `parallel` (bool, default `true` when multiple fact modules configured) — run fact modules concurrently; can be overridden by `ansible_facts_parallel`
- **Returns**: `ansible_facts`.
- **Idempotent**: Yes.
- **Check-mode supported**: Full (runs by default in check mode).
- **Notes**: This is what the implicit `Gathering Facts` task at play start actually invokes. Parallel mode trades latency for total CPU. With `gather_timeout` and parallel, total is bounded by the slowest collector, not the sum.

### mount_facts
- **Purpose**: Return mount table information from /etc/mtab, /proc/mounts, /etc/fstab, etc.
- **Required params**: none.
- **Key optional params**:
  - `devices` (list) — fnmatch patterns to filter by device
  - `fstypes` (list) — fnmatch patterns to filter by FS type
  - `include_aggregate_mounts` (bool) — return `aggregate_mounts` list when same mount point appears in multiple sources
  - `mount_binary` (any, default `mount`)
  - `on_timeout` (string, default `error`) — `error`/`warn`/`ignore`
  - `sources` (list) — `all`/`static`/`dynamic` aliases or explicit paths
  - `timeout` (float) — per-mount; null = infinite
- **Returns**: `ansible_facts` with mount data; `aggregate_mounts` when enabled.
- **Idempotent**: Yes.
- **Check-mode supported**: Full.
- **Notes**: Added in ansible-core 2.18. Static sources include `/etc/fstab`, `/etc/vfstab`, `/etc/filesystems`. For modifying mounts (vs just reading), use `ansible.posix.mount` (out of `ansible.builtin`).

### package_facts
- **Purpose**: Return the installed package list as a fact.
- **Required params**: none.
- **Key optional params**:
  - `manager` (list, default `["auto"]`) — `auto`, `apt`, `dnf`/`rpm`/`zypper`, `pacman`, `pkg`, `pkg_info`, `portage`, `apk`
  - `strategy` (string, default `first`) — `first` (first available manager) or `all` (every available manager)
- **Returns**: `ansible_facts.packages` — dict of `{package_name: [{name, version, source, arch, ...}]}` (lists because the same name can have multiple installed versions, e.g. multiple kernels).
- **Idempotent**: Yes.
- **Check-mode supported**: Full. Diff none.
- **Notes**: This is the right tool for "is X installed?" — much better than parsing `dpkg -l` via `command:`. Requires the relevant Python bindings on the managed node (python-apt for apt, RPM bindings for rpm).

### service_facts
(See section E.)

### getent
- **Purpose**: Run `getent` against a database (passwd/group/hosts/services/etc.) and return the result as a fact.
- **Required params**: `database` (string).
- **Key optional params**:
  - `key` (string) — single entry to look up; otherwise full enumeration
  - `service` (string, since 2.9) — `--service` flag if supported by the OS
  - `split` (string) — field separator override
  - `fail_key` (bool, default `true`)
- **Returns**: `getent_<database>` fact (list/dict of fields).
- **Idempotent**: Yes.
- **Check-mode supported**: Full.
- **Notes**: Not all databases support enumeration; check OS docs. Multiple duplicate entries are returned since 2.11.

### async_status
- **Purpose**: Poll an async task's status by job id.
- **Required params**: `jid` (string).
- **Key optional params**:
  - `mode` (string, default `status`) — `status` or `cleanup`
- **Returns**: `started` (bool), `finished` (bool), `ansible_job_id`, `stdout`, `stderr`, `erased` (path when cleanup mode).
- **Idempotent**: Yes (read-only or cleanup).
- **Check-mode supported**: Full (since 2.17).
- **Notes**: The poll-in-a-loop companion to running tasks with `async: 600 poll: 0`. With `until: result.finished`, and `retries:`/`delay:`, you build a poll-until-done pattern.

---

## K. Repository / VCS

### git
- **Purpose**: Clone, fetch, or update a Git repository on the managed node.
- **Required params**: `repo` (string), `dest` (path; not required if `clone: false`).
- **Key optional params**:
  - `version` (string, default `HEAD`)
  - `force` (bool, default `false`)
  - `depth` (int) — needs git ≥1.9.1
  - `clone` (bool, default `true`)
  - `update` (bool, default `true`)
  - `accept_hostkey` (bool, default `false`) — `StrictHostKeyChecking=no` (MITM risk)
  - `accept_newhostkey` (bool, default `false`, since 2.12) — `StrictHostKeyChecking=accept-new` (safer)
  - `ssh_opts` (string)
  - `key_file` (path)
  - `executable` (path)
  - `bare` (bool, default `false`)
  - `recursive` (bool, default `true`) — submodules
  - `reference` (string)
  - `remote` (string, default `origin`)
  - `refspec` (string)
  - `single_branch` (bool, default `false`) — since 2.11
  - `track_submodules` (bool, default `false`)
  - `separate_git_dir` (path) — since 2.7
  - `archive` (path), `archive_prefix` (string, since 2.10) — `zip`/`tar.gz`/`tar`/`tgz`
  - `umask` (any)
  - `verify_commit` (bool, default `false`) — needs git ≥2.1.0
  - `gpg_allowlist` (list, default `[]`) — alias `gpg_whitelist` deprecated
- **Returns**: `after` (commit SHA), `before` (commit SHA, null if new clone), `remote_url_changed` (bool), `git_dir_before`, `git_dir_now`.
- **Idempotent**: Yes (compares HEAD to requested version).
- **Check-mode supported**: Full. Diff full.
- **Notes**: Requires git ≥ 1.7.1 on the managed node. Don't embed credentials in repo URLs — use SSH keys or a credential helper. `dest:` must be empty for a fresh clone. `single_branch` + `depth` give you the cheapest possible clone for CI use.

### subversion
- **Purpose**: Checkout/update/export an SVN repo.
- **Required params**: `repo` (string; aliases `name`, `repository`).
- **Key optional params**:
  - `dest` (path) — required unless all of checkout/update/export are off
  - `revision` (string, default `HEAD`; aliases `rev`, `version`)
  - `checkout` (bool, default `true`)
  - `update` (bool, default `true`)
  - `export` (bool, default `false`)
  - `force` (bool, default `false`) — discard local mods or fail
  - `in_place` (bool, default `false`)
  - `switch` (bool, default `true`)
  - `username` (string), `password` (string)
  - `executable` (path)
  - `validate_certs` (bool, default `false`, since 2.11)
- **Returns**: standard.
- **Idempotent**: Yes (compares revision).
- **Check-mode supported**: Full. Diff none.
- **Notes**: Requires `svn` on the target. Does not handle externals.

---

## L. Network and host operations

### iptables
- **Purpose**: Manage individual iptables rules in the running kernel (does NOT persist).
- **Required params**: typically `chain` (string).
- **Key optional params** (large surface — selected highlights):
  - `state` (string, default `present`)
  - `table` (string, default `filter`) — `nat`/`mangle`/`raw`/`security`
  - `action` (string, default `append`) — `append`/`insert`
  - `rule_num` (string) — position when inserting
  - `chain_management` (bool, default `false`) — allow chain create/delete
  - `protocol` (string) — `tcp`/`udp`/`icmp`/`ipv6-icmp`/`esp`/`ah`/`sctp`/`all`
  - `source`, `destination`, `src_range`, `dst_range`, `source_port`, `destination_port`, `destination_ports` (list, since 2.11)
  - `in_interface`, `out_interface`
  - `ip_version` (string, default `ipv4`) — `ipv4`/`ipv6`/`both`
  - `ctstate` (list)
  - `syn` (string) — `ignore`/`match`/`negate`
  - `tcp_flags` (dict)
  - `jump` (string), `goto` (string)
  - `policy` (string) — for chain default policy
  - `reject_with` (string)
  - `match` (list), `match_set` (string, since 2.11), `match_set_flags`
  - `limit`, `limit_burst`
  - `fragment`
  - `comment`
  - `to_source`, `to_destination`, `to_ports`
  - `gateway` (since 2.8) — TEE
  - `set_dscp_mark`, `set_dscp_mark_class`
  - `log_prefix`, `log_level`
  - `flush` (bool)
  - `set_counters`
  - `uid_owner`, `gid_owner`
  - `icmp_type`
  - `wait`
  - `numeric` (bool, default `false`)
- **Returns**: standard.
- **Idempotent**: Yes for individual rules (the module checks for existence first).
- **Check-mode supported**: Full. Diff none.
- **Notes**: Linux only; in-memory only — pair with iptables-save / iptables-persistent for persistence (or use `community.general.iptables_state`). DNS resolution happens once when the rule is submitted. For complex chained policies, render via `template:` and apply with `iptables-restore` via `command:`. Requires `become: true`.

### reboot
- **Purpose**: Reboot a host and wait for it to come back.
- **Required params**: none.
- **Key optional params**:
  - `pre_reboot_delay` (int, default 0) — seconds before kicking the reboot; converted to minutes (rounded down) on Linux/macOS/OpenBSD
  - `post_reboot_delay` (int, default 0) — settle time after the host comes back
  - `reboot_timeout` (int, default 600)
  - `connect_timeout` (int) — plugin default
  - `test_command` (string, default `whoami`)
  - `msg` (string, default `Reboot initiated by Ansible`)
  - `search_paths` (list, default `["/sbin", "/bin", "/usr/sbin", "/usr/bin", "/usr/local/sbin"]`) — where to find the shutdown binary
  - `boot_time_command` (string, default `cat /proc/sys/kernel/random/boot_id`) — must produce a different value pre/post reboot
  - `reboot_command` (string, since 2.11) — override; absolute path or a name searched in `search_paths`. If set, the delay/msg parameters are ignored.
- **Returns**: `elapsed` (int), `rebooted` (bool).
- **Idempotent**: No (reboots every run unless guarded with `when:`).
- **Check-mode supported**: Full.
- **Notes**: For Windows, `ansible.windows.win_reboot`. The total wait can be 2× `reboot_timeout` because the timeout applies to each phase (down + up). Use `boot_time_command` if `/proc/sys/kernel/random/boot_id` doesn't exist (some embedded systems).

### pause
- **Purpose**: Pause execution for a duration or until acknowledged.
- **Required params**: typically one of `seconds`/`minutes`/`prompt`.
- **Key optional params**:
  - `seconds` (string), `minutes` (string)
  - `prompt` (string)
  - `echo` (bool, default `true`) — echo input
- **Returns**: `delta` (string), `echo` (bool), `start`, `stop`, `stdout`, `user_input`.
- **Idempotent**: Yes (no side effects).
- **Check-mode supported**: Full.
- **Notes**: ctrl+c then `c` to continue early, `a` to abort. `bypass_host_loop: full` so it pauses once even when the play targets many hosts.

---

## M. Validation / scaffolding

### validate_argument_spec
- **Purpose**: Validate a dict of arguments against a role argument spec (the `meta/argument_specs.yml` schema).
- **Required params**: `argument_spec` (dict).
- **Key optional params**:
  - `provided_arguments` (dict)
- **Returns**: `argument_errors` (list), `argument_spec_data` (dict), `validate_args_context` (dict — role name/path/type).
- **Idempotent**: Yes.
- **Check-mode supported**: Full. Diff none.
- **Notes**: Since 2.11. Used implicitly when a role declares `argument_specs.yml`; can also be invoked manually to validate ad-hoc dicts.

### tempfile
(See section B.)

---

## N. Modules requested but NOT in `ansible.builtin`

These names appear in the user's must-have list but live outside the
`ansible.builtin` namespace. runsible should not implement them under that
name in v1; they belong to community-tier collections.

| Requested name | Actual home                | Notes |
| -------------- | -------------------------- | ----- |
| `archive`      | `community.general.archive`| Symmetric to `unarchive`. Common ask — runsible should provide this in v1.5 at the latest. |
| `at`           | `ansible.posix.at`         | `at` job scheduling. |
| `mount`        | `ansible.posix.mount`      | Modify `/etc/fstab` and live mounts. |
| `sysctl`       | `ansible.posix.sysctl`     | Manage `/etc/sysctl.conf` entries. |
| `mail`         | `community.general.mail`   | Send mail from a play. |
| `crypto`       | `community.crypto.*`       | Family: `openssl_certificate`, `openssl_privatekey`, `x509_certificate`, etc. Runsible should ship a minimal cert-gen story. |
| `pacman`       | `community.general.pacman` | Arch package manager. |
| `zypper`       | `community.general.zypper` | SUSE package manager. |
| `homebrew`     | `community.general.homebrew` | macOS package manager. |
| `validate`     | not a module               | `validate:` is a parameter on `copy`, `template`, `lineinfile`, `blockinfile`, `assemble`, `replace`. |
| `debugger`     | not a module               | The `debugger:` keyword on plays/tasks (`always`/`never`/`on_failed`/`on_unreachable`/`on_skipped`) controls the interactive debugger; it's not a module invocation. |

---

## O. v1 implementation priorities for runsible

Sorting the catalog by "playbook-line frequency in the wild," here is the
suggested first-tier ship order. (Numbers reflect typical real-world coverage,
not user-listed order.)

**Tier 1 — must work day one (50% of typical playbook lines):**

1. `command`, `shell`, `raw`, `script` — execution
2. `copy`, `template`, `file`, `stat` — file management
3. `apt`, `dnf` (alias `yum`), `package`, `pip` — packages
4. `service`, `systemd_service` (alias `systemd`) — services
5. `user`, `group` — identity
6. `ping`, `setup`, `gather_facts` — bootstrap and probes
7. `debug`, `assert`, `fail`, `set_fact` — flow control
8. `include_tasks`, `import_tasks`, `include_role`, `import_role`,
   `import_playbook`, `include_vars` — composition
9. `meta` (at least: `flush_handlers`, `end_play`, `end_host`,
   `reset_connection`, `clear_facts`, `noop`)

**Tier 2 — needed for any serious workflow (next 30%):**

10. `lineinfile`, `blockinfile`, `replace`, `find`, `assemble`, `tempfile`
11. `unarchive` (no `archive` in builtin — accept this as v1)
12. `get_url`, `uri`, `wait_for`, `wait_for_connection`
13. `cron`, `hostname`, `known_hosts`
14. `add_host`, `group_by`
15. `git` (subversion is much rarer)
16. `apt_repository`, `deb822_repository`, `apt_key`, `yum_repository`,
    `rpm_key`
17. `service_facts`, `package_facts`, `mount_facts`, `getent`
18. `sysvinit` (legacy support but cheap to write)
19. `iptables`, `reboot`, `pause`

**Tier 3 — fill out the namespace:**

20. `dnf5`, `dpkg_selections`, `debconf`, `set_stats`,
    `validate_argument_spec`, `async_status`, `expect`, `slurp`, `fetch`

**Out of scope for v1:** all redirects/aliases, all network device modules
(aireos/asa/eos/ios/iosxr/junos/nxos/vyos/etc. — they live in netcommon and
vendor collections), `include` (removed upstream).

---

## P. Cross-cutting concerns runsible must handle uniformly

These are the common parameter conventions and behaviors that recur across
modules. runsible's module framework should bake them in once rather than
re-implementing per module.

### File attribute family
Every file-touching module accepts the same 8 fields:
- `owner`, `group`, `mode`
- `attributes` (chattr flags)
- `seuser`, `serole`, `setype`, `selevel` (SELinux MLS context)

Plus `unsafe_writes: bool` for atomic-write fallback on broken filesystems
(NFS, Docker bind mounts) and `safe_file_operations: full` capability when
the module supports atomic write+chmod.

### Validation hook
`validate:` on `copy`/`template`/`lineinfile`/`blockinfile`/`assemble`/
`replace` runs `<command> %s` against the staged temp file before promoting.
Standard pattern: `validate: nginx -t -c %s`, `validate: /usr/sbin/sshd -t
-f %s`, `validate: visudo -cf %s`.

### `creates`/`removes` guards
On `command`/`shell`/`raw`/`script`/`expect`/`uri`: skip the task if
`creates` exists (or `removes` doesn't). This is the only way to make those
modules idempotent.

### Vault auto-decryption
`copy`, `template`, `script`, `unarchive`, `assemble` auto-decrypt vaulted
source files (`decrypt: true`). `lineinfile`, `blockinfile`, `replace`
explicitly do NOT — those edit existing files in place.

### Check mode levels
- **Full**: predicts changes accurately, never modifies (most state-changing
  modules: copy, template, file, lineinfile, etc.)
- **Partial**: predicts only when guards are present (command/shell with
  `creates`/`removes`; unarchive — partial because of gzipped tar
  limitations).
- **None**: cannot run in check mode; task is skipped.

### Diff mode levels
- **Full**: produces unified-diff output (copy, template, lineinfile,
  blockinfile, replace, find, assemble, fetch).
- **Partial**: limited diff (file — perms only; unarchive — uses gtar
  --diff).
- **None**: no diff (most read-only modules don't need it; debug/assert/
  set_fact don't either).

### Connection independence
`debug`, `assert`, `fail`, `set_fact`, `set_stats`, `add_host`, `group_by`,
`include_*`, `import_*`, `meta`, `pause`, `validate_argument_spec` all run
on the controller and don't open a connection. They are tagged
`connection: none` in the docs. runsible should treat them as a separate
"action plugin" tier that doesn't even need a transport.

### Become semantics
Most modules respect `become:` to elevate privileges. The `meta`,
`pause`, `set_fact`, `assert`, `debug`, `fail`, `add_host`, `group_by`,
`include_*`, `import_*`, `wait_for_connection`, and `validate_argument_spec`
modules explicitly note `become: none` — these run on the controller anyway.
Forgetting `become: true` on `apt`/`dnf`/`service`/etc. is the single most
common new-user error; runsible should consider making "needs root" a
declared module attribute and warning when it's missing.
