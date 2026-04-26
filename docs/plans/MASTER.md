# runsible — Master Plan
## The ordering, the dependency DAG, the critical path

> **Status.** As of 2026-04-26: 13 crate names reserved on crates.io as v0.0.1 stubs; ~131k words of research, opinion, and per-crate plans authored under `docs/`. No production code yet. This is the synthesis document — read it first if you are joining the project.
>
> **Reading order for newcomers** (do not skip the first three):
> 1. `docs/research/00-user-story-analysis.md` — *why* the project exists at all
> 2. `docs/research/11-poor-decisions.md` — *what* we redesign
> 3. `docs/plans/MASTER.md` — *this file* — the build order
> 4. `docs/plans/<crate>.md` — the crate you intend to work on
> 5. `docs/research/<NN-topic>.md` — the corresponding Ansible reference for context
>
> Everything below assumes you have read items 1–2.

---

## 1. The thesis in one paragraph

Ansible's controller is calcified Python. Its data plane is YAML, with all of YAML's quirks. Its templating is Jinja2, which keeps Python on the controller forever. Its plugin axes are theoretically orthogonal but operationally a maze. Its variable precedence is a 22-level rule that no engineer recites from memory. Its vault is a symmetric password file. Its package story is a three-headed beast: roles vs collections vs playbooks. None of these are *bugs* — they are accumulated design debt that the original team chose, justifiably, in the 2010s, and that nobody at Red Hat is empowered to throw out and start over. **runsible is the empowered redesign.** A static Rust binary controller, a TOML data plane, a Rust-native templating engine, an asymmetric per-recipient vault (age + SSH keys), one package concept, a typed module trait that makes idempotence provable, plan-and-apply as first-class artifacts, and a SAT-based dependency resolver with a real lockfile. Existing Ansible content migrates through `yaml2toml`, which is the bridge that decides whether the project succeeds or dies in adoption.

Everything in this directory is in service of that thesis.

---

## 2. The 14 workspace members

Thirteen are published binaries (the user-facing CLI parity with Ansible). One is an unpublished workspace library (`runsible-core`) that hosts the shared parser, AST, types, errors, and the `Module` trait. Without `runsible-core`, the per-crate plans would each duplicate ~5k lines of code or pull each other in as cyclic dependencies. With it, the dependency DAG is acyclic.

| # | Crate | Status | Plan | One-line mission |
|---|---|---|---|---|
| 0 | **`runsible-core`** *(new, unpublished)* | proposed | implicit in plans | Shared types, parser, AST, errors, `Module` trait, `Connection` trait, NDJSON event schema. Imported by every binary. |
| 1 | `runsible-config` | published v0.0.1 stub | [`runsible-config.md`](runsible-config.md) | Read, validate, dump, explain runsible's TOML config. Foundation; depended on by all. |
| 2 | `runsible-inventory` | published v0.0.1 stub | [`runsible-inventory.md`](runsible-inventory.md) | Define hosts, group them, attach vars, expose merged view. |
| 3 | `runsible-vault` | published v0.0.1 stub | [`runsible-vault.md`](runsible-vault.md) | Per-recipient (age/SSH) encryption + decryption; replaces ansible-vault. |
| 4 | `runsible-connection` | published v0.0.1 stub | [`runsible-connection.md`](runsible-connection.md) | Talk to remote hosts; OpenSSH default + russh fallback; the worker. |
| 5 | `yaml2toml` | published v0.0.1 stub | [`yaml2toml.md`](yaml2toml.md) | Convert Ansible YAML to runsible TOML. The adoption bridge. |
| 6 | `runsible-playbook` | published v0.0.1 stub | [`runsible-playbook.md`](runsible-playbook.md) | The engine — parse, type-check, plan, apply, report. Center of gravity. |
| 7 | `runsible` | published v0.0.1 stub | [`runsible.md`](runsible.md) | Ad-hoc tool, equivalent to `ansible`; uses the engine in single-task mode. |
| 8 | `runsible-galaxy` | published v0.0.1 stub | [`runsible-galaxy.md`](runsible-galaxy.md) | Package manager: install, build, publish, lockfile, SAT solver. |
| 9 | `runsible-doc` | published v0.0.1 stub | [`runsible-doc.md`](runsible-doc.md) | Render module/handler/filter docs from TOML siblings; `serve` mode. |
| 10 | `runsible-pull` | published v0.0.1 stub | [`runsible-pull.md`](runsible-pull.md) | Pull-mode daemon/timer; heartbeat surface; MSP wedge. |
| 11 | `runsible-console` | published v0.0.1 stub | [`runsible-console.md`](runsible-console.md) | Interactive REPL; explore-then-codify front door. |
| 12 | `runsible-lint` | published v0.0.1 stub | [`runsible-lint.md`](runsible-lint.md) | First-party linter; shares parser with playbook. |
| 13 | `runsible-test` | published v0.0.1 stub | [`runsible-test.md`](runsible-test.md) | Sanity / unit / integration testing for runsible packages. |

Total: 14 workspace members. crate names 1–13 are reserved publicly; `runsible-core` will be added as a workspace-local lib (no crates.io reservation needed unless we choose to publish it for downstream consumers).

---

## 3. The dependency DAG

```
                          runsible-core (lib)
                                ▲
              ┌─────────────────┼──────────────────┐
              │                 │                  │
       runsible-config   runsible-vault     runsible-connection
              ▲                 ▲                  ▲
              │                 │                  │
              └────────┬────────┴────────┬─────────┘
                       │                 │
               runsible-inventory      yaml2toml
                       ▲                 ▲
                       │                 │
                       └────────┬────────┘
                                │
                       runsible-playbook  ◄── (the engine)
                                ▲
              ┌─────────────────┼──────────────────┬──────────────┐
              │                 │                  │              │
          runsible        runsible-galaxy    runsible-pull   runsible-console
       (ad-hoc bin)                                                ▲
                                                                   │
                                                            runsible-doc ◄───┐
                                                                              │
                       runsible-lint  ◄── (shares parser via runsible-core)   │
                       runsible-test  ◄── (drives playbook + lint + units)   │
                                                                              │
                                                                              │
                            (runsible-doc reads what galaxy installs) ────────┘
```

Edge legend:
- An arrow from A to B means **B depends on A** (A must compile + test before B is meaningfully developable).
- Solid edges are **library imports** (`use runsible_inventory::...`).
- The `runsible-doc → runsible-galaxy` edge is **filesystem-layout-shaped** (doc reads from where galaxy installs); not a code import.

**Acyclic.** No crate-pair has a circular dependency. `runsible-core` is the bottom of the DAG; everything imports it; it imports nothing from the runsible workspace.

**Critical path** (longest arrow chain, blocks user-visible value):
`runsible-core → runsible-config → runsible-connection → runsible-playbook → runsible (ad-hoc)`

That's 5 levels. The first end-to-end demo (`runsible all -m runsible_builtin.ping`) requires every link in this chain.

---

## 4. Build phases & timeline

Phases are hard cut-overs in development priority. Within a phase, crates can be built in parallel by separate contributors; between phases, downstream phases can stub out upstream contracts to begin design but cannot integrate until upstream lands.

### **Phase 0 — Foundation**

| Crate | Milestone | Deliverable |
|---|---|---|
| `runsible-core` | F0 | Workspace-local crate scaffolded; types/errors/`Module` trait/`Connection` trait/`Event` enum sketched; no full implementations |
| `runsible-config` | M0 | Read TOML config from search path; validate; `runsible-config show <key>`; ~30 keys defined |

**Success metric:** any other crate can `use runsible_core::types::*` and `use runsible_config::Config;` and compile.

**Risks:** schema bikeshedding on the `Module` trait and on the config shape. Commit to v0.0.x semver instability until P5.

### **Phase 1 — Adapters (parallel)**

Four crates, each independently developable now that Phase 0 has landed:

| Crate | Milestone | Deliverable |
|---|---|---|
| `runsible-inventory` | M0 | TOML parser; pattern matcher; `runsible-inventory --list`; `--host` |
| `runsible-vault` | M0 | age recipient model; encrypt/decrypt files; `keygen`; `recipients add/remove/list` |
| `runsible-connection` | M0 | `SshSystem` + `Local`; sudo become; library only (no internal binary yet) |
| `yaml2toml` | M0 | Pass 1 mechanical conversion of playbooks + inventories + vars files |

**Success metric:** a smoke test where a TOML inventory + a vault'd vars file is parsed and a remote command is executed via SSH+sudo against a containerized host.

**Risks:**
- yaml2toml correctness on real corpora (per Risk R2 in user-story analysis). Begin assembling the top-200-Galaxy-roles harness in this phase even if it's not green yet.
- runsible-connection's russh fallback is M1, not M0 — keep it on the roadmap and don't let scope creep pull it forward.

### **Phase 2 — The engine**

| Crate | Milestone | Deliverable |
|---|---|---|
| `runsible-playbook` | M0 → M1 | M0 = single-task happy path; M1 = plays + handlers + blocks + tags + when + loop + 12-module library + role include/import |

The longest single-crate effort by a wide margin.

**Success metric:** a real Ansible playbook (a realistic non-trivial one — `geerlingguy.docker` is the canonical proof) is converted via yaml2toml, run via `runsible-playbook`, and produces correct host state on a fleet of 5+ containerized hosts at controller-time competitive with or better than ansible-playbook.

**Risks:**
- Module trait stability. Every breaking change cascades into every module. Lock the trait at the start of Phase 2 and grandfather required changes through a deprecation cycle.
- Cold-start performance. CI-gate at <100ms for the binary entry; <500ms for "parse a 1000-line playbook + plan against a 50-host inventory."
- yaml2toml regressions. Before exiting Phase 2 the corpus harness must be green on the top 50 Galaxy roles (excluding plugin-heavy ones).

### **Phase 3 — Surface tools (parallel)**

| Crate | Milestone | Deliverable |
|---|---|---|
| `runsible` | M0 → M1 | Ad-hoc tool; parity with `ansible -m <module> [pattern]`; M1 adds `--check`, `--diff`, `--explain-vars` |
| `runsible-galaxy` | M0 → M1 | M0 = install/lockfile from a local file:// registry; M1 = HTTP registry + publish + signing |
| `runsible-doc` | M0 → M1 | M0 = list/show/snippet against a local package tree; M1 = serve + search |
| `runsible-lint` | M0 | ~20 schema rules + text/JSON output |

**Success metric:** a `runsible-galaxy install <package>` followed by `runsible-playbook site.toml` works end-to-end from a real registry. `runsible-doc serve` renders the installed package's docs.

**Risks:**
- Network effect on the registry: it's useless without packages. Before exiting Phase 3 the registry should be seeded with `runsible-galaxy import-ansible-role` outputs of the top 50 Galaxy roles.
- Ad-hoc cold-start regressions are reputation-fatal — re-test the <100ms budget on every commit.

### **Phase 4 — Operator tools (parallel)**

| Crate | Milestone | Deliverable |
|---|---|---|
| `runsible-pull` | M0 → M1 | M0 = oneshot fetch + apply + heartbeat; M1 = daemon + interval scheduler + HTTP heartbeat POST |
| `runsible-test` | M0 → M1 | M0 = sanity + units against a single package; M1 = integration + docker/podman + Rust coverage |
| `runsible-console` | M0 → M1 | M0 = REPL with module invocation + history; M1 = completion + group-switching + become |
| `runsible-lint` | M1 | Full ~50 rule catalog + profiles + auto-fix for 10 rules |

**Success metric:** an MSP can run `runsible-pull` on a fleet of 30 hosts (managed via systemd timers, one config per tenant), with heartbeats arriving at a control plane, and a soak surface no failures the operator hasn't been notified of.

### **Phase 5 — Hardening**

Across all crates: every M2 milestone, perf hardening, the corpus harness green on top-200 Galaxy roles, signed run records (P3 compliance persona wedge), schema versioning + migration tooling for v1.0 stabilization.

**Success metric:** v1.0 release. Marketing post writes itself: cold-start vs Ansible, plan/diff vs Ansible, age vault vs ansible-vault, NDJSON output vs Ansible's text stream.

---

## 5. Persona-to-crate map

Which crates ship value to which user persona (from `00-user-story-analysis.md`)?

| Persona | Primary crates | Secondary | Wedge phase |
|---|---|---|---|
| **P1 platform engineer** (200–2000 hosts, CI-driven) | `runsible`, `runsible-playbook`, `runsible-config`, `runsible-vault` | `runsible-galaxy`, `runsible-lint`, `yaml2toml` | P3 (alpha CI demo) |
| **P2 MSP / IT services** (multi-tenant, 30 customers) | `runsible-pull`, `runsible-vault`, `runsible-galaxy` | `runsible-config`, `runsible-inventory` | P4 (pull-mode landing) |
| **P3 compliance / hardening** (CIS/STIG, signed runs) | `runsible-playbook`, `runsible-vault`, `runsible-test` | `runsible-pull`, `runsible-galaxy` (signing) | P5 (signed run records) |
| **P4 solo / homelab** (≤20 hosts, ad-hoc + light playbooks) | `runsible`, `runsible-console`, `yaml2toml` | `runsible-doc` | P3 (ad-hoc + console) |
| **P5 bootstrap-and-go** (single-file playbook, no roles) | `runsible-playbook`, `runsible-vault` | `yaml2toml` | P2 (engine M1) |

P1 and P2 are the highest-leverage segments per the user-story analysis. The plan above lands P5 first (P2 engine), P4 in P3, P1 in P3-4, P2 in P4, and P3 in P5.

---

## 7. The cross-cutting `runsible-core` contract

Multiple plans propose `runsible-core` as a workspace-local library. To make that real, here is the explicit surface that lives there (do not duplicate in any binary crate):

```rust
// runsible-core/src/lib.rs (sketch)

pub mod types {
    // The TOML AST (intermediate representation between toml::Value and the typed Playbook)
    // The Playbook, Play, Task, Handler, Block types
    // Inventory, Host, Group types (mirrored — runsible-inventory has the parser, runsible-core has the types)
    // Pattern (compiled host pattern)
    // Plan, HostPlan, TaskPlan
    // Event (NDJSON event enum: PlayStart, TaskStart, TaskOutcome, HandlerFlush, RunSummary, Error)
}

pub mod errors {
    // Top-level RunsibleError + per-area sub-errors
    // ParseError, TypeError, PlanError, ApplyError, ConnectionError, VaultError
}

pub mod schema {
    // The runsible.toml config schema definitions
    // The runsible-package manifest schema
    // The .doc.toml schema
    // The .runsible-lint.toml schema
    // (Each binary crate imports the schema it cares about; runsible-core hosts them all to prevent drift)
}

pub mod traits {
    pub trait Module {
        type Input: serde::DeserializeOwned;
        type Plan: serde::Serialize + Diff;
        type Outcome: serde::Serialize;

        fn plan(&self, input: &Self::Input, host: &HostState) -> Result<Self::Plan>;
        fn apply(&self, plan: &Self::Plan, host: &mut HostState) -> Result<Self::Outcome>;
        fn verify(&self, plan: &Self::Plan, host: &HostState) -> Result<()>;
    }

    pub trait Connection: Send + Sync {
        async fn exec(&self, cmd: &Cmd) -> Result<ExecOutcome>;
        async fn put_file(&self, src: &Path, dst: &Path, mode: Option<u32>) -> Result<()>;
        async fn get_file(&self, src: &Path, dst: &Path) -> Result<()>;
        async fn slurp(&self, src: &Path) -> Result<Vec<u8>>;
        async fn close(&mut self) -> Result<()>;
    }

    pub trait Resolver {
        // For runsible-galaxy: SAT-based dep resolution
    }
}

pub mod parser {
    // The shared TOML → typed AST parser
    // Used by runsible-playbook (run), runsible-lint (check), runsible-test (sanity)
    // *Single source of truth* — a rule cannot diverge from runtime
}

pub mod templating {
    // The MiniJinja wrapper + filter catalog
    // The `omit` sentinel
    // Filter implementations
}

pub mod ndjson {
    // The Event enum + NDJSON encoder + pretty-print decoder
    // The schema versioning ("runsible.event.v1")
}
```

This is the contract. Every per-crate plan that says "uses the shared parser" or "uses the Module trait" is referring to this crate. **No type defined here may be redefined in a downstream crate.** A linter rule (`L100` in `runsible-lint.md`) will eventually enforce this.

---

## 8. Project-level risk register

This is in addition to the per-crate risk lists. These are the risks that can sink the *project*, not just one crate.

| # | Risk | Probability | Impact | Mitigation |
|---|---|---|---|---|
| **PR1** | yaml2toml fails on real-world Ansible content; reputation damage | High | Existential | Corpus harness from Phase 1; CI-gate by Phase 2; top-200 green by v1.0; honest "supported corpus" page on the website |
| **PR2** | Python collection plugin compatibility expectations drag scope | High | High | Public messaging: collections require runsible-native re-package; never promise Python plugin loading; ship `runsible-galaxy import-ansible-collection` with honest TODO output |
| **PR3** | Cold-start performance regresses past <100ms | Medium | High | CI bench every commit; allocator profiling; defer plugin-style dynamic loading to v2 |
| **PR4** | The Module trait churns post-Phase 2; module crates can't keep up | Medium | High | Lock the trait at start of P2; grandfather changes via deprecation cycle; integration test that rebuilds all known modules against a trait change |
| **PR5** | Single-maintainer bus factor | High | Existential | This `docs/` directory is the bus-factor mitigation; recruit one co-maintainer per crate cluster (foundation, engine, surface, operator) |
| **PR6** | runsible-galaxy registry has no packages → no users → no packages | High | High | Seeded launch: 50 imported Galaxy roles ready to install on day one; partnership with one or two big-name Galaxy role maintainers |
| **PR7** | Ansible community noise ("just rewriting Ansible in Rust") | Certain | Medium | Lead launch with what runsible *does that Ansible can't*: cold-start, plan/diff, age vault, NDJSON. Never lead with parity. |
| **PR8** | A parallel project (or Red Hat itself) ships a Rust-native Ansible | Low-medium | Medium | Move fast on M1; keep the Unlicense (public domain) license to prevent any future fork-IP issues; the names are reserved |
| **PR9** | Crypto bug in runsible-vault | Low | Existential | Use `age` directly; do not roll our own crypto; external review of vault implementation before v1.0 |
| **PR10** | Compatibility-mode flags accumulate; v2 cleanup becomes its own project | Medium | Medium | Document every `--ansible-compat` flag with a sunset version; remove on schedule |

---

## 9. Decision log — open questions that need a user call

These come from the per-crate plans' "open questions" sections. Bundled here so we resolve them once, project-wide, before they fragment.

| # | Question | Crates affected | Default if no decision | Suggested call-by date |
|---|---|---|---|---|
| Q1 | Default registry: host one (`registry.runsible.dev`) vs punt to "users self-host" | galaxy | Host one | Before Phase 3 |
| Q2 | Should `--precedence-compat ansible` ship in v1 or be reserved for a "compat" feature flag at compile time? | playbook, inventory | Ship in v1 with deprecation warning | Before Phase 2 ends |
| Q3 | Default templating engine: MiniJinja (smaller, less features) vs Tera (larger, more features) | playbook (and runsible-core) | MiniJinja | Before Phase 2 starts |
| Q4 | Default DEK cipher: ChaCha20-Poly1305 vs AES-256-GCM | vault | ChaCha20-Poly1305 | Before Phase 1 ends |
| Q5 | `~/.config/runsible/` (XDG) vs `~/.runsible/` (dotfile) for user state | config, vault, console | XDG | Before Phase 0 ends |
| Q6 | Default register address for the `runsible-pull` heartbeat (none, localhost, configurable-only) | pull | Configurable-only (no default URL) | Before Phase 4 |
| Q7 | Should `runsible-doc` embed markdown in TOML strings, or require `.md` files referenced by path? | doc | Embed markdown via TOML multi-line strings | Before Phase 3 |
| Q8 | Single `Connection` worker process per host vs per-task | connection, playbook | Per host (long-lived; serves multiple tasks) | Before Phase 1 ends |
| Q9 | NDJSON event schema versioning policy (`runsible.event.v1` → `v2`): backwards compat for one major or strict major-version churn? | playbook, runsible | Backwards compat for one major | Before Phase 3 |
| Q10 | `runsible-lint --strict` semantics: convert warnings to errors, or just exit non-zero on findings? | lint | Exit non-zero only | Before Phase 4 |

These are **not** technical investigations — each has a recommended default in the relevant plan. They become blockers if the user wants different defaults; otherwise, the defaults stand and we move.

---

## 10. The launch story (when v1 ships)

This is the message that justifies the project. Lead with these in this order:

1. **Cold start.** A static Rust binary. `runsible all -m runsible_builtin.ping` returns first results in <100ms vs Ansible's 5-30s.
2. **Plan/diff first-class.** `runsible-playbook --plan` produces a JSON plan; `--diff-against <previous-plan.json>` shows exactly what changed. Not check-mode-with-best-effort — a real, structured artifact.
3. **age vault.** Per-recipient encryption. Add a teammate = one command; no rekey of file contents. SSH keys work as recipients.
4. **NDJSON output.** Default when stdout isn't a TTY. CI ingestion is a two-line jq command, not a callback plugin.
5. **Typed playbooks.** Tag enum, handler IDs, module references — all type-checked before any host is touched. Typo on host #47 becomes typo on parse.
6. **One package concept.** Roles, collections, playbooks — three answers became one. `runsible.toml` declares a package; everything else is import.
7. **TOML data plane.** No more "norway problem," no more `01:30 → 5430 seconds`, no more anchor/alias YAML rabbit holes.
8. **yaml2toml is the bridge.** Bring your existing playbooks. The conversion is one command and produces TOML you can review and commit. CI catches regressions.

What we explicitly *don't* claim at v1:
- Faster SSH (we use OpenSSH; we promise faster *controller*, not faster transport).
- Plug-and-play with Python collections (those need `import-ansible-collection`; honest about the TODO list).
- Windows / network device support (defer to v1.5 / plugin-era).

---

## 11. What "done" means at v1.0

A checklist, not a date. We ship v1.0 when *all* of these are true:

- [ ] All 13 binaries reach M2 in their respective per-crate plans
- [ ] `runsible-core` is locked at v1.0 — Module trait, Connection trait, Event schema all under semver
- [ ] yaml2toml corpus harness green on top-200 Galaxy roles (with documented exclusions)
- [ ] **Every imported package in the default registry carries its upstream license verbatim.** No third-party Ansible content has been relicensed under runsible's Unlicense. Per-package license + `imported_from` provenance is visible on every package page and in `runsible-galaxy info <pkg>` output.
- [ ] Cold-start budget (<100ms for `runsible --version`; <500ms for parse + plan of a 1000-line playbook against 50-host inventory) holds in CI
- [ ] runsible-vault crypto reviewed externally (one independent reviewer, written report)
- [ ] runsible-galaxy default registry has 50+ packages installable
- [ ] runsible-doc serves docs for the runsible-builtin module set (~70 modules)
- [ ] runsible-lint catalog has 50+ rules; profiles map cleanly from ansible-lint
- [ ] runsible-test sanity green on the runsible repo itself (dogfooding)
- [ ] runsible-pull soaked on at least one external pilot fleet (P2 MSP wedge proof)
- [ ] Documentation site (runsible.dev or similar) covers: getting started, migrating from Ansible, every CLI tool, every module
- [ ] Three external blog posts published: one each by P1, P2, P4 personas (organic — not solicited)

---

## 12. References

### Per-crate plans (this directory)
- [`runsible.md`](runsible.md) — ad-hoc binary
- [`runsible-playbook.md`](runsible-playbook.md) — engine
- [`runsible-galaxy.md`](runsible-galaxy.md) — package manager
- [`runsible-vault.md`](runsible-vault.md) — secrets
- [`runsible-inventory.md`](runsible-inventory.md) — host targeting
- [`runsible-doc.md`](runsible-doc.md) — docs tooling
- [`runsible-config.md`](runsible-config.md) — configuration
- [`runsible-console.md`](runsible-console.md) — REPL
- [`runsible-pull.md`](runsible-pull.md) — pull-mode daemon
- [`runsible-lint.md`](runsible-lint.md) — linter
- [`runsible-test.md`](runsible-test.md) — testing tooling
- [`runsible-connection.md`](runsible-connection.md) — connection worker
- [`yaml2toml.md`](yaml2toml.md) — YAML→TOML bridge

### Research and opinion (`../research/`)
- [`00-user-story-analysis.md`](../research/00-user-story-analysis.md) — the consulting deliverable; personas, JTBDs, wedge
- [`01-cli-surface.md`](../research/01-cli-surface.md) — every Ansible binary's CLI surface (12 binaries, ~7k words)
- [`02-playbook-language.md`](../research/02-playbook-language.md) — every playbook keyword (~9k words)
- [`03-inventory.md`](../research/03-inventory.md) — inventory subsystem (~5k)
- [`04-vault.md`](../research/04-vault.md) — vault byte format + CLI (~4k)
- [`05-collections-galaxy.md`](../research/05-collections-galaxy.md) — collections + galaxy (~6k)
- [`06-configuration-reference.md`](../research/06-configuration-reference.md) — config keys reference (~10k)
- [`07-onboarding-and-best-practices.md`](../research/07-onboarding-and-best-practices.md) — onboarding (~4k)
- [`08-builtin-modules.md`](../research/08-builtin-modules.md) — every ansible.builtin module (~11k)
- [`09-connection-templating-facts.md`](../research/09-connection-templating-facts.md) — connection + templating + facts (~6k)
- [`10-test-and-lint.md`](../research/10-test-and-lint.md) — test + lint reference (~6k)
- [`11-poor-decisions.md`](../research/11-poor-decisions.md) — every redesign (~6k) — **READ THIS BEFORE WRITING ANY CODE**

### External
- Ansible documentation: https://docs.ansible.com/ansible/latest/
- Ansible source: https://github.com/ansible/ansible
- ansible-lint source: https://github.com/ansible/ansible-lint
- age spec: https://age-encryption.org/v1
- TOML spec: https://toml.io/en/v1.0.0

---

## 13. Closing

You have ~131,000 words of design before the first line of production code. That is a lot — but it is calibrated. Every word answers a question someone will ask later ("why didn't we make `meta:` typed?") that would otherwise cost real work to relitigate.

The order is the message. Build runsible-core, then runsible-config, then the four adapters in parallel, then the engine, then the surface tools, then the operator tools. Critical path is tight — five crates from "git init" to "runsible all -m ping works." v1 ships when the dogfooding closes.

Build it. The names are reserved.

— end —
