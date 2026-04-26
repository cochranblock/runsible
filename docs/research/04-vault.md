# Ansible Vault: Exhaustive Reference

This document is an exhaustive reference for Ansible Vault, sourced from the
official documentation under `https://docs.ansible.com/ansible/latest/vault_guide/`
and the `ansible-vault` CLI reference. It is intended to be used as the design
authority for the `runsible-vault` crate so that runsible's vault subsystem can
be byte-for-byte compatible with Ansible's existing on-disk format and
operational semantics.

---

## 1. What Vault Is, and What It Is Not

Ansible Vault is a symmetric-encryption tool that protects "data at rest" inside
Ansible source trees. It can encrypt:

- Individual variables (inline, with the `!vault` YAML tag).
- Entire structured-data files (group_vars, host_vars, var files passed via
  `-e @file.yml`, files loaded by `include_vars` or `vars_files`, role
  `vars/`, `defaults/`, `tasks/`, `handlers/` files, and even arbitrary
  binary files used by `copy`, `template`, `unarchive`, `script`, or
  `assemble`).

Vault does **not** protect data in transit, in memory, or after decryption. The
official guidance states explicitly that "Encryption with Ansible Vault ONLY
protects 'data at rest'." Once Ansible decrypts a value at runtime,
responsibility for not leaking it (logs, debug, registered output, child
processes) falls on the playbook author. The documented mitigations are using
`no_log: true` on tasks that touch secrets and securing the editor used during
`ansible-vault edit` / `create` so that swap/backup files do not leak the
plaintext.

Vault is invoked through one CLI binary, `ansible-vault`, plus implicit
decryption invoked by `ansible`, `ansible-playbook`, `ansible-pull`, and
`ansible-console`.

---

## 2. On-Disk File Format

### 2.1 Header Line

Every vault-encrypted file (and every vaulted string blob) begins with a single
ASCII header line. Two formats exist:

```
$ANSIBLE_VAULT;1.1;AES256
$ANSIBLE_VAULT;1.2;AES256;label
```

Fields are separated by literal semicolons:

1. **Magic** - the constant byte string `$ANSIBLE_VAULT`. In Ansible's source
   this is `b_HEADER = b'$ANSIBLE_VAULT'`.
2. **Version** - either `1.1` or `1.2`. Version `1.0` existed in very early
   releases and is no longer accepted on write. Version `1.2` is identical to
   `1.1` except that the header carries the **vault-id label** in a fourth
   semicolon-separated field. `1.2` is emitted whenever a non-default vault id
   is used at encryption time.
3. **Cipher name** - in current releases the only allowed value, both for
   reading and writing, is `AES256`. The Ansible source has both
   `CIPHER_ALLOWLIST = frozenset((u'AES256',))` and a matching
   `CIPHER_WRITE_ALLOWLIST`. No other ciphers are accepted; an unknown
   cipher name aborts with a fatal error.
4. **Vault-id label** (1.2 only) - free-form text identifying which key was
   used. The documentation calls out that "the label appears in plain text
   within encrypted content headers, documenting which password was used for
   encryption" - this is by design. The label is **not** part of any keyed
   integrity check and must be treated as untrusted input.

The header line is terminated by a single newline (`\n`). Ansible reads the
file in binary mode and tolerates a Unix line ending here; mixed line endings
produced by Windows editors that re-save the whole file will corrupt the
payload (see Quirks below).

### 2.2 Payload Body

Everything after the header newline is the **vaulttext**: an ASCII-only,
hex-encoded blob, formatted into 80-column lines (the last line may be
shorter; no trailing newline is required, but Ansible writes one).

When the surrounding 80-column wrapping is removed and the hex is decoded back
to bytes, the resulting buffer is itself three newline-separated, hexlified
fields:

```
hex(salt)        \n
hex(hmac)        \n
hex(ciphertext)
```

In Python parser terms:

```python
b_salt, b_crypted_hmac, b_ciphertext = b_vaulttext.split(b"\n", 2)
```

Each of those three fields is then `unhexlify`'d independently. So the file is
"hex inside hex" - the outer hex+wrap exists so the entire blob is safe to
embed in YAML; the inner three fields are joined by literal `\n` characters
inside the outer hex.

#### 2.2.1 Salt

- **Length:** 32 bytes of cryptographically random data, freshly generated
  per encryption operation (per file or per `encrypt_string` invocation).
- **Source:** `os.urandom(32)` in Ansible's reference implementation.
- **Override:** the env var `ANSIBLE_VAULT_ENCRYPT_SALT` /
  config option `DEFAULT_VAULT_ENCRYPT_SALT` (INI key
  `vault_encrypt_salt`) may pin the salt - this exists for
  reproducibility and is not for production.

#### 2.2.2 HMAC

- **Algorithm:** HMAC-SHA256 (RFC 2104).
- **Key:** the second 32 bytes of the PBKDF2 output (see 2.3).
- **Input:** the raw, padded, encrypted ciphertext bytes (the same bytes
  whose hex form is the third field).
- **Length:** 32 bytes (SHA-256 output size).
- **Verification order:** Ansible verifies the HMAC **before** attempting
  decryption (encrypt-then-MAC), so a wrong password (which derives a wrong
  HMAC key) fails fast with an integrity error rather than producing
  attacker-controlled garbage plaintext.

#### 2.2.3 Ciphertext

- **Cipher:** AES-256 in CTR mode (`AES-256-CTR`).
- **Key:** the first 32 bytes of the PBKDF2 output.
- **Counter / IV:** the last 16 bytes of the PBKDF2 output, interpreted as a
  big-endian 128-bit integer used to seed CTR.
- **Padding:** PKCS#7 / RFC 5652 section 6.3 padding to the AES 16-byte block
  boundary. (Strictly, CTR mode does not require padding to operate, but
  Ansible's reference implementation still applies PKCS#7 because the
  surrounding code path is shared with block-mode ciphers; the trailing
  padding bytes are stripped on decrypt.)

### 2.3 Key Derivation

Ansible derives 80 bytes of keying material from the user-supplied vault
password using PBKDF2-HMAC-SHA256:

| Parameter         | Value                               |
|-------------------|-------------------------------------|
| PRF               | HMAC-SHA256                         |
| Iterations        | `10000`                             |
| Salt              | the 32-byte salt from the payload   |
| Output length     | `2 * 32 + 16 = 80` bytes            |

The 80-byte output is then partitioned positionally:

| Bytes     | Purpose                       |
|-----------|-------------------------------|
| `[0:32]`  | AES-256-CTR encryption key    |
| `[32:64]` | HMAC-SHA256 key               |
| `[64:80]` | AES-CTR counter / IV (128-bit)|

The PBKDF2 iteration count is hard-coded in the on-disk format: it is **not**
negotiated, **not** parameterised in the header, and a different iteration
count (such as Ansible's higher modern recommendations or a reimplementation's
preference) cannot be expressed in version `1.1` / `1.2` files.

A new file format would be needed to bump iterations safely. Any
re-implementation aiming for compatibility must pin to 10000.

### 2.4 Worked Example of the Layout

Given a file whose header is `$ANSIBLE_VAULT;1.2;AES256;dev`, the body looks
like:

```
3038336232653265386...
6230653430633137653...
...
```

After unwrapping the 80-col formatting and `unhexlify`-ing once, the resulting
bytes (shown here ASCII-printable) look like:

```
30383362326532653...\n
6230653430633137...\n
3138353634653930...
```

After splitting on the first two `\n` and `unhexlify`-ing each piece a second
time, you have raw `salt` (32 bytes), raw `hmac` (32 bytes), and raw
`ciphertext` (variable length).

---

## 3. Operations

The `ansible-vault` binary exposes seven subcommands:

### 3.1 `create`

Opens an editor (resolved from `$EDITOR`, falling back to `vi`) on a temp file,
then encrypts the contents back to disk on close.

```
ansible-vault create --vault-id test@multi_password_file foo.yml
```

The new file's header is written from the supplied vault id (`1.2` if the
label is non-default). The temp file is created inside `$TMPDIR` (or the
platform default, typically `/tmp`); see Quirks for the implications.

### 3.2 `encrypt`

Encrypts one or more existing plaintext files in place.

```
ansible-vault encrypt foo.yml bar.yml baz.yml
```

Multiple files may be passed; each gets its own header and its own fresh
salt/HMAC/ciphertext. With `--output -` the result goes to stdout (only valid
with a single input file).

### 3.3 `decrypt`

Permanently strips encryption and writes the plaintext back to disk (or
stdout via `--output -`). Multiple files may be passed.

```
ansible-vault decrypt foo.yml bar.yml baz.yml
```

There is no confirmation prompt; running this on tracked files clobbers them
in place, so the documentation explicitly warns against it.

### 3.4 `view`

Decrypts to memory and pipes through `$PAGER` (defaults to the system pager,
typically `less`). Read-only; does not modify the file.

```
ansible-vault view foo.yml
```

### 3.5 `edit`

Decrypts the file to a temp file, opens `$EDITOR` on it, and re-encrypts on
exit, then deletes the temp file. The `view` subcommand gets the plaintext
through the pager; `edit` gets it through the editor. Same `$TMPDIR`
considerations apply.

```
ansible-vault edit foo.yml
ansible-vault edit --vault-id pass2@vault2 foo.yml
```

### 3.6 `rekey`

Re-encrypts existing files under a new password and/or vault id. Both the old
secret (for decryption) and the new secret (for re-encryption) must be
provided.

```
ansible-vault rekey foo.yml bar.yml baz.yml
ansible-vault rekey \
    --vault-id preprod1@ppold \
    --new-vault-id preprod2@prompt \
    foo.yml bar.yml baz.yml
```

This is the only safe way to rotate the password of an existing **file**;
encrypted **strings** (encrypt_string output) cannot be rekeyed - the
documentation states this explicitly. To rotate string-level secrets you must
re-issue every `encrypt_string` call.

### 3.7 `encrypt_string`

Encrypts a single string and prints YAML-ready output to stdout for inline
inclusion in a vars file or play.

```
ansible-vault encrypt_string \
    --vault-password-file a_password_file \
    'foobar' \
    --name 'the_secret'
```

emits

```
the_secret: !vault |
     $ANSIBLE_VAULT;1.1;AES256
     62313365396662343061393464336163383764373764613633653634306231386433626436623361
     6134333665353966363534333632666535333761666131620a663537646436643839616531643561
     ...
```

With a vault id:

```
ansible-vault encrypt_string --vault-id dev@a_password_file 'foooodev' --name 'the_dev_secret'
```

emits a `1.2` header carrying the label:

```
the_dev_secret: !vault |
     $ANSIBLE_VAULT;1.2;AES256;dev
     ...
```

Inputs can also come from stdin:

```
echo -n 'letmein' | ansible-vault encrypt_string --vault-id dev@pwfile --stdin-name 'db_password'
```

or interactively via `--prompt`, with `--show-input` controlling whether the
typed characters are echoed.

The `-n / --name` flag may be repeated to encrypt multiple values in a single
invocation. Beware: typing the cleartext as a positional argument leaks it
into shell history; the documentation flags this and recommends `--prompt` /
`--stdin-name`.

---

## 4. Vault IDs and Multiple Vaults

### 4.1 The `label@source` Syntax

`--vault-id` is the unified flag that supersedes `--ask-vault-pass` and
`--vault-password-file`. Its value has two parts joined by `@`:

```
--vault-id <label>@<source>
```

- `<label>` is an arbitrary token (e.g. `dev`, `prod`, `db`); it ends up in
  the `1.2` header so future runs can identify which key to try first.
- `<source>` is one of:
  - `prompt` - interactively read the password from the TTY.
  - a **file path** - read a single line from that file and use it as the
    password (no trailing newline; the file should be `chmod 600`).
  - an **executable path** - run the file as a subprocess; its stdout is the
    password. The convention is that the executable's filename ends in
    `-client` or `-client.<ext>` to tell Ansible to invoke it as a "vault
    password client" and pass `--vault-id <label>` as an argument.

If the `@source` half is omitted (`--vault-id dev-password`), the entire
value is treated as a path and behaves exactly like
`--vault-password-file dev-password`. Likewise `--vault-id @prompt` is just
`--ask-vault-pass`.

### 4.2 Vault Password Client Scripts

A "vault password client" is the third source option, used to integrate with
external secret stores (Keyring, HashiCorp Vault, etc.). Requirements per the
docs:

- The script's file name **must** end in `-client` or `-client.<ext>`
  (e.g., `vault-keyring-client.py`).
- It must be marked executable.
- It must accept a `--vault-id <label>` argument.
- It must print the resolved password to stdout (and only the password).
- If it needs to ask the user something (e.g. for a master KDF passphrase),
  it must read from / write to the controlling TTY directly, not stdout.

A canonical example shipped in `contrib-scripts/vault/` is
`vault-keyring-client.py`, invoked as
`ansible-playbook --vault-id dev@contrib-scripts/vault/vault-keyring-client.py`.

### 4.3 Multiple `--vault-id` Flags

The flag can be repeated, and Ansible builds a list of (label, secret) pairs.
At decrypt time:

1. If the file's header carries a label, Ansible tries the matching secret
   first.
2. If that fails (or the file is `1.1` and has no label), Ansible falls back
   to trying every other supplied secret in command-line order.

This default behaviour means a file labelled `dev` can still be opened by
the `prod` password, just slower. Setting `DEFAULT_VAULT_ID_MATCH=True`
(`ANSIBLE_VAULT_ID_MATCH`, INI `vault_id_match`) makes the match strict:
Ansible refuses to fall back, so a labelled file can only be opened by its
matching label.

Even with strict matching enabled, the docs note that "Ansible does not
enforce using the same password every time you use a particular vault ID
label" - i.e., the label is just a key into your collection of secrets, not
a fingerprint of the password itself.

### 4.4 Defaults

- `DEFAULT_VAULT_IDENTITY` (INI `vault_identity`, env
  `ANSIBLE_VAULT_IDENTITY`, default `default`) - the label assumed when a
  caller omits it. With this set, `--vault-id @prompt` becomes
  `--vault-id default@prompt`.
- `DEFAULT_VAULT_IDENTITY_LIST` (INI `vault_identity_list`, env
  `ANSIBLE_VAULT_IDENTITY_LIST`, default empty list) - a list of vault ids
  pre-loaded for every invocation; equivalent to passing the same set of
  `--vault-id` flags every time.
- `DEFAULT_VAULT_PASSWORD_FILE` (INI `vault_password_file`, env
  `ANSIBLE_VAULT_PASSWORD_FILE`, default unset) - a single password file
  used when no `--vault-id` is given.
- `DEFAULT_VAULT_ENCRYPT_IDENTITY` (INI `vault_encrypt_identity`, env
  `ANSIBLE_VAULT_ENCRYPT_IDENTITY`, default unset) - when several vault ids
  are loaded, picks which one to use for *encryption* operations (decrypt
  always tries them all per the rules above).

---

## 5. Inline Variables: the `!vault` YAML Tag

`encrypt_string` exists to produce YAML you can paste straight into a vars
file. The output uses two YAML tricks:

```yaml
the_secret: !vault |
     $ANSIBLE_VAULT;1.1;AES256
     62313365...
     6134333665...
```

- `!vault` is a custom YAML tag registered by Ansible's loader. When the
  loader sees a scalar tagged `!vault`, it constructs an `AnsibleVaultEncryptedUnicode`
  object whose value is the raw scalar text. Decryption is deferred until
  the value is dereferenced (lazy, see section 7).
- `|` is YAML's literal block scalar - it preserves newlines exactly. Vault
  payloads need this because they are multi-line.
- The leading whitespace on each payload line is the YAML literal-block
  indent; YAML strips it consistently when reading. **Indentation matters**:
  every payload line must be indented further than the `!vault |` line,
  and uniformly so. If your hand-edited file has a mix of tabs and spaces,
  or if one line dedents, the YAML loader will raise a parse error or, worse,
  truncate the payload.

Encrypted vars happily coexist with unencrypted vars in the same file:

```yaml
db_host: db.example.com           # cleartext
db_password: !vault |             # encrypted
     $ANSIBLE_VAULT;1.2;AES256;prod
     ...
```

Variables can also be encrypted with different vault ids in the same file -
each `!vault` block carries its own header so Ansible knows which key to
apply.

---

## 6. Vault and CLI Flags

This section is the consolidated list of every `ansible-vault` flag, sourced
from the CLI reference page. Flags that begin with `--` accept long form;
short forms are noted explicitly.

### 6.1 Common Flags (all subcommands)

| Flag | Purpose |
|------|---------|
| `-h, --help` | Print usage and exit. |
| `--version` | Print version, config-file location, module search path, exit. |
| `-v, --verbose` | Increase verbosity; repeatable up to `-vvvvvv`. |

### 6.2 Subcommand-specific Flags

The same set of vault-secret flags is available on every encrypting/decrypting
subcommand (`create`, `encrypt`, `decrypt`, `view`, `edit`, `rekey`,
`encrypt_string`):

| Flag | Notes |
|------|-------|
| `--vault-id <label@source>` | Repeatable; supplies a labelled secret. |
| `--vault-password-file <path>` | Alias `--vault-pass-file`. Path to single-line password file or executable client. |
| `-J, --ask-vault-password` | Alias `--ask-vault-pass`. Prompt on TTY. (`--ask-vault-pass` is documented as the legacy name; `--ask-vault-password` is the canonical form. Neither is "deprecated" in the strict sense, but new docs prefer the long form.) |

Encrypting subcommands (`create`, `encrypt`, `edit`, `rekey`,
`encrypt_string`) additionally support:

| Flag | Notes |
|------|-------|
| `--encrypt-vault-id <label>` | When several vault ids are loaded, choose the one to encrypt with. Required if more than one is loaded and `DEFAULT_VAULT_ENCRYPT_IDENTITY` is unset. |

`encrypt`, `decrypt`, and `encrypt_string` accept:

| Flag | Notes |
|------|-------|
| `--output <FILE>` | Write to FILE; `-` writes to stdout. With `decrypt` or `encrypt_string`, the input may come from stdin. |

`encrypt_string` adds:

| Flag | Notes |
|------|-------|
| `-n, --name <NAME>` | Variable name for the YAML output. Repeatable. |
| `-p, --prompt` | Prompt for the cleartext interactively. |
| `--show-input` | Echo characters while prompting (default hides them). |
| `--stdin-name <NAME>` | Read cleartext from stdin and emit a YAML key called NAME. |

`rekey` adds:

| Flag | Notes |
|------|-------|
| `--new-vault-id <LABEL@SOURCE>` | New label/secret to write the file with. |
| `--new-vault-password-file <PATH>` | New password file (without label). |

`create` adds:

| Flag | Notes |
|------|-------|
| `--skip-tty-check` | Allow editor launch without a controlling TTY (useful in sandboxed CI). |

### 6.3 Environment Variables

| Variable | Purpose |
|----------|---------|
| `ANSIBLE_CONFIG` | Override default `ansible.cfg` discovery. |
| `ANSIBLE_VAULT_PASSWORD_FILE` | Default `--vault-password-file`. |
| `ANSIBLE_VAULT_IDENTITY` | Default vault id label. |
| `ANSIBLE_VAULT_IDENTITY_LIST` | Comma-separated list of pre-loaded vault ids. |
| `ANSIBLE_VAULT_ENCRYPT_IDENTITY` | Vault id used for encryption when several are loaded. |
| `ANSIBLE_VAULT_ENCRYPT_SALT` | Pin the salt (testing only). |
| `ANSIBLE_VAULT_ID_MATCH` | If true, decryption only tries the matching label. |
| `EDITOR`, `VISUAL` | Editor used by `create` and `edit`. |
| `PAGER` | Pager used by `view`. |
| `TMPDIR` (and platform equivalents) | Where temp files for `edit`/`create` are written. |

### 6.4 Configuration Files

In ascending precedence:

- `/etc/ansible/ansible.cfg` (system-wide)
- `~/.ansible.cfg` (per-user)
- `./ansible.cfg` (project, current dir)
- `ANSIBLE_CONFIG` (explicit override)

All vault config keys live under the `[defaults]` section.

---

## 7. Vault at Playbook Runtime

### 7.1 When Decryption Happens

The two content shapes have different decryption timing:

- **Encrypted variables** (`!vault` tagged scalars) are **lazy**. The loader
  parses them as `AnsibleVaultEncryptedUnicode`; decryption fires only when
  the value is dereferenced (a templated string, a parameter pass, a debug
  print). This means a vars file may contain encrypted values for vault ids
  the runner doesn't have - and the run will succeed as long as nothing
  touches them.

- **Encrypted files** (whole-file vault) are **eager**. Whenever Ansible
  loads such a file - vars files, group_vars, host_vars, included tasks,
  role files, even files referenced by `copy`, `template`, `unarchive`,
  `script`, `assemble` - it must decrypt the entire file in memory. A
  missing key causes an immediate fatal error.

Special case for file modules: when an encrypted file is the `src` argument
to `copy`, `template`, `unarchive`, `script`, or `assemble`, Ansible
decrypts it on the controller and ships plaintext (or templated plaintext)
to the target. The target host never sees the vault payload. This is the
only sanctioned way to "deploy a vaulted file" - the files end up as
plaintext on the managed host.

### 7.2 Vault for Inventory

Inventory plugins call into the same loader, so any of the following may be
vault-encrypted:

- `host_vars/<host>.yml`, `host_vars/<host>/*.yml`
- `group_vars/<group>.yml`, `group_vars/<group>/*.yml`
- Variables loaded from any custom inventory plugin via the standard
  variable loaders.

Mixing encrypted and unencrypted files in the same `host_vars/<host>/`
directory works. Mixing encrypted and unencrypted **vars** in the same file
also works (using the `!vault` tag for the encrypted ones). What does not
work is mid-file partial encryption of a single value - it's `!vault` or
nothing.

### 7.3 Required Flags at Runtime

`ansible-playbook`, `ansible`, `ansible-pull`, and `ansible-console` all
accept the same vault-secret flags as `ansible-vault`:
`--vault-id`, `--vault-password-file`, `--ask-vault-password`. Without at
least one of those (or the equivalent default config), encountering any
encrypted content is a fatal error.

---

## 8. Quirks and Gotchas

A list of operationally-relevant edge cases, distilled from the docs and
years of community pain. A reimplementation needs to honour these or break
existing user trees.

1. **Line endings.** The on-disk format is plain ASCII inside; Ansible reads
   in binary and tolerates `\n`. Editors (notably on Windows, or VS Code's
   "auto" mode) sometimes rewrite the file with `\r\n` line endings. The
   header line then becomes `$ANSIBLE_VAULT;1.1;AES256\r\n`, and the
   internal `\n`-separated salt/HMAC/ciphertext split breaks. Symptoms are
   "HMAC verification failed" or "vault format unhexlify error". Best
   practice: tell your editor to leave LF.
2. **Trailing whitespace.** Some YAML editors trim trailing spaces line by
   line. Vault payloads don't have trailing spaces, but the indentation of
   inline `!vault |` blocks does have *leading* whitespace - if an editor
   re-indents it inconsistently, YAML parses it as a different scalar and
   the payload is mangled.
3. **Mixed encrypted+unencrypted vars in one file.** Allowed and encouraged;
   each encrypted value carries its own header. Decryption is per-value, so
   only values you actually use need their key available.
4. **Mixed vault ids in one file.** Also allowed; one file may contain
   `!vault` blocks with different `1.2;...;label` headers. The runner just
   needs all referenced labels in its identity list.
5. **`encrypt_string` cannot be rekeyed.** The `rekey` subcommand applies
   only to whole-file vaults. To rotate string-level vault secrets you must
   re-run `encrypt_string` with the new key for every value, then commit.
6. **Vault editor TMPDIR.** `create` and `edit` decrypt to a temp file inside
   `$TMPDIR` (or platform default). On a multi-user box with
   world-readable `/tmp`, this is a leak surface. The docs recommend setting
   `$TMPDIR` to an in-memory tmpfs or a locked-down per-user directory, and
   configuring the editor to disable swap files (`set noswapfile`, `set
   nobackup`, `set nowritebackup`, `set viminfo=`, `set clipboard=` for
   vim; `(setq make-backup-files nil)`, `(setq auto-save-default nil)`,
   `(setq x-select-enable-clipboard nil)` for emacs).
7. **`--ask-vault-pass` vs `--ask-vault-password`.** Both work; the long
   form is canonical in current docs but the short alias is not formally
   deprecated. Same for `--vault-pass-file` vs `--vault-password-file`.
8. **`--vault-id` without `@`.** When the value contains no `@`, Ansible
   treats the whole string as a path/file (or executable client) - i.e.
   identical to `--vault-password-file VALUE`. Useful for terse invocations
   but easily confusing.
9. **Plaintext leakage from `decrypt`.** `ansible-vault decrypt` writes the
   plaintext over the file in place with no prompt. There is no `--dry-run`.
   Audit before running.
10. **Hex-in-hex.** Re-implementations often get this wrong: the outer
    body is not a single hex blob but three hex fields joined by literal
    `\n` characters, then the whole thing is hexlified. You must hex-decode
    once, split on `\n`, hex-decode each piece.
11. **PBKDF2 iteration count is fixed at 10000.** The format does not carry
    iteration count or KDF parameters. You cannot bump iterations without
    inventing a new format version.
12. **Salt size is 32 bytes**, not 16 like many CTR-mode tutorials assume.
13. **HMAC is over the *encrypted* ciphertext** (encrypt-then-MAC). The
    HMAC key is the second 32 bytes of PBKDF2 output; do not use the AES
    key.
14. **`AES256` in the header is mode-agnostic.** It always means AES-256-CTR
    with the IV derived from PBKDF2 bytes 64..80. Don't assume CBC.
15. **`--vault-id` label and the empty string.** `--vault-id @prompt`
    behaves as if you'd asked for the `default` label (or the configured
    `DEFAULT_VAULT_IDENTITY`). Files written with no label use `1.1`
    headers; files written with a non-empty label use `1.2` headers - even
    if that label happens to be the literal string `default`.
16. **Shell history leaks.** The docs warn that
    `ansible-vault encrypt_string ... 'plaintext' --name foo` writes the
    plaintext into shell history. Use `--prompt` or `--stdin-name`.

---

## 9. Implementation Notes for `runsible-vault`

To be byte-for-byte compatible with existing Ansible vault files, runsible's
crate must:

- Implement the `1.1` and `1.2` envelope (header line + hex-wrapped triple).
- Pin AES-256-CTR with a 32-byte salt, 10000 PBKDF2-HMAC-SHA256 iterations,
  80-byte derived material partitioned 32/32/16, encrypt-then-MAC HMAC-SHA256,
  RFC 5652 padding.
- Accept multiple `--vault-id` flags with the documented `label@source`
  semantics, including the file-vs-executable detection and the
  `*-client[.ext]` convention for password-providers.
- Respect `ANSIBLE_VAULT_PASSWORD_FILE`, `ANSIBLE_VAULT_IDENTITY_LIST`,
  `ANSIBLE_VAULT_IDENTITY`, `ANSIBLE_VAULT_ID_MATCH`, and
  `ANSIBLE_VAULT_ENCRYPT_IDENTITY` for default behaviour.
- Implement lazy decryption for `!vault` tagged values when integrating with
  the runsible YAML/TOML loader, so partial-key runs work the way Ansible
  users already expect.
- Honour decrypt-before-deploy semantics for any file-shipping action that
  takes a vaulted source path.
- Reject unknown cipher names; reject unknown header versions; tolerate but
  warn on `\r\n`-mangled headers (rather than silently producing garbage).
- Provide a `rekey` operation that streams through the file, never holding
  the plaintext beyond the lifetime of the in-memory buffer.

A future format version (call it `2.0`) could carry the iteration count,
KDF identifier (Argon2id), and AEAD identifier (ChaCha20-Poly1305 or
AES-GCM) in the header, eliminating the encrypt-then-MAC homebrew. Such a
format would have to be opt-in and never produced unless the user explicitly
asks for it, since vanilla Ansible cannot read it.

---

## 10. Summary Table

| Aspect             | Value                                          |
|--------------------|------------------------------------------------|
| Magic              | `$ANSIBLE_VAULT`                               |
| Versions           | `1.1`, `1.2` (1.2 carries vault-id label)      |
| Cipher             | `AES256` (== AES-256-CTR)                      |
| KDF                | PBKDF2-HMAC-SHA256, 10000 iters                |
| Salt size          | 32 bytes (random per encryption)               |
| Derived bytes      | 80; split 32 / 32 / 16                         |
| Cipher key         | Derived bytes [0..32]                          |
| HMAC key           | Derived bytes [32..64]                         |
| AES counter (IV)   | Derived bytes [64..80] as big-endian int       |
| Padding            | PKCS#7 / RFC 5652 6.3                          |
| Integrity          | HMAC-SHA256 over ciphertext (encrypt-then-MAC) |
| Wire encoding      | hex(triple), wrapped to 80 cols, `\n` between  |
| Inline YAML tag    | `!vault` + literal block (`|`)                 |
| Multiple keys      | `--vault-id label@source`, repeatable          |
| Strict matching    | `DEFAULT_VAULT_ID_MATCH=True`                  |
| File deploy module | Decrypts on controller, ships plaintext        |

This is the surface a compatible implementation must replicate; everything
beyond it (iteration count tuning, AEAD ciphers, key-rotation tooling) is a
deliberate extension and must be opt-in.
