# runsible — User Story Analysis
## Engagement Memo, Cochran Block × [Simulated Elite Consultancy], 2026-04-26

> **Frame.** This document is what an elite practice would deliver to runsible if it were a paying client engagement, not what an academic survey would say. We ignore PR-friendly framings and tell you where the money is, where the risk is, and what the next four moves are. We do not flatter, we do not hedge, and we do not list every Ansible feature — we list the ones that map to dollars, hours saved, or production incidents avoided.

---

## 1. Why this matters at all

Ansible is the default config-management tool for the long tail of mid-market infra, MSPs, government & defense automation, ed-tech, fintech build-out shops, and small platform teams at SaaS companies between 50 and 5,000 hosts. Its market dominance is *despite* a large catalog of design pain, not because of it. The pain is paid, daily, in:

- **Cold start cost.** A fresh `ansible-playbook` invocation pays the Python interpreter + module imports + Jinja2 boot + SSH multiplex setup tax on *every* run. On a 100-host fleet, the controller is the bottleneck before the network is.
- **Failure-at-apply.** Variable typos, precedence surprises, and module signature errors surface on host #47 of 100, ten minutes into a run. Recovery is manual.
- **YAML quirks shipped to prod.** `yes`/`no` becoming bool, `01:30` becoming seconds, and `:` in unquoted strings continue to bite ten years after they were named.
- **Plugin sprawl.** Connection plugins, become plugins, lookup plugins, callback plugins, action plugins, vars plugins, inventory plugins, cache plugins, filter plugins, test plugins, module plugins. The orthogonality is theoretical; in practice every plugin axis is a Python ABI risk surface.
- **Galaxy + Roles + Collections.** Three overlapping reuse mechanisms with three different lifecycle models. The "right" answer depends on the year you started learning Ansible.
- **Vault UX.** A symmetric password file is the default secret model in 2026. Teams routinely commit the file or share it on Slack.

A Rust+TOML reimagining is not interesting because Rust is fashionable. It is interesting because the controller is a hot path that has been calcified in Python for fifteen years, and because TOML eliminates an entire class of YAML production incidents. **The competitive moat is performance + safety, not feature parity.**

---

## 2. Personas — who actually pays for this to exist

We rank by addressable hours saved per year per seat. We are not optimizing for vibes.

### P1 · Platform engineer at a 200-2,000-host SaaS or fintech (highest-leverage segment)

- **Daily life:** runs CI-driven Ansible from GitHub Actions or self-hosted runners against staging + prod. Cares about minute-level wall-clock on deploys because a roll forward is also a roll back, and the team Slack waits on it. 50% of their playbooks call `command:` or `shell:` because their internal modules predate or postdate the official ones.
- **Pain ranked:**
  1. Cold start — controller + Jinja + Python per playbook is ~15-30s before any host is touched. Gates rollback speed.
  2. No structured output for CI dashboards. Currently they screen-scrape JSON callback plugin into Datadog.
  3. Variable-precedence rules confuse the on-call rotation; nobody recites all 22 levels.
  4. Vault password lives in a file or in a CI secret. Teams of >5 cannot rotate without a runbook.
  5. `--check` mode is unreliable per module; the SRE team doesn't trust it.
- **What they will pay for:** sub-second cold start; native JSON output stream; a deterministic plan-then-apply model with per-host preview; multi-recipient secrets that don't require sharing one password.
- **Switching cost:** medium-high. ~10k LOC of YAML to port. **The yaml2toml crate is the bridge into this segment.** If conversion is one-shot and lossless on 95% of real playbooks, runsible has a hand to play here.

### P2 · MSP / IT services automation engineer (highest-margin segment)

- **Daily life:** runs Ansible against many tenants — 30 small networks, none larger than ~50 hosts. Tenant isolation, key isolation, audit trail, and onboarding new tenants in under an hour are existential.
- **Pain ranked:**
  1. Tenant secrets bleed risk — vault password files do not naturally partition by client.
  2. Inventory hygiene across 30 customers is a clerical job.
  3. ansible-pull is the right model but its UX is afterthought-grade (no built-in scheduling, no health surface).
  4. Reporting back to the customer is bespoke per tenant.
- **What they will pay for:** first-class multi-tenant inventory + secrets, self-updating ansible-pull replacement with a heartbeat surface, a report.json artifact per run that a billing pipeline can consume.
- **Switching cost:** low if `runsible-pull` actually ships before parity-everywhere. **MSPs will be early adopters because their pain on the secret-management axis is acute and Ansible is not fixing it.**

### P3 · Compliance & hardening engineer (defense, gov, regulated finance)

- **Daily life:** runs CIS / STIG / DISA SCAP-derived playbooks against thousands of hosts; output goes into compliance evidence packs.
- **Pain ranked:**
  1. Need a *signed* run record. Ansible's audit story is "tail the log."
  2. `--check` fidelity matters for ATO submission — drift detection without remediation is the whole product.
  3. Idempotency is asserted but not proven; second-run-no-op is convention, not type-checked.
- **What they will pay for:** machine-signed plan + apply records (cryptographic proof of which controller ran what, on which hosts, against which playbook hash); deterministic plan format suitable for diffing across runs; a dry-run mode whose output is type-equivalent to the apply output.
- **Switching cost:** high but the budget is unusually large per seat. **Niche, but a beachhead — landing one DoD subcontractor pays for a substantial chunk of the project's runway.**

### P4 · Solo sysadmin / homelab / freelance DevOps (long tail, low margin)

- **Daily life:** ~1-20 hosts, lots of ad-hoc work, occasional playbook. Loves the "I know what I'm doing" tools.
- **Pain ranked:**
  1. `ansible all -m shell -a 'uptime'` cold-start latency is annoying enough to drive them to fabric/pyinfra/just/scripts.
  2. ansible-galaxy install spam to ~/.ansible/collections is invasive.
  3. Editor support for YAML+Jinja is a kitchen of LSPs that don't agree.
- **What they will pay for:** speed; a single static binary; sane defaults (no ~/.ansible/ unless asked); a TOML editor experience that feels like Cargo.
- **Switching cost:** trivial. **This persona is the one that will write the blog posts that drive adoption among P1 and P2.**

### P5 · The "I just want to bootstrap a k8s node" engineer

- **Daily life:** Ansible is a means to an end — bootstrap a fresh VM through cloud-init's tail end, install kubeadm, hand off. Hates that this requires learning roles + collections + vault + galaxy.
- **What they will pay for:** a one-file declarative playbook that runs locally with `runsible-playbook bootstrap.toml` and disappears. **No roles, no collections, no Galaxy, no vault — just one TOML and a binary.**
- **Switching cost:** trivial.

### Personas explicitly out of scope (per project direction — "skip the plugins for now")

- Network engineers using `cisco.ios.*` / `arista.eos.*` etc. Their workflow is overwhelmingly plugin-mediated and the value runsible can deliver is gated on later collection-plugin support.
- Windows-fleet operators relying on the WinRM / win_* module pipeline. Same gating issue.

---

## 3. Jobs to be Done — what the user is hiring runsible for

Phrased as JTBD ("when ___, I want to ___, so I can ___"):

| # | When | I want to | So I can |
|---|------|-----------|----------|
| J1 | I'm bringing up a fresh host | declare its desired state in one file | reach a known-good baseline without scripting it |
| J2 | I'm rolling out a change to a fleet | preview exactly what will change per host | catch unintended diffs before they ship |
| J3 | A run failed | get a structured, ranked failure report | open the right ticket against the right host without grepping logs |
| J4 | I'm holding secrets | encrypt at rest with per-team key access | rotate without coordinating a password change with everyone |
| J5 | I'm composing reusable automation | bundle modules+vars+templates into a unit | share with my team without telling them which YAML quirk to avoid |
| J6 | I'm running from CI | get a machine-readable artifact of what happened | feed it into our deploy dashboard / SIEM |
| J7 | I'm running ad-hoc | pay zero start-up cost | use the tool the way I use `ssh` |
| J8 | I'm onboarding | learn one syntax | not also learn YAML's pitfalls and Jinja's |
| J9 | I'm scaling out | parallelize without configuration | use my hardware |
| J10 | I'm running idempotently | trust that the second run is a no-op | sleep through the night |

These ten JTBDs are the first-tier acceptance test for the v1 product. If the v1 release post can't honestly claim runsible nails 8 of 10, do not ship.

---

## 4. Where Ansible is winning that runsible must NOT lose

Cold-eyed: Ansible has earned its dominance on real things, not just inertia. We list them so runsible doesn't reinvent its mistakes by ignoring its strengths.

1. **Ad-hoc + playbook in the same tool.** `ansible -m ping all` and `ansible-playbook site.yml` use the same inventory, same modules, same auth. runsible's `runsible` (ad-hoc) and `runsible-playbook` must do the same.
2. **Module names that read like English.** `apt`, `service`, `copy`, `template`, `file`, `user`. We do not invent new vocabulary just to be different.
3. **Inventory grouping is *good*.** Ansible's group nesting + group_vars/host_vars is one of its most-loved features. Preserve the model; switch the file format.
4. **Roles as a unit of reuse.** Even though "role vs collection" is a mess, the *role concept* (defaults + tasks + handlers + templates + files in one folder) is a winning abstraction.
5. **Idempotent module library.** The `ansible.builtin.*` modules' API shape (named params, returns `changed` + `failed`, supports check mode) is the industry standard. Mirror it.
6. **Single-binary CLI surface from the user's POV.** `ansible-*` are 12 binaries but they share inventory + config + vault. runsible must preserve this seamlessness even though we ship 13 binaries.

---

## 5. Where Ansible is losing — runsible's wedge

Ranked by addressable user pain × redesign feasibility:

| # | Ansible misdesign | Cost to user today | runsible redesign | Difficulty |
|---|---|---|---|---|
| 1 | Python cold start (~5-30s) | Every CI run | Static Rust binary, <100ms cold start | Free (architecture choice) |
| 2 | YAML type ambiguity | Production incidents | TOML-only as canonical; YAML import-only via yaml2toml | Free |
| 3 | Plan/apply not first-class | --check is unreliable | Plan is a real artifact; apply consumes a plan | Medium |
| 4 | 22-level variable precedence | On-call confusion | Collapse to ~5 documented levels with explicit override syntax | High (compatibility) |
| 5 | Vault is symmetric file password | Teams of >5 can't rotate | Per-recipient keys (age/SSH), file fallback for transition | Medium |
| 6 | Roles vs Collections vs Playbooks | Onboarding tax | One concept: "package" — roles are packages, collections are packages, playbooks are entry points | High (porting) |
| 7 | No compile-time check on playbooks | Typo on host #47 | Parse + type-check the whole project before any host is touched | Medium |
| 8 | Output is unstructured stream | CI scrapers everywhere | NDJSON by default; pretty-print only on TTY | Free |
| 9 | Connection plugins reinvent SSH | ControlPersist tuning hell | Use system openssh + ControlMaster; russh as fallback for embedded use | Low |
| 10 | Idempotency is convention | "Second run no-op" is a hope | Module trait with `plan() -> Plan` and `apply(plan) -> Outcome`; planner verifies plan after apply | Medium |
| 11 | Galaxy resolution is single-pass | Conflicts surface at install | Real SAT-based resolver; lockfile (`runsible.lock`) | Medium |
| 12 | ansible-lint is a third-party project | Style drift | runsible-lint is first-party, shares the parser with runsible-playbook | Free |
| 13 | Fact gathering is opt-out, full-fat by default | 80% of users only need 5% of facts | Lazy facts: gather what the play actually references, use a `facts.required = [...]` declaration | Medium |
| 14 | `set_fact` is mutable, lazily scoped | Subtle bugs | `set_fact` becomes shadowing in a child scope; mutation requires `set_fact !` (deliberate annotation) | High (semantics break) |
| 15 | No project lockfile for collections | Reproducibility hole | `runsible.lock` for all dependencies | Free |

The first 10 are the v1 wedge. The rest are v1.1+ if the v1 lands.

---

## 6. Release stages (no schedule, just ordering of capability)

### Alpha
- `runsible-playbook` runs a TOML playbook against an inventory using SSH connection + sudo become + the smallest useful module set (`command`, `shell`, `copy`, `template`, `file`, `package`, `service`, `debug`, `set_fact`, `assert`).
- `runsible-inventory` parses TOML inventories + INI Ansible inventories (read-only).
- `yaml2toml` round-trips a real-world Ansible playbook (e.g., a published role from Galaxy) without losing semantics.
- `runsible` (ad-hoc) runs single-module commands against inventory patterns.
- The ten JTBDs above are passing on at least 6 of 10.

### 1.0
- Vault, galaxy, vars precedence (collapsed model), full builtin-module set, plan-then-apply, NDJSON output, lockfile, lint as first-party, doc generator from module sources.
- `runsible-pull` ships and an MSP can put it on a fleet.
- `runsible-test` runs sanity + integration tests for a runsible package.
- Compatibility: yaml2toml round-trips ≥80% of public Ansible content.

### Post-1.0
- Plugin equivalents for the platforms users actually need (cloud, k8s, container — *not* networking unless that segment shows up). These are loadable as `.so` / `.dylib` / `.dll` Rust dynamic libs with a stable C ABI, not Python files.
- A signed audit trail story for compliance (P3 segment).
- A web UI for on-prem MSP control planes (P2).

---

## 7. Where to spend, where to cut

### Spend
- **yaml2toml correctness on real corpora.** This is the bridge into the existing user base. If it fails on `geerlingguy.docker` the project is dead at adoption.
- **Cold-start performance.** Benchmark every commit. If cold-start regresses past 200ms it's a release blocker.
- **Plan format + diff UX.** This is where runsible *visibly* beats Ansible.
- **Vault redesign.** age + SSH-key recipients, with a one-command migration from existing vault files.

### Cut, ruthlessly
- Plugin compatibility with Python collections. Do not even try. They are out of scope for v1, period.
- Network device modules. Out of scope.
- Windows. Defer to v1.5.
- Fancy CLI UX (TUI, progress bars beyond a status line). Engineering hours wasted on cosmetics.
- A web UI in v1. Wait for users to ask.
- "Ansible Tower" equivalent. Different product.

---

## 8. Risk register (this is the part the consultant earns their fee on)

| # | Risk | Probability | Impact | Mitigation |
|---|---|---|---|---|
| R1 | Ansible community treats runsible as a "fork that won't" and ignores it | High | Medium | Do not pitch as fork. Pitch as a controller for users who already write playbooks but want Rust speeds. yaml2toml is the olive branch. |
| R2 | yaml2toml is incorrect on real-world content; reputation damage | Medium-high | High | Build a corpus harness against the top 200 Galaxy roles + the top 50 published collections (excluding plugin-heavy ones); CI fails if any regress. |
| R11 | License incompatibility on imported corpus — relicensing GPL/Apache content under runsible's Unlicense would be a license violation | High if not designed for | Existential (legal + reputational) | The import tool refuses to convert without a detected upstream license; converted packages inherit the original license verbatim; the registry surfaces per-package license + provenance; `LICENSE` / `NOTICE` files are copied; runsible **never** relicenses third-party content. The runsible workspace itself stays Unlicense; the *imported packages* keep their originals. |
| R3 | The 22-level precedence collapse breaks playbooks that depended on a specific layer | Medium | Medium | Provide a `--precedence-compat ansible` mode that emulates the original layering for one major version, then deprecate. |
| R4 | Connection performance does not actually beat OpenSSH ControlPersist | Low-medium | High | Default to system ssh; russh is a fallback for embedded contexts. Don't promise faster SSH; promise faster controller. |
| R5 | Galaxy/collection compatibility expectation drags scope | High | High | State explicitly that collections require a runsible-native re-package. Ship a `runsible-galaxy import` that walks a Python collection and produces a runsible package skeleton + a "TODO" list of plugins that need porting. |
| R6 | The bundled crate names attract typo-squatters or a parallel project | Now mitigated | — | Done — all 13 names are reserved. |
| R7 | Single-maintainer bus factor | Project-existential | High | Document the architecture early and exhaustively. Master plan + per-crate plans serve this. |
| R8 | Rust ecosystem dependencies (openssh-rs, russh, age, tera/minijinja) churn | Medium | Medium | Pin major versions; have a "supported deps" matrix; budget a quarter per year for upgrades. |

---

## 9. The four moves, in order

1. **Land yaml2toml and an inventory parser.** These are the bridge artifacts. Without them, no user can try runsible against their own playbooks.
2. **Land `runsible-playbook` with a useful 12-module kit.** This is the demo: take a real Ansible playbook, convert it, run it, show controller-time improvement. The launch post writes itself.
3. **Land `runsible-vault` with the age-recipient model.** This is the "we are different" artifact. Pitch it to MSPs (P2) directly.
4. **Land `runsible-pull` + a signed-run-record format.** This opens the P3 (compliance) door and gives the P2 segment its self-update story.

The window to be the obvious successor to Ansible-the-controller will not stay open forever — Konfig, Pkl-based systems, and others are circling.

---

## 10. Closing — what we'd be uncomfortable saying in public but tell you privately

- Ansible's biggest moat is the muscle memory of its CLI surface. Mirror it with religion. Every flag that makes sense on `ansible-playbook` should make sense on `runsible-playbook`. The user must not have to re-learn anything except the data format — and yaml2toml softens that.
- Galaxy/collections are political artifacts. Red Hat owns the platform and is unlikely to accept a Rust-native package format. **Do not depend on Galaxy.** Build `runsible-galaxy` to read collections (one-way import) and to publish to a runsible-native registry. Self-host the registry. The hard fork is here, not in the playbook language.
- The most likely failure mode for runsible is *not* technical — it is community noise. Ten thousand HN comments saying "this is just rewriting Ansible in Rust" will be more painful than any technical bug. The user-story analysis above is your defense: when you ship, lead with what runsible does that Ansible *can't*, not with parity. Cold start, plan/diff, age vault, structured output. In that order.

— end of memo —
