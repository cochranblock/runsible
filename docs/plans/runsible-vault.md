# runsible — `runsible-vault`

## 1. Mission

Encrypt secrets at rest, decrypt at runtime, manage recipient keys. Crucially: **`runsible-vault` is not a Rust port of `ansible-vault`.** The Ansible vault is a symmetric password-file scheme — one shared secret per file (or per `--vault-id`), and "rotation" means everybody re-fetching the new password file from wherever it is currently being smuggled. That model is the single most quoted reason MSPs and small platform teams refuse to share an Ansible tree across more than a handful of engineers, and "vault password leaked on Slack" is a recurrent incident class. `runsible-vault` is a redesign around per-recipient asymmetric encryption: every secret is encrypted under a per-file data-encryption key (DEK) that is **wrapped per recipient** using age (X25519) or SSH (`ssh-ed25519` / `ssh-rsa`) public keys, with the wrapped DEKs sitting in the file header. Adding or removing a teammate is one command and does not re-encrypt the body. There is no shared "vault password" on disk. Compatibility with the Ansible format is provided as a one-shot import path only — we read and decrypt `$ANSIBLE_VAULT;1.1`/`;1.2` files, never write new ones, and rewrite imported files into the runsible-native `$RUNSIBLE_VAULT;1` envelope on first touch. Restating §6 of `11-poor-decisions.md`: "every team member who can decrypt can also encrypt — there's no read-only role; rotation requires re-encrypting every file and notifying every user; the password lives in a file." We delete those three statements.

## 2. Scope

**In scope:**

- Native age- and SSH-key recipient model for every encrypted artifact runsible owns.
- Whole-file encryption: `secrets.toml` → `secrets.toml.vault` (and the inverse).
- Inline-value encryption inside a TOML file (the `!vault` equivalent for TOML — no custom tag syntax, since TOML has no tags; we use a structured table).
- Per-file recipient management (`add`, `remove`, `list`) with no body re-encryption.
- One-shot ingest of existing `ansible-vault` files (`import-ansible`).
- Password-file fallback for transition (`--password-file`), kept for one major version with a deprecation warning.
- Local key storage at `~/.runsible/keys.toml`, with optional pass-through to system keyring and `ssh-agent`.
- Library API consumed by `runsible-playbook`, `runsible`, `runsible-pull`, `runsible-inventory`, `runsible-console`.

**Out of scope (v1):**

- **Symmetric-only operation as default.** Exists for legacy import and the `encrypt-string --recipient @passphrase` corner case; the CLI nags.
- **Key escrow.** Solve it at the recipient layer (CISO recipient on every file; hardware-token recipient in the safe). No "recovery key" pseudo-recipient.
- **HSM and PKCS#11 integration.** Deferred to v2+; the recipient model is extension-friendly.
- **Secret stores as a backend** (HashiCorp Vault, AWS Secrets Manager, GCP Secret Manager). Those are runtime-fetch tools; vault is the at-rest crate. A future `runsible-fetch` covers that case.
- **Re-encrypting to the Ansible format.** Migrator is one-way.

## 3. File format on disk

### 3.1 The runsible-native envelope

A vault file is one ASCII header line followed by a base64-encoded payload wrapped to 76 columns:

```
$RUNSIBLE_VAULT;1;CHACHA20-POLY1305;AGE;3
YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IFgyNTUxOSAxLi4uClUyRnNkR1ZkWDE5dEt6...
ZklRdmFKckRsYWZ0d3FrZ2FnTjBPVGc1V21oamRPLTNydmRkSnY0NW1XZjg9CjE5T0R...
LS0tIEdGM2N0eDl4WDFGTUMtcEZWQVFFUDQwMHFGM2N0eDl4WDFGTUMtcEZWQVFFCg==
```

Header fields:

1. **Magic** — `$RUNSIBLE_VAULT`. Distinct from `$ANSIBLE_VAULT`.
2. **Version** — `1`. Single integer (TOML/SemVer culture, not Python's `1.1`/`1.2`).
3. **DEK cipher** — `CHACHA20-POLY1305` in v1; field exists so hardware-AES platforms can opt into `AES-256-GCM` without a new envelope.
4. **Wrap scheme** — `AGE` (X25519 + SSH in one blob). Future: `AGE2`, `HPKE`.
5. **Recipient count** — sanity check; cross-checked against age's internal count. Mismatch = "envelope tampered," abort.

Header is terminated by a single `\n`. CRLF terminator is rejected at line 1 with `vault: header has CRLF; rewrite with LF`. Ansible tolerates CRLF and fails opaquely later (Quirk #1 in `04-vault.md`); we don't inherit that.

Body is `base64(age_payload)`, wrapped to 76 columns for git-diff friendliness. The age payload is canonical age binary — no custom framing, no hex-in-hex (Quirk #10). It contains the recipient stanzas with wrapped DEKs, then ChaCha20-Poly1305 ciphertext + Poly1305 tag (no separate HMAC; AEAD handles integrity).

### 3.2 Annotated example

A file written for three recipients (one age, one ssh-ed25519, one ssh-rsa), conceptually:

```
$RUNSIBLE_VAULT;1;CHACHA20-POLY1305;AGE;3      <- header
age-encryption.org/v1                          <- age's own header (inside base64)
-> X25519 nQbHuP...                            <- wrap #1: age recipient (alice)
J2v...                                         <- wrapped DEK for alice (32 bytes)
-> ssh-ed25519 K3p... ZHQ                      <- wrap #2: bob's id_ed25519.pub
3yZ...                                         <- wrapped DEK for bob
-> ssh-rsa W4f... bxq                          <- wrap #3: carol's id_rsa.pub
QlA...                                         <- wrapped DEK for carol
--- IGZjOWFjMmQ4...                            <- age internal HMAC of recipients block
<binary body: ChaCha20-Poly1305 ciphertext + Poly1305 tag>
```

### 3.3 Comparison to the Ansible format

| Aspect | Ansible (`$ANSIBLE_VAULT;1.2;AES256;label`) | runsible (`$RUNSIBLE_VAULT;1;CHACHA20-POLY1305;AGE;N`) |
|---|---|---|
| Symmetric or asymmetric? | Symmetric password | Asymmetric per-recipient DEK wrapping |
| Recipients per file | 1 (the password) | N (any mix of age + SSH) |
| Add/remove a person | Re-encrypt every byte | Re-wrap a 32-byte DEK |
| Cipher | AES-256-CTR + HMAC-SHA256 (homebrew encrypt-then-MAC) | ChaCha20-Poly1305 (AEAD) |
| KDF | PBKDF2-HMAC-SHA256, **fixed** 10000 iters | None on hot path; key passphrases use age scrypt |
| Wire encoding | hex-in-hex with `\n`-split inside | base64 of canonical age binary |
| Header carries recipient identity? | Free-text label only | Typed recipient list |
| Tolerates CRLF header? | Yes, silently (corrupts body parse later) | No (hard error at parse) |

### 3.4 Inline TOML vault values

TOML has no user-defined tags, so the `!vault` equivalent is a structured table with a fixed shape:

```toml
[secrets]
db_password = { vault = "v1", recipients = ["age1qx...0xj", "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIK3p..."], ciphertext = "YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+..." }
```

Fixed keys:

- `vault` — version; must be `"v1"`.
- `recipients` — array of recipient public keys; advisory (used to pick a key and for `recipients list` without body parse). Source of truth is the wrapped DEKs inside the ciphertext; mismatch is a hard error.
- `ciphertext` — base64 of the age payload, same as a whole-file vault body.

`encrypt-string` emits the long-form table for readability:

```toml
[secrets.db_password]
vault = "v1"
recipients = ["age1qx...0xj", "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIK3p..."]
ciphertext = """
YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IFgyNTUxOSBuUWJIdVAuLi4KSjJ2Li4uCi0+IHNzaC1lZDI1
NTE5IEszcC4uLiBaSFEKM3laLi4uCi0tLSBJR1pqT1dGak1tUTQuLi4KPGJpbmFyeT4=
"""

[secrets.api_token]
plaintext = "not-actually-secret-just-an-id"
```

Encrypted and unencrypted values coexist. Decryption is **lazy** — a value is decrypted only when the engine reads it (loader leaves a typed `VaultValue` placeholder). A project may carry secrets a given operator can't decrypt; runs succeed as long as nothing references them. Matches the `!vault` tagged-scalar behaviour Ansible users expect (§7.1 of `04-vault.md`). Whole-file vaults are eager: `db.toml.vault` is decrypted at parse time.

## 4. Recipient model

A **recipient** is a public-key identity allowed to decrypt the file. One of:

- **Age recipient**: bech32-encoded X25519 public key starting with `age1`. Generated by `keygen` or any age tool (`age-keygen`, `rage-keygen`).
- **SSH recipient**: OpenSSH `authorized_keys`-line format — `ssh-ed25519`, `ssh-rsa`, `ecdsa-sha2-nistp256/384/521`, `sk-ssh-ed25519@openssh.com` (FIDO2). Pulled from `~/.ssh/*.pub` or fetched from `https://github.com/<user>.keys` via `--from github:<user>`. Rides on age's native SSH support; we don't reinvent key parsing.

Encryption flow: generate a 256-bit random DEK; AEAD-encrypt the body (ChaCha20-Poly1305, 96-bit nonce framed by age); wrap the DEK for each recipient (age's X25519 ECDH or SSH-key ECDH); emit envelope header + base64 of the age payload.

**Adding a recipient** (`recipients add <file> --recipient age1...`):

1. Decrypt only the DEK of `<file>` using a recipient we already hold a private key for.
2. Wrap the same DEK for the new recipient.
3. Splice the new wrapped-DEK into the recipient stanza, re-emit the envelope. **The body ciphertext is bit-identical.** A 50MB encrypted blob can have a teammate added in milliseconds.

**Removing a recipient** drops their wrapped-DEK stanza and re-emits the envelope.

**Caveat:** removing a recipient does not invalidate copies the removed recipient already cloned. Surfaced in the CLI output ("rotate the underlying secrets — this only removes the recipient from future copies"). Leaver runbook: rotate the upstream credential, `recipients remove`, then `rekey` (mint a new DEK).

**Read-only access.** Age does not natively distinguish "can decrypt" from "can encrypt" — anyone who is a recipient can encrypt new content for the same set. A strict read-only role (decrypt yes, encrypt no) is **not provided**, and the docs say so. What most users actually mean by "read-only" is **separation of decrypt from CI write access**: a CI service account is decrypt-only by virtue of not having git-push permission on the secrets file. For organisational read-only-with-audit, the recipient list is signed by a separate signing key (see §7) and `runsible-vault verify --signed-by <pubkey>` attests the recipient list has not been tampered with. Not classical RBAC, but what the threat model can enforce honestly.

## 5. CLI surface

- **`runsible-vault init`** — creates `~/.runsible/` (`0700`) and `keys.toml` (`0600`), prompts for a passphrase, mints a personal age keypair, writes a `recipients.toml` stub. Refuses to overwrite without `--force`.

- **`runsible-vault keygen [--name <label>] [--out <path>]`** — mints an age keypair; with `--name` adds to the local keys file under that label, with `--out` writes to a path. Public key to stdout.

- **`runsible-vault recipients add|remove|list <file>`** — `add` accepts `--recipient <pubkey>` (repeatable), `--from github:<user>`, or `--from team:<team>` (reads `recipients.toml`). `remove` takes `--recipient`. `list` prints the file's recipients.

- **`runsible-vault encrypt <file> [--recipient ...] [--from team:<name>] [--output -]`** — encrypts in place or to stdout. Recipients from flags, group, or `runsible.toml` `[vault] default_recipients`.

- **`runsible-vault decrypt <file> [--output -]`** — confirmation prompt unless `--yes`. (Quirk #9 of `04-vault.md`: Ansible has none and clobbers files silently.)

- **`runsible-vault edit <file>`** — decrypt to `memfd_create(2)` on Linux (tmpfs per-user dir on macOS/BSD, named pipe on Windows), open `$EDITOR`, re-encrypt on close. Plaintext never hits a disk inode another process can read. Quirk #6 of `04-vault.md`.

- **`runsible-vault view <file>`** — decrypt and pipe through `$PAGER`. No temp file when the pager reads stdin (`less`, `most`, `bat`).

- **`runsible-vault rekey <file>`** — mint a fresh DEK, re-encrypt body, re-wrap for all current recipients. Used after `recipients remove` or on rotation cadence.

- **`runsible-vault encrypt-string <value> [--recipient ...] [--from team:<name>] [--name <key>]`** — emits a TOML inline-table snippet. `--prompt` reads from TTY (no echo); `--stdin` from stdin. Best-effort: zero-byte over the argv slot to defend against shell history.

- **`runsible-vault import-ansible <file> --recipients <team.toml>`** — reads `$ANSIBLE_VAULT;1.1`/`;1.2` (prompts for legacy password or `--password-file`), decrypts via PBKDF2/AES-256-CTR/HMAC-SHA256, re-encrypts under runsible recipients. `--in-place` overwrites and renames `<file>` → `<file>.vault`. Directory argument walks recursively.

- **`runsible-vault verify <file>`** — parses envelope, attempts decryption with each local key, reports per-recipient match. Used by `runsible-lint` and CI. Non-zero on parse failure or (with `--require-decrypt`) no local key opened it.

Cross-cutting on every subcommand: `-v/--verbose`, `-h`, `--version`, `--keystore <path>`.

## 6. Recipients file (`recipients.toml`)

A TOML file mapping team-member names and group names to public keys. Lives at `~/.runsible/recipients.toml` for personal use and `<project>/recipients.toml` (or `<project>/.runsible/recipients.toml`) for project-shared. Project takes precedence on lookup.

```toml
[people.alice]
keys = ["age1qx9k...0xj", "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIK3p... alice@laptop"]
github = "alice-mcp"
notes = "primary on-call"

[people.bob]
keys = ["age1xrs...8vn"]
github = "bobsled"

[people.carol]
keys = ["ssh-rsa AAAAB3NzaC1yc2EAAAADAQABA... carol@yubikey"]
notes = "FIDO2-backed; hardware prompts on use"

[people.ci-prod]
keys = ["age1ci0...rxq"]
notes = "GitHub Actions deployer; private key in GH secrets"

[teams.platform]
members = ["alice", "bob"]
[teams.prod]
members = ["alice", "ci-prod"]
[teams.staging]
members = ["alice", "bob", "ci-prod"]
[teams.everyone]
members = ["alice", "bob", "carol", "ci-prod"]
```

`--from team:platform` and `--recipient @alice` consult this file. Rotating a key = `recipients.toml` PR + `runsible-vault recipients add/remove` on the affected files. We do **not** make `recipients.toml` "the source of truth that auto-syncs every file" — that would re-introduce the rekey-the-world cost we are deleting. The file is a directory, not a contract; per-file recipients are fixed at encryption time.

## 7. Crypto details

- **DEK cipher: ChaCha20-Poly1305** with a fresh random 256-bit key per file. Constant-time on every CPU runsible runs on. §16 discusses the AES-GCM alternative.
- **DEK nonce:** 96-bit random per file, framed by age (not separately stored).
- **Per-recipient wrap: age v1.** age uses X25519 + ChaCha20-Poly1305 for X25519 recipients; for SSH recipients it uses `ssh-ed25519` via X25519 conversion or `ssh-rsa` via RSA-OAEP-SHA256 wrap. We depend on the `age` crate (current-stable) and consume its public API; we do not re-implement the wrap.
- **SSH key support:** age natively supports `ssh-ed25519` and `ssh-rsa`. `ecdsa-sha2-nistp256/384/521` accepted with a warning. FIDO2-backed `sk-ssh-ed25519@openssh.com` accepted with a runtime warning that decryption requires the hardware token at apply time.
- **File integrity:** the AEAD tag on the body is the integrity check. No separate HMAC; Poly1305 is the MAC.
- **Signing keys:** for the recipient-list tamper-evidence path in §4, the file optionally carries an **Ed25519 signature stanza** over the canonicalised recipient list plus the body's AEAD tag. The signing public key lives in `recipients.toml` under `[signing]`; `runsible.toml` declares which signing key is required for which paths (`[vault.signed]` `"secrets/prod/" = "age1prod-signer..."`). Enforced by `verify --signed-by` and by `runsible-playbook` when policy demands it. Opt-in; default files are unsigned. Useful for the P3 (compliance) persona.
- **For `import-ansible` compatibility:** we re-implement the Ansible decryption side per §2.3 of `04-vault.md`: PBKDF2-HMAC-SHA256, 10000 iterations, 32-byte salt, 80 derived bytes split 32/32/16, AES-256-CTR with the 16-byte slice as big-endian IV, HMAC-SHA256 over ciphertext using the second 32 bytes, PKCS#7 padding stripped. **No encryption side** — there is no `export-ansible`.

Crypto crates: `age` (recipient wrap), `chacha20poly1305` (held direct in case we swap body cipher), `pbkdf2`/`hmac`/`sha2`/`aes`/`ctr` (import-ansible only), `ed25519-dalek` (signing), `secrecy` (in-memory key material; zeroised on drop, excluded from Debug/Display), `subtle` (constant-time compares), `zeroize`.

## 8. Integration with `runsible-playbook` and friends

`runsible-vault` is a library crate with a thin CLI. Library API the workspace consumes:

- `Vault::open(path: &Path) -> Result<VaultFile>` — parses the envelope; nothing is decrypted yet.
- `VaultFile::decrypt(&self, keystore: &dyn KeyStore) -> Result<SecretBytes>` — decrypts via whichever local private key matches a wrapped DEK.
- `Value::is_vault(&self) -> bool` — TOML loader hook: does this parsed table match the inline-vault shape?
- `Value::resolve_vault(&self, keystore: &dyn KeyStore) -> Result<Value>` — lazy resolution of an inline vault table.

Engine rules:

- **Inline vault values** decrypt at first read, lazily. Unreferenced vault values are never decrypted, never error. Matches §7.1 of `04-vault.md`.
- **Encrypted whole files** (`<name>.toml.vault`) decrypt at load; the decryption produces an in-memory TOML string parsed directly from the buffer. We deliberately **do not** materialise to a tmpfile. Deletes Quirk #6 of `04-vault.md` for the runtime case as well as the editor case.
- **Decryption errors are typed and surfaced**, never collapsed into "decryption failed":
  - `VaultError::NoMatchingKey` — none of the operator's keys match any wrap stanza.
  - `VaultError::AeadFailure` — AEAD tag did not verify; corrupt file or wrong key opening the wrong stanza.
  - `VaultError::HeaderMalformed { line, reason }` — envelope parse failure with a precise pointer.
  - `VaultError::SignatureMissing` / `VaultError::SignatureInvalid` — when policy required a signature.
  - `VaultError::AnsibleLegacyFile` — file is `$ANSIBLE_VAULT`; the message says "use `runsible-vault import-ansible`".
- **The engine never holds a decrypted secret in a `String`.** Decryption returns `SecretBytes` (zeroising on drop); the templating engine consumes them through a `SecretRef` scoped to the templating call. In-process plaintext lifetime is microseconds by the time a task hits the SSH wire.
- **Decryption is the engine's call into vault, not the other way around.** Vault exposes a pure-library API; it does not sniff which keystore is configured. The `KeyStore` trait is implemented by `runsible-config`-supplied backends (file, libsecret, Keychain, Credential Manager, ssh-agent); the calling binary wires the right one.

## 9. Key storage

Where the operator's **private** keys live. Lookup order (first hit wins):

1. **`~/.runsible/keys.toml`** — passphrase-protected, age-passphrase-encrypted at rest. Each key has a label and metadata:

   ```toml
   [keys.default]
   public = "age1qx9k...0xj"
   private = "AGE-SECRET-KEY-1QSY...8GH"   # itself age-passphrase-encrypted
   created = 2026-04-26T09:00:00Z
   notes = "personal default"

   [keys.prod-deploy]
   public = "age1prod...rxq"
   private = "AGE-SECRET-KEY-1XYZ...PQR"
   created = 2026-04-26T09:05:00Z
   notes = "delegated to ci-prod"
   ```

2. **System keyring** — service `runsible-vault`, account = key label. libsecret (gnome-keyring/kwallet/KeePassXC) on Linux, Keychain (`security` framework) on macOS, Credential Manager (Win32) on Windows. Uniform via the `keyring` crate.

3. **`ssh-agent`** — for SSH-key recipients. Vault asks the agent to do X25519 ECDH using the agent-held key; private key never enters runsible's process. Strict win for FIDO2 ed25519, agent-forwarded keys, and PKCS#11-to-ssh-agent smart cards.

4. **`--key <path>`** — explicit age-format private key, for one-off ops and CI.

`keys.toml` is `0600` in a `0700` directory; vault refuses to read either with worse perms with a clear "your keystore is world-readable, fix permissions and retry" message. SSH-client-style hardening.

## 10. Migration story (Ansible → runsible)

Operator flow for moving a team off `ansible-vault`:

1. **`runsible-vault init`** — creates `~/.runsible/keys.toml`, prompts for a passphrase, mints an `age1...` keypair, prints the public key.

2. **Each teammate does step 1**, then sends the team their public key. (Teams with SSH keys on GitHub use `recipients add --from github:<user>` instead.)

3. **Build the recipients file** at the project root or use ad-hoc `--recipient` flags:

   ```toml
   [people.alice]
   keys = ["age1qx9k...0xj"]
   [people.bob]
   keys = ["age1xrs...8vn"]
   [people.ci-prod]
   keys = ["age1ci0...rxq"]
   [teams.prod]
   members = ["alice", "bob", "ci-prod"]
   ```

4. **Import the existing vault files.**

   ```sh
   runsible-vault import-ansible group_vars/prod/secrets.yml --from team:prod --in-place
   ```

   Decrypts the legacy file (prompting once for the Ansible password, or reading `--password-file`), re-encrypts under team:prod, renames `secrets.yml` → `secrets.toml.vault`.

5. **Bulk-import a directory:** `runsible-vault import-ansible group_vars/ --from team:prod --in-place --recurse`. Walks recursively, converts every file with the `$ANSIBLE_VAULT` magic.

6. **Drop the Ansible password file.** Delete `~/.vault_password`, remove `--vault-password-file` from CI scripts.

7. **Update CI** to grant the deploy account its own age key. Private key lives age-passphrase-encrypted in CI secrets (e.g. GitHub Actions secret `RUNSIBLE_VAULT_KEY`); job runs with `RUNSIBLE_VAULT_KEY_PASSPHRASE` set. CI's public key is just another `recipients.toml` entry.

8. **Compatibility shim.** For one major version, `--vault-password-file <path>` is honoured on every secret-touching binary. When a vault file is loaded, the asymmetric path is tried first; on `NoMatchingKey`, the legacy path runs with a deprecation warning. Removed at v2.

## 11. Redesigns vs Ansible

Where `runsible-vault` deliberately does not match `ansible-vault`. Cross-referenced to §6 of `11-poor-decisions.md` and §3 of `04-vault.md`.

- **Drop the symmetric password model entirely.** Asymmetric per-recipient encryption is the default and recommended path. Symmetric is import-only.
- **Drop `--ask-vault-pass` / `-J`.** A passphrase prompt is the wrong abstraction for a vault — what the operator gates on a passphrase is *their private key*, not the vault file. We keep `--ask-pass` on the keystore but never prompt for a "vault password" to open a file. The Ansible triad of `--ask-vault-pass` / `--vault-password-file` / `--vault-id` collapses to `--key <path|label>` plus optional `--ask-pass`. Legacy flags accepted for one major version.
- **Drop multiple `--vault-id`s.** Recipients subsume them: instead of "this file uses the `prod` password," "this file is encrypted *for* the prod team." An operator on both teams holds both private keys. The decryption code tries every local key against every wrap stanza.
- **Drop the `1.1`-vs-`1.2` header distinction.** The recipient list is the identity. No free-text label field on disk. `recipients.toml` carries the human-readable names; the file's recipient list carries cryptographic identities.
- **Drop `encrypt_string`'s hex-in-hex wire format.** Inline values are TOML tables containing base64 of the same envelope as whole files. Quirk #5 of `04-vault.md` (`encrypt_string` can't be rekeyed) is gone — inline values rekey via the same path as files.
- **Drop the in-place "edit" tmpfile race.** `memfd_create(2)` on Linux, `mkstemp` in `chmod 0700` per-user dir on macOS/BSD, named-pipe on Windows. Editor never sees a shared-`/tmp` path. Quirk #6 of `04-vault.md`.
- **Drop tolerance of `\r\n`-mangled headers.** Hard-error at parse with a precise message. Quirk #1 of `04-vault.md`.
- **Drop the fixed PBKDF2 iteration count.** No KDF in the body cipher's hot path; the only KDF is age's passphrase scrypt, which carries its parameters in the ciphertext. No "stuck on 10000" failure mode. Quirk #11 of `04-vault.md`.
- **Drop "no confirmation prompt on `decrypt`."** Prompt unless `--yes`. Quirk #9.
- **Drop the `*-client` executable convention.** Replaced by the `KeyStore` trait. Custom integrations implement `KeyStore`, not "script that prints a password to stdout." Legacy honoured during the deprecation window only.
- **Drop the "label appears in plaintext in headers" gotcha** (§2.1 field 4 of `04-vault.md`). The envelope carries cryptographic identities only; friendly names live in `recipients.toml`.

## 12. Milestones

- **M0:** envelope read/write; encrypt/decrypt files with one or more age recipients; CLI for `init`, `keygen`, `encrypt`, `decrypt`, `edit`, `view`, `recipients list`. Single-recipient case round-trips; multi-recipient case works for age-only recipients. Library API stable enough that `runsible-playbook` can call it.

- **M1:** inline TOML vault values (the `{ vault = "v1", ... }` table); `recipients add/remove` with no body re-encrypt; `rekey`; `verify`; `encrypt-string`; recipients can be named via `recipients.toml`. `runsible-playbook` integrates lazy resolution.

- **M2:** `import-ansible` (full migration including PBKDF2/AES-CTR/HMAC re-implementation); SSH-key recipient support (ed25519 first, then RSA, then ECDSA with caveats). Bulk directory import. Compatibility shim for `--vault-password-file`.

- **M3:** keyring backends (libsecret, Keychain, Credential Manager); ssh-agent integration; signing-key path for the compliance persona; `verify --signed-by`; `runsible-lint` rules (e.g., "this `<file>.vault` has no recipient overlap with this project's `recipients.toml`"); doc generator producing the operator-facing migration guide from this plan.

## 13. Dependencies on other crates

Runtime dependencies on other runsible crates: **none**. `runsible-vault` is a leaf in the dependency DAG. It does not parse playbooks, query inventory, open SSH, or read workspace config. (It reads `~/.runsible/keys.toml` and `recipients.toml`; those are vault-owned files.)

Reverse dependencies — crates that use vault as a library:

- **`runsible-playbook`** — decrypts vault values during template rendering; loads `<file>.toml.vault` at parse time.
- **`runsible`** (ad-hoc), **`runsible-console`** — same.
- **`runsible-pull`** — pulled repo may contain vault files.
- **`runsible-inventory`** — `host_vars/<host>.toml.vault` and `group_vars/<group>.toml.vault` are first-class.
- **`runsible-config`** — does not depend on vault, but provides the `KeyStore` backends (libsecret/Keychain/Credential Manager wrappers) vault consumes via the trait. Trait lives in vault; implementations live in config. Avoids vault taking transitive deps on every keyring backend.
- **`runsible-lint`** — uses `Vault::open` and the verify path to assert vault files are well-formed and `recipients.toml` is consistent.

External crates: `age`, `chacha20poly1305`, `pbkdf2`, `hmac`, `sha2`, `aes`, `ctr`, `ed25519-dalek`, `secrecy`, `subtle`, `zeroize`, `base64`, `bech32`, `rand`, `keyring` (optional, feature-gated), `ssh-key` (for the recipients-file parser).

## 14. Tests

**Unit:**

- Envelope round-trip with every supported recipient type (age, ssh-ed25519, ssh-rsa, ecdsa, sk-ssh-ed25519); single-recipient and every combination of two and three.
- Recipient `add` preserves body bytes (encrypt → SHA-256 body → add → SHA-256 body → assert equal). `remove` likewise.
- `rekey` *changes* body bytes (negative test).
- `decrypt` after `add` works for the new recipient; after `remove` fails for the removed one.
- AEAD tamper: flip a body bit → `AeadFailure`. Header tamper: flip recipient count → `HeaderMalformed`. CRLF: replace header `\n` with `\r\n` → `HeaderMalformed { reason: "CRLF" }`.
- KDF correctness for `import-ansible`: known-answer test vectors for PBKDF2/HMAC.
- `import-ansible` round-trip: encrypt with real `ansible-vault` (CI fixture only), import with runsible, assert plaintext matches.
- Inline TOML vault parse + decrypt; mixed encrypted + cleartext in one file.
- `KeyStore` conformance: mock + file + libsecret (feature-gated) pass the same property tests.

**Integration:**

- Multi-recipient file (alice, bob, carol) decrypted by each of three subprocesses, each holding only one key.
- Project-tree fixture: `recipients.toml`, `secrets.toml.vault`, playbook referencing an encrypted value; `runsible-playbook` runs against localhost and the value templates into a task. Tests the cross-crate seam.
- `import-ansible` against `tests/fixtures/ansible-vault-corpus/` — twenty real-shape Ansible vault files (1.1, 1.2, mixed `!vault` inline, binary blobs); all must round-trip.
- Editor-flow: `runsible-vault edit` with `EDITOR` set to a mutating script; assert no plaintext leaked to any path findable via `find /tmp -newer ...` during the edit window. (Linux memfd path; macOS/BSD tmpfs path tested separately.)
- Performance: `recipients add` on a 100MB encrypted blob completes in <500ms (body is not re-encrypted; should be milliseconds regardless of body size).

**Negative / fuzz:**

- `cargo-fuzz` target on the envelope parser. Property tests with random bytes as headers; assert no panics, always a clean error.

## 15. Risks

- **Crypto is unforgiving — get age integration right, do not roll our own.** Biggest risk in this crate. Mitigation: depend on the `age` crate's published, audited public API; do not reach into internals; gate every release on `cargo deny` + `cargo audit` + the test corpus. The `import-ansible` PBKDF2/AES-CTR path is the one place we reimplement crypto, and it is decryption-only (no encryption), which shrinks the attack surface — but still needs a KAT corpus and ideally third-party review before v1.
- **Key-storage UX is the actual UX moat over `ansible-vault`.** If the keyring story is bad — libsecret needing a dbus session, Keychain prompting twice, Credential Manager needing elevation — users fall back to `~/.runsible/keys.toml` with an empty passphrase, which is `~/.vault_password` with extra steps. Mitigation: M3 ships a stress-tested backend per platform and a per-invocation DEK cache (keyring hit once, not per-task).
- **`import-ansible` correctness.** PBKDF2 iters, salt size, hex-in-hex parse, padding strip, HMAC-then-decrypt order — all five are gettable wrong; Quirk #10 of `04-vault.md` exists because ad-hoc reimplementations get them wrong. Mitigation: published test-vector corpus in `tests/fixtures/ansible-vault-corpus/`; cross-check against Ansible's own unit tests; dedicated module with `#![deny(unsafe_code)]` and 100% line coverage.
- **Recipient management at scale.** A 100-person team rotating recipients on hundreds of files via a script: slow because of process startup, not crypto. Mitigation: M2 ships a multi-file `recipients add --recipient ... <file> <file> ...` form; future `runsible-vault repair --reconcile-with recipients.toml` for leaver-case bulk remove.
- **Recipient list visibility.** The recipient list is in the file header; it is not a secret. For most threat models this is fine (`recipients.toml` was already public in the repo); for some it is sensitive (a leaked file reveals who has prod access). Documented. Mitigation for the sensitive case is group-shared keys, but that re-introduces the symmetric-password problem in miniature, so we discourage it.
- **`memfd_create` is Linux-only.** macOS, BSDs, Windows fall back to `chmod 0700` per-user temp dirs, which are weaker. UX works everywhere; leak surface is platform-dependent. Documented.
- **Age v1 wire format may evolve.** If age v2 lands, we need a `$RUNSIBLE_VAULT;2;...;AGE2;N` envelope and a transitional reader. The version field is for exactly this. Long-horizon risk.

## 16. Open questions

- **Default DEK cipher: ChaCha20-Poly1305 vs AES-256-GCM.** Both AEAD, both well-vetted, both constant-time. ChaCha20-Poly1305 wins on ARM (Raspberry Pi running runsible-pull) and is age's default. AES-256-GCM wins on x86-AES-NI by ~3x raw throughput. Decision: ship ChaCha20-Poly1305 in v1 — aligns with age, faster on the slowest target hardware, and body-cipher cost is irrelevant for typical secrets (<10KB). Revisit if a real workload reports otherwise. Open until v1 cut.
- **Native SSH-key recipient: depend on `age-ssh` transitively or vendor SSH key parsing.** age supports `ssh-ed25519`/`ssh-rsa` via `age::ssh` behind a feature flag (`ssh = ["age/ssh"]`). Decision-pending: enable the feature, accept transitive deps on `rsa`/`ssh-key`/`pem-rfc7468`, do not vendor. Open until M2.
- **"Team server" / recipient registry for very large teams.** Not in v1 — teams we have spoken to use git for `recipients.toml`. If a customer pulls it in: `--recipients-from <https://...>` fetches and validates against a pinned signing key. Open until v2.
- **`verify --signed-by` behaviour after signing-key rotation.** (a) fail closed (safer) vs (b) key-history file (operator-friendly). Decision: ship (a), document rotation runbook as "rekey + re-sign on rotation," revisit if friction is real.
- **Is `recipients.toml` required?** UX win for teams; bureaucratic overhead for solos (P4/P5). Decision: never required, always honoured if present.
- **How aggressive is the deprecation of `--vault-password-file`?** Honoured in v1.x with warning; removed at v2. ~18-month migration window. Tentative; open to a one-year extension if a major customer hasn't migrated.

— end of plan —
