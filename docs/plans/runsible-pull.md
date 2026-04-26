# runsible — `runsible-pull`

## 1. Mission

`runsible-pull` is the inversion of the push model: a managed host fetches its own playbook from a git, HTTPS, or S3 source and applies it locally, on a schedule, against itself. It is the daemon-or-timer that puts a fleet of hundreds-to-thousands of hosts into known-good state without a controller in the middle, and surfaces a heartbeat so the operator can see at a glance which hosts are healthy and which have gone dark. Per §7 of `11-poor-decisions.md` the original `ansible-pull` is the right model wrapped in afterthought UX — no built-in scheduler, no heartbeat, no separation of fetch from apply, no monitoring story — and the entire MSP segment (P2 in `00-user-story-analysis.md`) lives with that pain every day. `runsible-pull` is the wedge that turns the P2 persona from sympathetic onlooker into paying first adopter: it ships a daemon with internal scheduling, an atomically-updated heartbeat, optional HTTP POST, signature verification, and multi-tenant isolation by design. The pitch to a 30-tenant MSP writes itself: one config per tenant, one heartbeat per tenant, one vault recipient per tenant, no shared keys, no shared state, no shared blast radius.

## 2. Scope

**In:**
- Fetch a playbook bundle from one of `git`, `https` (tarball/zip), or `s3` sources, with auth (SSH key, OAuth/PAT token, IAM/instance-profile).
- Apply the fetched playbook locally by spawning `runsible-playbook` (or invoking it as a library) against a `local` connection inventory, with the host's own facts available.
- Emit a structured heartbeat artifact at a configurable local path; update it atomically (write tmp + rename) on every cycle, even on fetch or apply failure.
- Optionally POST the same heartbeat (or a summary subset) to an HTTP endpoint with bearer-token or mTLS auth, configurable retry/backoff.
- Built-in scheduler: `interval = "10m"` plus `jitter` to avoid the thundering-herd problem when 100 hosts boot at once.
- One-shot mode (`--once`) for systemd-timer / cron deployments that prefer external scheduling.
- Verify a detached signature on the fetched bundle against a list of trusted public keys; refuse to apply on signature failure.
- Tenant isolation: each tenant's config is a separate file, runs as a separate systemd unit, writes to a separate heartbeat directory, decrypts vault material with its own recipient key.
- Structured per-cycle run log (NDJSON) with fetch outcome, apply outcome, signature status, and a reference to the playbook hash that was applied.
- Pre-run drift detection: hash the on-disk fetched bundle against the previous successful apply, and surface "no change" cycles as a distinct outcome (cheaper than a full apply).
- `runsible-pull init` to generate a starter `pull.toml` plus a matching `runsible-pull@.service` unit file the operator can drop into `/etc/systemd/system/`.
- `runsible-pull status` to read and pretty-print the local heartbeat.

**Out:**
- Push-mode execution. That is `runsible-playbook`'s job; this binary is local-only at apply time.
- Fleet-wide reporting / aggregation / dashboards. Receiving heartbeats and rolling them up is a separate product (a control plane); `runsible-pull` only emits.
- Orchestration of multiple hosts in concert. Cross-host order, batch sizing, dependency between hosts: those are push-mode concerns.
- Key management for the signing keys that produce the trusted bundles. The operator's signing-side workflow is adjacent (`runsible-vault` recipients) and lives in that crate.
- A web UI of any kind. Operators run `runsible-pull status` or read the heartbeat JSON from their existing tooling.
- Plugin-loadable fetchers. v1 ships with the three source types fixed in-tree; new sources land via PR, not via runtime extension.

## 3. Operating modes

Three modes, on purpose: P2 MSPs and P3 compliance teams split on operational philosophy.

- **One-shot.** `runsible-pull --once --url ... --ref ... --playbook ...` — fetch, apply, write heartbeat, exit. Useful for debugging and CI smoke-tests.
- **Daemon.** `runsible-pull --config /etc/runsible/pull.toml` — long-running with internal scheduler. SIGHUP reloads, SIGTERM finishes the current cycle and exits.
- **Timer.** A systemd timer (or cron) calls `runsible-pull --once --config <path>` per `OnUnitActiveSec`. The internal scheduler is bypassed.

**Recommended for a 100-host MSP fleet: systemd-timer mode, one timer per tenant per host.** Daemon mode is technically superior (lower per-cycle overhead, finer scheduler), but timer mode wins on operational legibility — `systemctl list-timers` enumerates everything, `journalctl -u runsible-pull@cust-acme.service` is one command per tenant, and a stuck cycle does not cascade into a stuck daemon. Daemon mode becomes the right call once a tenant exceeds ~500 hosts and per-cycle process fork/exec is measurable.

## 4. The pull config (`/etc/runsible/pull.toml`)

The config is TOML-only. Per `06-configuration-reference.md` §1 we explicitly do **not** support the YAML/INI legacy of `ansible.cfg`. The schema is small and deliberately flat where it can be.

```toml
# /etc/runsible/pull-cust-acme.toml — one file per tenant

[source]
type = "git"                                            # "git" | "https" | "s3"
url  = "https://gitlab.example.com/inframgmt/cust-acme/playbooks.git"
ref  = "main"                                           # branch | tag | sha
shallow = true
submodules = "checkout"                                 # "ignore" | "checkout" | "recursive"

[source.auth]
type = "ssh-key"                                        # "ssh-key" | "token" | "iam" | "none"
path = "/etc/runsible/pull.key"

[schedule]
interval = "10m"                                        # ignored under --once
jitter   = "30s"                                        # uniformly distributed [0, jitter]
catchup  = "skip"                                       # "skip" | "run-once"
on_boot  = { delay = "2m" }

[apply]
playbook = "site.toml"
inventory = "inventory/local.toml"                      # defaults to synthetic localhost
extra_vars_from = ["/etc/runsible/host.vars.toml"]
tags = []
skip_tags = []
check = false
diff = true
on_failure = "log"                                      # "log" | "exit" | "page"
hold_apply_lock_for = "30m"

[heartbeat]
local = "/var/lib/runsible/cust-acme/heartbeat.json"
local_log = "/var/lib/runsible/cust-acme/cycles.ndjson"
log_rotate_at = "16MiB"

[heartbeat.http]
url = "https://control.example.com/heartbeat"
auth.type = "bearer"                                    # "bearer" | "mtls" | "none"
auth.token_file = "/etc/runsible/heartbeat.token"
include_run_summary = true
timeout = "5s"
retry = { attempts = 3, backoff = "exponential", max = "30s" }
on_failure = "queue"                                    # "queue" | "drop"

[verify]
signature_required = true
trusted_keys = [
    "/etc/runsible/control-plane.pub",
    "/etc/runsible/control-plane.backup.pub",           # multi-key for rotation
]
signature_path = ".runsible.sig"
algorithm = "ed25519"                                   # "ed25519" | "minisign" | "ssh"
fail_action = "halt"                                    # "halt" | "page"

[isolation]
tenant = "cust-acme"
state_dir = "/var/lib/runsible/cust-acme"
run_as_user = "runsible-pull-cust-acme"
unshare_namespaces = ["mount", "uts"]
```

Only `[source]`, `[apply].playbook`, and `[heartbeat].local` are required; the rest defaults sensibly.

### 4.1 Why TOML, why one file per tenant

Per §1 of `11-poor-decisions.md` we ban YAML at the source-of-truth layer; TOML's typed scalars eliminate the `gather_facts: yes` / `archive: no` class of incident. One file per tenant is not a TOML constraint — it is operational hygiene: the file is the file-mode boundary, the systemd unit boundary, the journald grep boundary, and the rsync boundary on tenant offboarding.

## 5. Heartbeat artifact

The heartbeat is the single most important user-visible artifact this binary produces. It is what monitoring scrapes, what billing pipelines tail, and what a P3 compliance auditor reads as evidence that a host applied its baseline. Schema:

```json
{
  "schema": "runsible.heartbeat.v1",
  "host": "web03.acme.example.com",
  "tenant": "cust-acme",
  "runsible_version": "0.4.2",
  "config_path": "/etc/runsible/pull-cust-acme.toml",
  "config_hash": "sha256:6d4a...c2f1",
  "source": {
    "type": "git",
    "url": "https://gitlab.example.com/inframgmt/cust-acme/playbooks.git",
    "ref": "main",
    "resolved_sha": "9f3e8a1c0d4b...",
    "fetched_at": "2026-04-26T14:03:22Z"
  },
  "verify":     { "required": true, "outcome": "ok", "key_id": "ed25519:control-plane-2026-04" },
  "last_fetch": { "started_at": "...", "ended_at": "...", "outcome": "ok", "bytes": 482133, "no_change": false },
  "last_apply": {
    "started_at": "...", "ended_at": "...",
    "outcome": "changed",
    "playbook": "site.toml",
    "playbook_hash": "sha256:c8e7...91a0",
    "summary": { "tasks_total": 42, "tasks_ok": 37, "tasks_changed": 5, "tasks_failed": 0, "tasks_skipped": 0, "handlers_run": 2 }
  },
  "next_scheduled": "2026-04-26T14:13:18Z",
  "previous_outcomes": [
    { "ended_at": "...", "outcome": "ok"      },
    { "ended_at": "...", "outcome": "changed" }
  ]
}
```

Outcome values: `ok`, `changed`, `failed`, `fetch-failed`, `verify-failed`, `skipped`.

Updates are atomic: write `<path>.tmp.<pid>`, fsync, `rename(2)`. A reader sees either the previous full document or the new one, never a torn write. Operators tail with `jq`, scrape via Prometheus textfile collector, or rsync mid-cycle without coordination.

The companion `cycles.ndjson` log is append-only NDJSON, one event per cycle, schema-compatible with the `runsible.event.v1` envelope from `runsible-playbook`. This is what a SIEM ingests.

## 6. Tenant isolation (MSPs)

The MSP segment is the wedge — every choice here is downstream of "30 tenants, none can see another's secrets, none can stall another's cycle, none can blow up a shared daemon."

- **One config file per tenant.** `/etc/runsible/pull-<tenant>.toml`, mode `0640`, owned by the tenant's service account group.
- **One systemd unit per tenant.** `runsible-pull@<tenant>.service` is templated; `runsible-pull init --tenant cust-acme` generates the config and the unit. Each unit has its own `User=` / `Group=`.
- **One state directory per tenant.** `/var/lib/runsible/<tenant>/` holds heartbeat, cycles log, bundle cache, apply lock. Mode `0750`. Cross-tenant read requires root.
- **One vault recipient per tenant.** Per §6 of `11-poor-decisions.md` we use `runsible-vault` age/SSH recipients, not a shared password. The tenant's key lives at `/etc/runsible/pull-<tenant>.age`, readable only by that tenant's account. A bad playbook from tenant A cannot decrypt tenant B's secrets.
- **Opt-in Linux namespace isolation.** `[isolation].unshare_namespaces = ["mount", "uts"]` for apply steps that should not mutate global state.
- **No shared in-process state.** Daemon mode is per-tenant, not multi-tenant; a crashed tenant restarts via `Restart=on-failure` without disturbing peers.

A 30-tenant host runs 30 units, 30 configs, 30 service accounts, 30 heartbeats. `runsible-pull status --all-tenants` walks `/var/lib/runsible/*/heartbeat.json` and prints a one-line summary per tenant.

## 7. CLI surface

The CLI is deliberately small; operational levers live in the config (deployment model is "drop and forget"). The CLI is for the moments operators are *not* forgetting.

| Invocation | Purpose |
|---|---|
| `runsible-pull --config <path>` | Daemon mode; internal scheduler; SIGHUP reloads, SIGTERM exits. |
| `runsible-pull --once [--config <path>]` | Single cycle; bypasses internal scheduler. |
| `runsible-pull --once --url <r> [--ref <r>] [--playbook <p>]` | Inline overrides for CI / debugging. |
| `runsible-pull status [--config <path>] [--all-tenants]` | Pretty-print local heartbeat(s). |
| `runsible-pull init [--tenant <t>] [--source-url <u>] [--out-dir <d>]` | Generate starter config + systemd unit. |
| `runsible-pull verify --config <path>` | Fetch + signature-check only. |
| `runsible-pull doctor --config <path>` | Pre-flight: DNS, key perms, heartbeat path writable, vault key readable, unit installed. |
| `--dry-run` | Fetch + verify, do not apply. |
| `--verbose, -v` / `--quiet, -q` / `--json` | Repeatable verbosity, quiet, force NDJSON output. |
| `--no-heartbeat-http` | Suppress HTTP POST for one invocation. |
| `--apply-now` (SIGUSR1) | In daemon mode, trigger an immediate cycle and reset the schedule. |

`-C` is **not** `--check` here, and **not** `--checkout` either. Neither short flag exists. Source ref is `--ref` or `[source].ref`; check mode is `[apply].check`. This is a deliberate fix for the §9 quirk in `01-cli-surface.md`; `doctor` catches operators ported from `ansible-pull` with a remediation hint.

### 7.1 Exit codes

`0` clean (`ok`/`changed`/`skipped`); `2` apply failed; `3` fetch failed; `4` signature failed; `5` config error; `6` lock contention; `64` `EX_USAGE`; `99` SIGINT. Stricter and more granular than Ansible's empirical scheme — monitoring can distinguish `fetch-failed` from `apply-failed` without parsing logs.

## 8. Drift / signature handling

Per §7 of `11-poor-decisions.md` the original `ansible-pull` does no signature verification. `--verify-commit` (for git only, GPG-only, controller-trust-only) is the entire story. We do better.

### 8.1 Signature verification

- The fetched bundle is expected to contain a detached signature at `[verify].signature_path` (default `.runsible.sig`). The signature is over a deterministic hash of the entire bundle excluding the signature file itself.
- Supported algorithms: `ed25519` (raw libsodium-style), `minisign` (the upstream tool's format), and `ssh` (signed via `ssh-keygen -Y sign`). The operator picks one per fleet and configures `[verify].algorithm`.
- `[verify].trusted_keys` accepts a list so that key rotation is graceful: roll out the new public key first, sign with the new key, retire the old key. `runsible-pull` accepts a signature from any listed key.
- A failed signature halts the cycle. The fetched bundle is moved to `<state_dir>/quarantine/<sha>` for forensics and the heartbeat records `verify.outcome = "failed"`. The previous successful bundle is **not** re-applied — that would mask compromise.
- The signature step happens **before** any code from the bundle runs. We do not trust the bundle to tell us its own signature path; that path comes from the operator-managed config.

### 8.2 Drift recording

- The heartbeat records the SHA-256 of the playbook file actually executed, plus the resolved SHA of the source ref (the git commit, the HTTPS bundle's content hash, or the S3 object's ETag).
- An operator can correlate `last_apply.playbook_hash` across the fleet and flag any host whose hash is stale.
- A separate drift-report mode (`runsible-pull verify --config <path> --diff-against-last-apply`) does a fetch + plan-only run and emits a JSON diff between what is currently on the host and what the new bundle would change. This is the "what would tonight's run do?" feature operators ask for.

### 8.3 Separation of fetch and apply

`ansible-pull` is one phase: fetch then immediately apply, with no way to retry an apply of a known-good fetch. We split:

1. **Fetch phase** produces a verified bundle on disk under `<state_dir>/bundles/<sha>/`.
2. **Apply phase** invokes the playbook from the bundle directory.

`[apply].on_failure` controls retry: `"log"` (default — re-fetch + re-apply on next interval), `"exit"` (let systemd restart), `"page"` (fire pager hook + continue). A v1.1 `retry-apply --bundle-sha <sha>` re-applies a cached bundle without re-fetching — useful when the apply failed for a transient cause the operator has manually fixed.

## 9. Redesigns vs Ansible

This crate exists primarily because `ansible-pull` is the worst-designed binary in the Ansible portfolio for the audience it most badly serves. Per §7 of `11-poor-decisions.md`:

| Ansible misdesign | runsible-pull redesign |
|---|---|
| No built-in scheduling — users wire to cron. | First-class `[schedule].interval` + `jitter`; daemon mode recommended, timer mode supported. |
| No heartbeat. Hosts silently stop pulling. | Atomic `heartbeat.json`, optional HTTP POST, append-only `cycles.ndjson`. |
| No drift report. Apply and forget. | Heartbeat records playbook hash + resolved source SHA + per-task summary; `verify --diff-against-last-apply` for planning. |
| Fetch + apply are one phase; partial failure forces re-fetch. | Separated phases; cached bundles under `<state_dir>/bundles/<sha>/`; `retry-apply --bundle-sha`. |
| `--verify-commit` is git/GPG-only, trusts the controller keyring. | First-class signature verification with per-fleet trusted-keys list, algorithm choice, multi-key rotation. |
| ~5-30s cold start per cycle. | Static Rust binary, sub-100ms cold start, daemon mode amortizes everything. |
| `-C` means `--checkout` here but `--check` everywhere else; `-m` is the VCS module. | No `-C` short flag at all; source ref is `[source].ref` or `--ref`; `doctor` catches operators ported from `ansible-pull`. |
| Multi-tenant story is "run multiple cron jobs and pray." | One config / unit / state dir / vault recipient per tenant; `init --tenant` scaffolds. |
| Vault is a shared symmetric password; MSPs cannot isolate tenant secrets. | Per-tenant `runsible-vault` recipients; tenant A cannot decrypt tenant B's secrets even on the same host. |

`runsible-pull` also inherits the cross-cutting redesigns: TOML config (vs. INI/YAML), NDJSON output (vs. text streams), typed exit codes, and the typed module trait via `runsible-playbook`.

## 10. Milestones

### M0 — Local pull, unsigned, one-shot

Fetch from a `git` source (HTTPS or SSH key auth); spawn `runsible-playbook` against the fetched bundle; write `heartbeat.json` atomically; `--once` only (no daemon, no HTTP, no signing); `runsible-pull status` reads the local heartbeat. **Acceptance:** one host, one repo, one TOML playbook, end-to-end run produces a valid heartbeat.

### M1 — Daemon, scheduling, HTTP heartbeat

Daemon mode with internal scheduler + jitter; HTTP POST with bearer auth, retries, queue-on-failure; rotating cycles NDJSON log; `doctor` and `init` (single-tenant); `https` and `s3` source types. **Acceptance:** 24h daemon run posting every 10m, gracefully handling at least one HTTP failure.

### M2 — Signed pulls + tenant isolation + systemd unit generator

Signature verification (`ed25519`, `minisign`, `ssh`) with multi-key trust + quarantine; multi-tenant configs/units/state-dirs/vault recipients; `init --tenant <t>` scaffolds config + unit + sudoers hints; `status --all-tenants`; opt-in namespace isolation. **Acceptance:** 3-tenant test harness on one VM with three timers, one bad signature, one fetch failure, one apply failure across tenants — heartbeats reflect each, no cross-tenant leakage.

### M3 — Drift planning, retry-apply, pager hook

`verify --diff-against-last-apply` for a JSON diff; `retry-apply --bundle-sha <sha>`; `[apply].on_failure = "page"` exec hook; optional Prometheus metrics (see §14).

## 11. Dependencies on other crates

- **`runsible-playbook`** — the apply step. Spawned as a binary by default (process isolation); invoked as a library under the `embed` feature for low-overhead daemon mode. Contract is the playbook's typed event stream and exit code.
- **`runsible-config`** — owns the TOML schema. `pull.toml` registers its `[source]`, `[schedule]`, `[apply]`, `[heartbeat]`, `[verify]`, `[isolation]` sections with the central validator.
- **`runsible-vault`** — decrypts `extra_vars_from` paths with the per-tenant recipient key. No crypto in this crate.
- **`runsible-galaxy`** — if the bundle references packages, run `runsible-galaxy install --lockfile <bundle>/runsible.lock` after fetch and before apply (lockfile-driven so the apply is reproducible against the signature-verified bundle).
- **`runsible-inventory`** — synthetic `localhost` when `[apply].inventory` is unset; otherwise parses `inventory/local.toml`.
- **`runsible-connection`** — only the `local` variant by default; no SSH machinery in apply.
- **`runsible-builtin`** — transitive via `runsible-playbook`.

## 12. Tests

Acceptance tests are gated to the milestones in §10.

### 12.1 Unit

- Heartbeat serialize → deserialize round-trip; per-field schema check.
- Atomic write: kill between `write_tmp` and `rename`; previous heartbeat must remain intact.
- Scheduler: with a fake clock and `interval = 10m, jitter = 30s`, the next-cycle distribution is uniform within the window.
- Signature verify: positive (good sig, trusted key) and negatives (no sig, wrong key, tampered bundle, unknown algorithm) — each produces a distinct error variant.
- Tenant isolation: two configs with same `[source].url` but different `[isolation].tenant` produce non-overlapping state dirs.
- Config parsing: ten malformed configs each produce a precise error at the offending key.

### 12.2 Integration

- **Git fetch + apply round-trip.** A `gix`-served bare repo in a tempdir with a tiny `site.toml`; `runsible-pull --once --url file://...`; heartbeat exists, exit 0.
- **HTTP POST.** An `axum` mock server in-process; two cycles, two POSTs received; kill the server mid-cycle; queue-on-failure preserves the heartbeat.
- **Signature positive + negative.** Sign a bundle with ed25519; correct trust list → apply runs. Mutate one byte → apply halts, bundle quarantined.
- **Multi-tenant isolation.** Two configs, two daemons; heartbeats don't collide, vault key files aren't cross-readable, an Acme failure doesn't touch Globex.
- **SIGHUP reload.** Daemon running; edit `[schedule].interval`; SIGHUP; next cycle fires on the new interval.
- **SIGTERM finishes cycle.** Daemon mid-apply; SIGTERM; apply completes, heartbeat updated, exit 0.
- **`doctor` end-to-end.** Broken config (unreadable key, unwritable heartbeat path, unreachable source); report enumerates each.
- **Lock contention.** Second daemon on same state_dir exits with code 6 in <1s.

### 12.3 Adversarial

- Bundle expansion bomb (cap via `[source].max_bundle_size`).
- Tarball path traversal (`../../etc/passwd`) — fetcher rejects.
- Signature replay (valid sig on bundle A applied to bundle B) — verify over bundle hash, not presence.
- Huge response from heartbeat HTTP endpoint — daemon does not OOM.

## 13. Risks

Pull-mode is operationally elegant but has failure classes push-mode does not.

- **Self-update cycle wedge.** If a bad `runsible-pull` is rolled out and the new binary cannot pull, the fleet stops pulling — including the pull that would deliver the fix. Mitigation: a "previous good binary" fallback. Upgrades land via the playbook itself: copy new binary to `/usr/bin/runsible-pull.next`, smoke-test `--version`, atomic-rename, keep the old as `.previous`. A wrapper in `ExecStartPre=` rolls back if `--version` fails.
- **Heartbeat HTTP endpoint is a fleet attack surface.** Every host POSTs to the same URL. A compromised host can flood, or impersonate others. Mitigation: per-host bearer tokens issued at provision time; mTLS for compliance fleets; the endpoint contract requires the control plane to rate-limit and reject heartbeats whose `host` field does not match the auth principal.
- **Signed bundles require operator key management.** Out of scope for this crate (lives in `runsible-vault`) but the multi-key trust list is in M2 specifically so rotation works. If the operator loses the signing key with `signature_required = true`, the fleet stops applying until a new signed bundle is published. Doc must call this out; `fail_action = "halt"` must never silently downgrade trust.
- **Quarantine dir grows unbounded.** A `[verify].quarantine_max = "1GiB"` budget with FIFO eviction.
- **Time skew.** A host with a bad clock misreports timestamps and may fail signatures that embed timestamps. Doctor checks NTP; daemon refuses to start if `chrony`/`systemd-timesyncd` is dead and `[schedule].require_clock_sync = true`.
- **Stale apply lock.** flock(2) on `<state_dir>/.apply.lock` is released on process exit. Startup scrubs `heartbeat.json.tmp.*` older than 1h.
- **Systemd timer drift on long-uptime hosts.** Doc recommends `RandomizedDelaySec` plus daemon mode for hosts with multi-year uptime.
- **MSP tenant onboarding is still manual.** `init --tenant` scaffolds the config but does not provision the service account, vault recipient, or source repo. A future `runsible-pull onboard` is the natural v1.1 follow-up.

## 14. Open questions

Choices we have deliberately not locked in; want MSP feedback first.

- **"Force pull now" webhook?** Either (a) the daemon embeds a tiny HTTP server on a port + token, or (b) the control plane SSHes and `pkill -SIGUSR1`. (b) avoids a new attack surface but requires SSH the pull-mode customer often avoids. Lean (a), feature-gated behind `[control].listen_addr` (default off).
- **Run as root or as a dedicated user?** Apply often needs root. Lean "dedicated user, document sudoers, ship `init --tenant ... --emit-sudoers`."
- **Prometheus metrics?** Cycle latency, fetch bytes, task counts, signature outcomes. Lean "yes, default off, opt in via `[metrics].listen_addr`, prefer systemd socket activation for credentialed scrape."
- **`[apply].on_failure = "page"` semantics.** Probably `exec_cmd` with documented env vars (`RUNSIBLE_PULL_TENANT`, `RUNSIBLE_PULL_OUTCOME`, `RUNSIBLE_PULL_SUMMARY_JSON`).
- **Bundle cache TTL.** Default: keep last 5, plus the most recent successful apply regardless of count.
- **Does `status` round-trip the HTTP endpoint?** Default local-only; `--check-control-plane` opts in.
- **Apply-diff preview workflow.** After M3, a P3 compliance feature: pre-compute the change set, require operator approval before apply. Defer to v1.1, gate on demand.
