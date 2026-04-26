# Ansible's Poor Decisions — and the runsible Redesigns
## Where the original tool got it wrong, ranked by ongoing user pain

> **Frame.** This is not a hatchet job. Ansible deserves credit for an enormous catalog of correct calls — the module API, the inventory model, the ad-hoc-and-playbook-share-everything ergonomic, the named-keyword task style. Those we mirror with religion. This document is the inverse: every place where Ansible's design has aged into a production tax, with a concrete redesign that runsible can ship.
>
> Every entry has the same shape: **what it is**, **why it's wrong**, **what users pay for it today**, **what runsible does instead**, **migration path from the Ansible original**. Compatibility is treated as a liability, not a virtue, when it traps users in known-bad ergonomics.

---

## 1. YAML as the source of truth

**What it is.** Ansible's data plane (playbooks, vars, inventory, defaults, requirements) is YAML. Always.

**Why it's wrong.**
- YAML 1.1 norway problem: `country: NO` becomes `country: false`. Forty other implicit type coercions like it.
- Indentation-based parsing means every editor needs a YAML-aware mode and every reviewer needs to count spaces.
- Anchors / aliases (`&` / `*`) are valid YAML but de facto banned in most Ansible style guides because they confuse other YAML consumers.
- Multi-document files (`---` separators) are valid YAML but inconsistently handled by Ansible tooling.
- YAML is not round-trippable through any common library without losing comments, key order, or formatting.

**What users pay for it today.** Production incidents from `port: 022` (parsed as int 22, not string "022"); SSH keys with leading `0` corrupted on inventory load; `version: 1.10` parsed as float `1.1`; CI failing because of a tab a reviewer missed.

**What runsible does instead.** TOML is the canonical, source-of-truth format. TOML rules eliminate every quirk above:
- All scalars are typed unambiguously (`port = "022"` is unambiguously a string; `port = 22` is unambiguously an int).
- No indentation grammar.
- No anchors or aliases — references are explicit (`{ source = "@vars.db.host" }` if we add a reference syntax at all).
- Comments are first-class and round-trip cleanly through `toml_edit`.
- One file = one document.

**Migration path.** `yaml2toml` ships in-tree and round-trips real Ansible content. The tool is opinionated about quirks: it normalizes `yes`/`no` to `true`/`false`, quotes leading-zero strings, etc. The output is reviewed once, then becomes the source of truth.

---

## 2. Jinja2 as the embedded templating language

**What it is.** Variable interpolation, conditionals, loops, filters, lookups — all expressed in Jinja2 syntax (`{{ var }}`, `{% if %}`, `var | default('x')`).

**Why it's wrong.**
- Jinja2 is implemented in Python. The controller cannot escape Python because Jinja runs there.
- The expression language inside `{{ }}` is Python-derived and shares Python's runtime gotchas (string-vs-bytes, lazy evaluation oddities, undefined chaining).
- Filter and test catalogs are extended by plugins — adding a Rust binary controller cannot replicate them without re-implementing every Jinja extension Ansible ships.
- `{{ item }}` inside `when:` is famously the *wrong* thing to write but Ansible accepts it silently.
- Conditional evaluation has subtle differences between bare strings (`when: foo`) and Jinja-templated strings (`when: "{{ foo }}"`).

**What users pay for it today.** Slow controller boot (Jinja loading + Ansible's filter set). Surprising evaluations. Errors that are Python tracebacks for what is conceptually a typo.

**What runsible does instead.** Pick a Rust-native templating engine (Tera or MiniJinja) at compile time. Constrain the expression language to a documented, typed subset:
- Variable reference: `{{ var.path.to }}` — type-checked against the declared shape if known.
- Filters: a fixed catalog (Ansible's most-used ~40 filters: `default`, `to_json`, `from_json`, `b64encode`, `b64decode`, `regex_replace`, `regex_search`, `unique`, `union`, `intersect`, `difference`, `flatten`, `dict2items`, `items2dict`, `combine`, `to_nice_json`, `to_yaml` (re-emit), `length`, `min`, `max`, `sum`, `bool`, `int`, `string`, `lower`, `upper`, `trim`, `replace`, `split`, `join`, `basename`, `dirname`, `realpath`, `expanduser`, `quote`, `mandatory`, `password_hash`, `hash`, `random`, `shuffle`).
- Tests: ditto.
- No `{% include %}`, no `{% macro %}` inside templates that ship with playbooks. Reuse is via TOML composition, not template includes.

**Migration path.** yaml2toml emits `{{ ... }}` expressions verbatim and warns when it encounters a filter not in the runsible catalog. A `runsible-lint` rule enumerates filters used by a project; users see exactly what doesn't port.

---

## 3. The 22-level variable precedence list

**What it is.** Ansible documents 22 sources of variables, ordered from "lowest priority" (command-line `-e` is the highest, role defaults are the lowest, with everything from group_vars and host_vars and play vars and task vars and registered vars and facts and `set_fact` and inventory vars layered between).

**Why it's wrong.**
- Nobody recites it from memory.
- The interactions are non-obvious (e.g., facts can override `vars:` if `cacheable: true`).
- It encourages a defensive style where every var is set at multiple levels "just in case."

**What users pay for it today.** On-call confusion. The infamous "why is this var the wrong value?" debugging session that ends with a `debug:` task printing `hostvars[inventory_hostname]`.

**What runsible does instead.** Collapse to **5 levels**:
1. **Project defaults** (`runsible.toml` `[defaults]`)
2. **Inventory** (host + group vars, with inheritance documented as parent → child)
3. **Playbook** (play-level `vars`, task-level `vars`)
4. **Runtime** (`-e` / `--var key=val`, `--vars-file path.toml`)
5. **Set-facts** (explicit, scoped to the play unless declared `[scope] global`)

Every assignment in a playbook can include a `precedence = "..."` annotation declaring its layer for clarity. The CLI ships `runsible explain-var <name>` that prints the resolution stack for a given host.

**Migration path.** yaml2toml maps each Ansible source onto its closest runsible level and emits the `precedence` annotation. A `--precedence-compat ansible` flag keeps the original 22 layers for one major version.

---

## 4. set_fact: mutable, lazy, cross-task state

**What it is.** A module that sets a variable for the rest of the play (or fact-cache lifetime if `cacheable: true`).

**Why it's wrong.**
- Mutates state in a "declarative" config tool.
- Variable lifetime is confusing — does the value survive into the next play? Into a handler? Into a delegated task?
- Encourages a "compute then act" pattern that is closer to scripting than declaration.
- `cacheable: true` makes the fact persist across runs in a way most users don't realize.

**What users pay for it today.** Bugs from stale facts. Unexpected scoping. The need for `meta: clear_facts`.

**What runsible does instead.**
- `set_fact` becomes **shadowing**, not mutation. A `set_fact` introduces a new scope; it does not mutate the existing one.
- Cross-task derived values are first-class via a `let` block at play level: `[[plays.let]] name = "release_id" expr = "..."` — the expression is evaluated once per host before tasks start, and the value is read-only.
- Mutation across tasks requires explicit `set_fact!` (with the `!`) and a documented warning. `cacheable` is removed; persistent fact storage is opt-in via a separate `runsible-fact-store` subcommand.

**Migration path.** yaml2toml emits `set_fact` as `set_fact!` with a comment noting the compatibility behavior; users are nudged to refactor to `let` blocks.

---

## 5. Roles vs Collections vs Playbooks: three reuse mechanisms

**What it is.**
- **Playbooks** are entry points (a list of plays).
- **Roles** are reusable units with a fixed directory structure (tasks/, vars/, defaults/, files/, templates/, handlers/, meta/, library/), versioned via Galaxy.
- **Collections** are namespaced bundles of modules + plugins + roles + playbooks, versioned via Galaxy.

**Why it's wrong.** Three answers to "how do I reuse this," each with its own lifecycle, install path, FQCN rules, version model, and dependency syntax. Onboarding tax: a junior engineer must learn all three. Ongoing tax: every "where does X go?" decision has three plausible answers.

**What users pay for it today.** `requirements.yml` files that mix roles and collections with different syntax. Galaxy installs that scatter content across `~/.ansible/collections/ansible_collections/` AND `~/.ansible/roles/`. Collection-roles vs Galaxy-roles being subtly different.

**What runsible does instead.** **One concept: package.** A package is a directory with `runsible.toml` declaring name, version, and exports (modules, tasks, templates, handlers, vars). A package can be installed locally or from a registry. Playbooks reference packages by `name@version`. Roles are packages. Collections are packages. There is no third thing.

**Migration path.** yaml2toml + a `runsible-galaxy import` subcommand walk a Galaxy role or collection and produce a runsible package skeleton. The user reviews and commits.

---

## 6. Vault: symmetric password file

**What it is.** A single password (or a small list of `--vault-id label@source` passwords) decrypts every encrypted file in the project. The password is typically in a file referenced by `--vault-password-file` or `ANSIBLE_VAULT_PASSWORD_FILE`.

**Why it's wrong.**
- Symmetric. Every team member who can decrypt can also encrypt — there's no read-only role.
- Rotation requires re-encrypting every file and notifying every user.
- The password lives in a file. The file gets committed by accident, shared on Slack, copied to laptops.
- Adding/removing a team member is a manual rekeying.

**What users pay for it today.** Rotation skipped. Files accidentally checked in. Shared passwords lingering forever. CI secrets stored in plaintext env vars.

**What runsible does instead.** Native asymmetric vault using **age** (or SSH-key recipients, since most engineers have an `~/.ssh/id_ed25519`):
- Each secret file lists its recipients in a header. Adding a recipient = one `runsible-vault add-recipient` command, no rekey of file contents (the per-file symmetric key is wrapped per recipient).
- Removing a recipient = re-wrap the per-file key without re-encrypting the body.
- Read-only roles via signing-only keys.
- File fallback is supported for transition: `runsible-vault import-ansible <file>` ingests an Ansible-vault file and re-encrypts under runsible's age recipients.

**Migration path.** Provide a one-shot migrator: `runsible-vault migrate-from-ansible --recipients team.toml`. Old `--vault-password-file` flag is honored for one major version.

---

## 7. ansible-pull: afterthought UX

**What it is.** A controller-less "client pulls its own playbook from a git repo and applies locally" mode.

**Why it's wrong.**
- No built-in scheduling — users wire it up to cron themselves.
- No heartbeat surface — there's no way for a fleet operator to see "which hosts ran ansible-pull in the last hour."
- No drift report — the host applies and forgets.
- ansible-pull's process model assumes a fresh interpreter per run; nothing is cached.

**What users pay for it today.** MSPs roll their own monitoring. Hosts silently stop pulling and nobody notices for weeks.

**What runsible does instead.** `runsible-pull` is a long-running daemon (or systemd timer; both supported) with:
- A built-in scheduler (`interval = "10m"` in config).
- A heartbeat artifact (`/var/lib/runsible/heartbeat.json`) updated every run, plus optional HTTP POST to a configurable URL.
- A drift report appended to a local NDJSON log per run.
- Clear separation between "fetch" and "apply" so a host can fail the apply without losing the fetched state.

**Migration path.** ansible-pull users can keep their cron job; runsible-pull's CLI accepts the same flags.

---

## 8. Connection plugin sprawl + reinventing SSH multiplexing

**What it is.** Every transport is a plugin: ssh, paramiko, local, winrm, docker, kubectl, lxd, podman, libvirt_lxc, ssh_jail, etc. The `ssh` plugin handles ControlPersist itself.

**Why it's wrong.**
- The plugin axis exists to allow Python implementations of each transport, but most transports either wrap an external binary (`ssh`, `kubectl`, `docker exec`) or wrap a Python library (`paramiko`, `pywinrm`).
- ControlPersist tuning in the ssh plugin is fiddly and well-documented but not well-implemented (per-host control sockets, race conditions on stale sockets, `ControlMaster=auto` interactions with sshd's MaxSessions).

**What users pay for it today.** SSH-related run failures that boil down to "control socket got stale," "server allows too few channels," or "config path quoting issue."

**What runsible does instead.**
- Default transport is **system OpenSSH** with explicit `ControlMaster=auto` and `ControlPersist=60s` flags. The user's `~/.ssh/config` is honored.
- Fallback transport for embedded use is **russh** (pure-Rust SSH client), selectable with `connection = "russh"`.
- Other transports (`local`, `kubectl exec`, `docker exec`, `podman exec`) are subprocess wrappers, not plugins. They live in `runsible-connection` as enum variants, not as discoverable extensions.
- Plugin-loading for new transports is deferred to v1.5+. By that point we know what users actually want.

**Migration path.** `connection = "ssh"` in inventory just works. `connection = "paramiko"` is silently mapped to `russh` with a warning.

---

## 9. Idempotency by convention

**What it is.** Each module is *expected* to be idempotent (second run = no change), but this is enforced only by reviewer convention. There is no type-system requirement, and `command:` / `shell:` modules are escape hatches with no idempotency at all.

**Why it's wrong.**
- "Idempotent" claims are unverified. A module can lie about `changed: false` and there's no test that catches it.
- The `command` module's `creates:` / `removes:` flags are a half-hearted way to add idempotence; users forget them.

**What users pay for it today.** Drifts. Re-runs that reapply changes that shouldn't apply. Circular `notify:` chains.

**What runsible does instead.** A typed module trait:

```rust
pub trait Module {
    type Input: serde::DeserializeOwned;
    type Plan: serde::Serialize + Diff;
    type Outcome: serde::Serialize;

    fn plan(&self, input: &Self::Input, host: &HostState) -> Result<Self::Plan>;
    fn apply(&self, plan: &Self::Plan, host: &mut HostState) -> Result<Self::Outcome>;
    fn verify(&self, plan: &Self::Plan, host: &HostState) -> Result<()>;  // post-apply, plan is empty
}
```

`plan()` runs first. If the resulting `Plan` is empty (no diff), `apply()` is skipped — that's the type-system enforcement of idempotence. After `apply()`, `verify()` runs — re-derives the plan against the post-apply state and asserts it is empty. Modules that fail verify are flagged as non-idempotent and the run records the violation.

`command:` and `shell:` modules are explicitly opt-out of verify (`verify_idempotent = false`), and the lint warns on every use.

**Migration path.** Existing Ansible-style modules are wrapped: their `check_mode` becomes `plan()`, their normal mode becomes `apply()`, and the wrapper auto-derives `verify()` by re-running `plan()`.

---

## 10. Output format

**What it is.** Ansible writes a stream of lines to stdout: `PLAY [...]`, `TASK [...]`, `ok: [host]`, `changed: [host] => {"...": "..."}`. There are callback plugins for JSON, but they're opt-in and the default is human-readable text.

**Why it's wrong.** CI consumers screen-scrape. Dashboards parse human strings. Failure aggregation is per-team-bespoke.

**What users pay for it today.** Custom callback plugins everywhere; a brittle "did the run succeed?" check that breaks on Ansible version bumps.

**What runsible does instead.**
- Default output is **NDJSON** when stdout is not a TTY.
- Pretty-printed text when stdout is a TTY (with color, indentation, and progress).
- The NDJSON schema is documented and versioned (`runsible.event.v1`).
- A bundled formatter (`runsible fmt-events`) re-renders NDJSON as the legacy Ansible-style text for tools that depend on it.

**Migration path.** None required — opt-in pretty mode for users who want it always (`RUNSIBLE_OUTPUT=pretty`).

---

## 11. Galaxy's resolver

**What it is.** `ansible-galaxy collection install` resolves dependencies single-pass, in the order they appear in `requirements.yml`. There is no SAT-style backtracking. There is no lockfile.

**Why it's wrong.**
- Conflicts surface late (at install time on a fresh machine) rather than at solve time.
- No reproducibility: a fresh install today and a fresh install tomorrow can pull different transitive versions.

**What users pay for it today.** "Works on my machine" for collection installs. CI flakiness when a transitive dep ships a new patch.

**What runsible does instead.**
- Real SAT-based resolver (off-the-shelf Rust crate: `pubgrub` or `sat`).
- A lockfile (`runsible.lock`) records the resolved version of every package.
- `runsible-galaxy install` is reproducible from a lockfile; without one it solves and writes the lockfile.

**Migration path.** Free. New behavior, no Ansible compatibility cost.

---

## 12. Fact gathering: opt-out, full-fat by default

**What it is.** Every play, by default, runs the `setup` module on every host first. `setup` collects ~150 facts (network, hardware, virtual, mounts, etc.) — most of which the playbook never reads.

**Why it's wrong.**
- 80% of users only reference 5% of facts.
- The first task on every host is a 1-3 second module that nobody asked for.
- Disabling (`gather_facts: false`) breaks playbooks that lazily reference a fact.

**What users pay for it today.** Slow runs. The pattern of `gather_subset: !all,!any,network` to claw back time is folklore, not documented well.

**What runsible does instead.**
- **Lazy facts.** A play declares `facts.required = ["network", "distribution", "kernel"]` (or omits it for "no facts"). Only those subsets are gathered.
- A static analyzer reads the playbook's templates and `when:` clauses and *infers* the required facts. The user can override with the explicit `facts.required` list.
- The `runsible-lint` warns on facts referenced but not in the required list (vs. assumed-available) and on facts in the required list but never referenced.

**Migration path.** yaml2toml emits a default `facts.required` list of the subsets typically used and warns the user to refine. A `--gather-everything` flag exists for transition.

---

## 13. Handlers: notify-by-name, deduplicated, queued

**What it is.** A task can `notify: <handler name>`. The handler runs once at the end of the play (or at `meta: flush_handlers`) regardless of how many times it was notified.

**Why it's wrong.**
- Notify-by-name is a string match. Typos silently no-op.
- Handler de-duplication is per-name, not per-effect: two different tasks notifying "restart nginx" merge into one restart, but two tasks notifying "restart nginx" and "reload nginx" both run even if reload would suffice.
- The "run handlers at the end of the play" rule means a failure mid-play leaves handlers un-run, even if the partial state warranted them.
- `listen:` adds a second notification mechanism (notify by event tag rather than name) — strictly more confusing.

**What users pay for it today.** Silent typo-no-ops. Handlers running too late. Handlers not running on partial failure.

**What runsible does instead.**
- Handlers are first-class typed objects with explicit IDs: `[handlers] [handlers.restart_nginx] action = "service" args = { name = "nginx", state = "restarted" }`.
- `notify` takes the typed handler ID. A typo is a parse-time error.
- Handlers run at task-list flush points which are explicit (`flush_handlers`, end-of-play, end-of-block).
- Handler de-duplication is by ID; the engine does not attempt to merge "reload" and "restart."
- On partial failure, handlers that were notified before the failure run by default; flag `[strategy] handlers_on_failure = "skip"` opts out.

**Migration path.** yaml2toml emits handlers with auto-generated IDs and rewrites `notify:` to use the IDs.

---

## 14. ansible-lint as a separate project

**What it is.** Linting Ansible content is a separate Python project (`ansible-lint`), with its own release cadence, its own versioning, and its own occasional incompatibilities with Ansible itself.

**Why it's wrong.** A user who runs Ansible 2.16 and ansible-lint 24.5 can be told their playbook is invalid by the linter and valid by the runner, or vice versa. Rules drift. The lint rules don't share a parser with Ansible itself.

**What users pay for it today.** Confusion. Duplicate dependency. CI matrices.

**What runsible does instead.** `runsible-lint` is first-party, ships from the same workspace as `runsible-playbook`, and shares the same parser. A rule cannot diverge from runtime semantics because the rule operates on the same AST runsible-playbook executes.

**Migration path.** Free.

---

## 15. No lockfile for the project

**What it is.** Ansible has `requirements.yml` (input) but no `requirements.lock` (output). Re-installing on a fresh machine can pull different transitive versions.

**Why it's wrong.** Reproducibility is a baseline expectation in 2026.

**What users pay for it today.** "It worked on staging, broke on prod" because a transitive dep version moved.

**What runsible does instead.** `runsible.lock` records every resolved package + version + checksum. CI installs from the lockfile. `runsible-galaxy update` rewrites the lockfile.

**Migration path.** Free.

---

## 16. The `become` ladder

**What it is.** `become: true`, `become_user: <user>`, `become_method: <sudo|su|doas|...>`, `become_flags: ...`, `become_password: ...`. Privilege escalation is a play/task-level concern declared via four-five keywords and a method registry.

**Why it's wrong.**
- The interaction with fact gathering is fragile (gather_facts before become or after?).
- `become_password` in plaintext or via `--ask-become-pass` is the only built-in password mechanism; secret managers require integration.
- Method-specific quirks (sudoers config, su requiring TTY, doas being parsimonious about flags) leak into playbook authoring.

**What users pay for it today.** Fragile sudoers files. Escape sequences in passwords broken on `su`. Network connection failures masked as become failures.

**What runsible does instead.**
- `become` as a typed sub-document with method-specific structured options:

```toml
[plays.become]
method = "sudo"
user = "root"
[plays.become.sudo]
flags = ["-H", "-S", "-n"]
preserve_env = ["HOME", "USER"]
password.from_keyring = "runsible:sudo:prod"
```

- Become passwords default to a system keyring (libsecret on Linux, Keychain on macOS, Credential Manager on Windows) — never plaintext in playbooks.
- A pre-flight check confirms become works on every targeted host before any task runs (saves the "host #47 sudoers misconfigured" surprise).

**Migration path.** yaml2toml maps the flat keywords to the typed sub-document. `become_password` becomes a deprecation warning recommending the keyring path.

---

## 17. `meta` as the dumping ground

**What it is.** `meta:` is a magic module accepting actions: `end_play`, `end_host`, `end_batch`, `noop`, `flush_handlers`, `clear_facts`, `clear_host_errors`, `refresh_inventory`, `reset_connection`.

**Why it's wrong.** Each of those is a different runtime concern (control flow, state mutation, infrastructure reset). Lumping them into a single `meta:` keyword obscures their semantics and makes their precondition documentation hard to surface.

**What users pay for it today.** Confusion. The need to `grep` the docs for each `meta:` action.

**What runsible does instead.** Each former `meta:` action is a first-class control-flow construct in runsible:
- `end_play` → `[[plays.tasks]] type = "control" action = "end_play"`
- `flush_handlers` → `[[plays.tasks]] type = "control" action = "flush_handlers"`
- `reset_connection` → `[[plays.tasks]] type = "control" action = "reset_connection"`
- etc.

Documentation per action is a single page each.

**Migration path.** yaml2toml rewrites `meta:` task entries.

---

## 18. `run_once` and `delegate_to` interact strangely

**What it is.** `run_once: true` runs the task on the first host in the batch and applies the result to all hosts in the batch. `delegate_to: <host>` runs the task on a different host than the inventory loop's current host. Combining them has subtle semantics around which host's facts are read, which host the result is stored under, and which connection is used.

**Why it's wrong.** "Subtle semantics" is the worst kind of semantics for a config tool.

**What users pay for it today.** "Why is my variable being templated against the wrong host?" debugging sessions.

**What runsible does instead.** Two clearly separated constructs:
- **`run_once`** is a play-level flag (not a task-level flag). The play runs on a single delegate (configured at play level: `[plays.run_once_on] host = "..."`); the play's result is broadcast to the batch as a fact.
- **`delegate_to`** is a task-level flag that swaps the connection but leaves variable scoping on the original host's hostvars. There is no other interaction.

**Migration path.** yaml2toml warns on combinations that change semantics and offers a `--strict-delegation` flag for the new semantics or `--ansible-compat` for the old.

---

## 19. Tags

**What it is.** Tags are strings attached to tasks/blocks/plays. `--tags foo,bar` runs only matching tasks; `--skip-tags foo` skips them. Special tags: `always`, `never`, `untagged`, `all`.

**Why it's wrong.**
- The string-matching model means a typo in `--tags` silently runs nothing.
- Inheritance is by reference (block tags propagate to tasks) but the propagation rules vary by Ansible version.
- The interaction of `always` and `never` with `--skip-tags` is a thicket.

**What users pay for it today.** Surprising no-op runs. Tags drift across role versions.

**What runsible does instead.**
- Tags are declared at the package level, like an enum: `[tags] release = {} hotfix = {} cleanup = {}`. Using an undeclared tag at the CLI is a hard error.
- The runner emits an "evaluated tag set" line to NDJSON before any task runs, listing the resolved tag intersection — users see what will run.

**Migration path.** yaml2toml collects all tags used in the project and writes a `[tags]` block; freshly-typed at the CLI thereafter.

---

## 20. ansible-doc's plugin-listing model

**What it is.** `ansible-doc -t module <name>` prints docs for a module. `-l` lists all available modules in a configured collection list. The doc strings are Python triple-quoted YAML embedded in module source.

**Why it's wrong.** Doc is parsed from Python source at runtime — slow, fragile (one bad doc string fails the listing), Python-only.

**What users pay for it today.** `ansible-doc -l` taking 5+ seconds. Doc errors hiding modules.

**What runsible does instead.**
- Module docs are a TOML sibling file: `mymod.toml` + `mymod.doc.toml`. `runsible-doc` reads the doc file directly. No source parsing.
- Doc is rendered to text, JSON, or markdown.
- A `runsible-doc serve` mode runs an HTTP doc browser locally.

**Migration path.** A `runsible-doc import-ansible <module-source>` command extracts a module's doc strings and emits a `.doc.toml`.

---

## 21. `serial:` and batching

**What it is.** `serial: <int|percentage|list>` controls how many hosts a play runs on at a time.

**Why it's wrong.** The list form (`serial: [1, 5, 10, 100%]`) is a poorly-known feature; the percentage interaction with `max_fail_percentage` is non-intuitive; the order of host iteration within a batch is `order: <inventory|sorted|reverse_sorted|reverse_inventory|shuffle>` — yet another keyword.

**What users pay for it today.** Surprising rollout pacing. Shuffle order being non-deterministic and breaking incident reproduction.

**What runsible does instead.**
- A typed `[plays.rollout]` sub-document:

```toml
[plays.rollout]
batches = [1, 5, 10, "100%"]
order = "inventory"          # or "sorted", "shuffled-with-seed=42"
max_fail_percentage = 10
```

- Shuffle is always seeded for reproducibility.

**Migration path.** yaml2toml maps `serial:` + `order:` + `max_fail_percentage:` into the typed sub-document.

---

## 22. The Galaxy "namespace.collection.module" FQCN with the `collections:` keyword

**What it is.** Modules are addressed by FQCN (`community.general.archive`). To save typing, `collections:` keyword at play/role level lets you list namespaces to search. The interaction is opaque (especially: `collections:` does not propagate from a play into included tasks in some versions).

**Why it's wrong.** "Sometimes propagates" is a bug, not a feature. The shortened name resolution is implicit and version-dependent.

**What users pay for it today.** "Worked in my playbook, breaks in my role." The user reaches for FQCN everywhere defensively.

**What runsible does instead.** No name shortening. Every module reference is `package.module` (e.g., `runsible_builtin.copy`). At the top of a TOML playbook, `[imports]` aliases shorten:

```toml
[imports]
copy = "runsible_builtin.copy"
template = "runsible_builtin.template"

[[plays.tasks]]
name = "Drop a file"
copy = { src = "...", dest = "..." }
```

The aliasing is lexical, not runtime, and applies to the file it's declared in.

**Migration path.** yaml2toml emits an `[imports]` block from inferred module use; `collections:` is dropped.

---

## 23. Implicit fact-cache scope

**What it is.** Fact caching backends (memory, jsonfile, redis, memcached, mongodb, yaml) persist `setup` results between runs. The cache is keyed by hostname.

**Why it's wrong.**
- Cross-environment fact bleed (staging facts read in prod) if the cache is shared.
- Cache invalidation is "wait for `fact_caching_timeout` to expire" or `meta: clear_facts`.
- The persistent fact mechanism is conflated with the in-run fact mechanism.

**What users pay for it today.** Subtle bugs from stale cached facts. Fact cache corruption. The need for "delete the cache file before every run" rituals.

**What runsible does instead.**
- Per-environment fact stores: `runsible-fact-store --env prod` is a separate namespace from `--env staging`.
- Cache TTL is mandatory and per-fact-subset, not global.
- An explicit `runsible-fact-store invalidate <host> [--subset network]` command.

**Migration path.** Existing fact caches can be imported; users are nudged to migrate.

---

## 24. Async tasks: `async` + `poll`

**What it is.** `async: <seconds>` makes a task background; `poll: <seconds>` polls for completion. Combining `async: 0` with `poll: 0` makes a fire-and-forget task that can be checked later with `async_status`.

**Why it's wrong.** The naming overloads `async` (a duration) and `poll` (a duration where 0 means "don't wait"). The state of an async task is held in a per-host JSON file that's hard to find.

**What users pay for it today.** Confusion. Lost async jobs.

**What runsible does instead.** Two distinct keywords:
- `[plays.tasks.async] timeout = "5m"` — the task may take up to 5 minutes; runsible waits for it.
- `[plays.tasks.background]` — the task is fire-and-forget; runsible records a job ID and exits the task immediately. `runsible-job status <id>` and `runsible-job wait <id>` are first-class subcommands.

**Migration path.** yaml2toml maps `async:` + `poll:` to the new shape.

---

## 25. Smaller items, in bulk

These are too small for full sections but are real:

- **`changed_when` and `failed_when` are Jinja expressions evaluated against the task result.** They should be typed predicates with documented field access. runsible: `[plays.tasks.changed_when] expr = "result.rc != 0 || result.stdout.contains('UPDATED')"`.
- **`no_log: true` is per-task and easy to forget.** runsible: declare sensitive fields at the module level (`[modules.copy.sensitive] = ["content"]`); the runner redacts them everywhere automatically.
- **`vars_prompt` blocks are interactive and break automation.** runsible: `vars_prompt` works only when stdin is a TTY; in non-TTY contexts the run errors at parse time unless `vars_prompt.required = false` and a default is provided.
- **`hash_behaviour = merge` is a global config knob that changes semantic behavior of every play.** runsible: per-merge-site explicit syntax (`merge = "deep"` or `merge = "replace"`), no global toggle.
- **`ANSIBLE_*` env vars number in the hundreds and many shadow config keys silently.** runsible: env-var support is opt-in per config key (declared in the config schema), not implicit.
- **`ANSIBLE_LIBRARY` and other path env vars accept colon-separated lists with no quoting story for paths containing colons.** runsible: TOML config only, never colon-list env vars.
- **The `command` module's `creates:`/`removes:`/`chdir:` shadows the actual command parsing.** runsible: `[command]` module has an explicit `[command.idempotence] creates = "..." removes = "..."` sub-document.
- **`when:` accepts both bare strings and lists; the list form is implicitly AND-ed.** runsible: `when` is always a list (single-item lists are fine); the AND semantic is documented at the type level.
- **`failed_when:` evaluating to `false` does *not* mean the task succeeded — it means the task is no longer considered failed.** This double-negative is famously confusing. runsible: rename to `success_predicate` and invert the semantics.
- **The `register:` mechanism creates a per-host variable whose schema varies per module.** runsible: each module declares a typed `Outcome`; `register:` writes to a typed slot.
- **`include_*` vs `import_*` is a static-vs-dynamic distinction that affects when tags, when, and other keywords propagate.** runsible: collapse to one mechanism, `compose` (always static, parsed at load time), with `compose_dynamic` reserved for the rare case of runtime-derived names.

---

## Summary table — the wedge

| # | Misdesign | runsible redesign | Migration cost |
|---|---|---|---|
| 1 | YAML | TOML + yaml2toml | One-shot conversion |
| 2 | Jinja2 | Tera/MiniJinja, fixed catalog | Filter mapping |
| 3 | 22-level precedence | 5 levels | Annotation |
| 4 | set_fact mutation | shadowing + let | `set_fact!` shim |
| 5 | Roles vs Collections | One concept: package | Importer |
| 6 | Vault password file | age recipients | Migrator |
| 7 | ansible-pull UX | Daemon + heartbeat | None |
| 8 | Connection plugins | OpenSSH + russh | Mostly none |
| 9 | Idempotency by convention | Typed module trait | Wrapper for legacy modules |
| 10 | Unstructured output | NDJSON default | None |
| 11 | Galaxy resolver | SAT + lockfile | Free |
| 12 | Lazy facts | Required-list + inference | Annotation |
| 13 | Handler typos | Typed handler IDs | Rewrite |
| 14 | Lint as separate project | First-party, shared parser | Free |
| 15 | No lockfile | runsible.lock | Free |
| 16 | Become flat keywords | Typed become sub-document | Mapping |
| 17 | meta: dumping ground | Typed control-flow tasks | Rewrite |
| 18 | run_once + delegate_to | Cleanly separated | Annotation |
| 19 | Tags by string | Tag enum | Schema generation |
| 20 | ansible-doc parses Python | TOML sibling files | Importer |
| 21 | serial: stringly | Typed rollout sub-document | Mapping |
| 22 | collections: keyword | Lexical [imports] block | Inferred rewrite |
| 23 | Implicit fact-cache | Per-env + TTL'd | Importer |
| 24 | async + poll naming | async vs background | Mapping |

End. The next document is the per-crate plans, which apply these redesigns to the 13 crates' missions.
