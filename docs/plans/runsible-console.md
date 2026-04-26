# runsible — `runsible-console`

## 1. Mission

`runsible-console` is the interactive REPL of the runsible suite — the equivalent of `ansible-console`. It exists for one workflow: a human at a terminal who needs to investigate a fleet right now, modify the targeted set as they go, and fire ad-hoc tasks against it without writing a playbook. The user picks an inventory, optionally narrows to a group or pattern, optionally enters become context, then types module invocations and watches output stream back per host. It is not a shell, not a playbook editor, not a long-running supervision tool — it is runsible's `psql`/`redis-cli`: a thin, fast, completion-aware front door over the real engine, built for explore-then-act work and for capturing the exploration as a first draft of a playbook. Compared to Ansible's console it ships fewer ceremonies (verbs match the rest of the runsible CLI; no `cd` metaphor) and richer output (per-host bordered rendering, expandable long output, exportable session transcripts).

## 2. Scope

**In scope.** A TUI REPL with proper line editing, persistent history across sessions, and tab-completion that knows modules, module flags, host names, group names, and variable names. Per-line ad-hoc execution delegated to the runsible engine. Mid-session context switching: change target with `@webservers`, become with `become root`, transport with `connect ssh`, vars with `vars set foo=bar`. Replay command history. Export the session as a TOML playbook draft with `export session.toml`. Inline documentation with `?<module>` (delegated to runsible-doc). Script mode (`--script file`) for canned investigations.

**Out of scope.** Not a general SSH terminal (use `ssh`). Not a long-running task supervisor (use `runsible-playbook` + `runsible-job`). Not an editor (use `$EDITOR`). Not a plugin discovery tool beyond the loaded catalog (`runsible-galaxy search` is the right place). No tutorial walkthrough mode in v1 (see open questions). No multi-pane TUI — the REPL is single-stream with in-band pretty rendering.

## 3. The REPL grammar

Each line typed by the user is dispatched by a small parser. The grammar is deliberately narrow.

- `<module> [arg=value ...]` — runs as ad-hoc against the current target. Argument syntax matches `runsible -a` (`key=value` space-separated, JSON values, `@file` for arg-file). Default module for a bare command is `command`, matching Ansible. The `!` prefix forces `shell`: `! ls /tmp | wc -l`.
- `@<group_or_pattern>` — switches the targeted host pattern. Accepts the full `runsible-inventory` pattern grammar (union `:`, intersection `:&`, exclusion `:!`, globs, `~regex`). `@all` returns to root. Current target shown in the prompt.
- `become <user> [method=sudo]` / `become off` — sets become for subsequent lines. No argument shows current state. Passwords come from the system keyring or are prompted on first use; never typed inline.
- `vars set <k>=<v>` / `vars unset <k>` / `vars show` — session-scope vars, passed as `-e` equivalents on every subsequent task.
- `connect <transport>` — switches the connection plugin (`ssh`, `local`, `russh`). Unknown transports are a parse-time error.
- `inventory reload` — re-reads inventory from disk after edits in another window. `inventory show [<pattern>]` prints the resolved host tree (equivalent to `runsible-inventory --graph`).
- `?<module>` / `help <module>` — shows the module's docs, delegated to `runsible-doc`. Bare `?` prints REPL command help.
- `history` — prints the indexed command history. `replay <n>` re-runs entry n (negative indexes from the end; `replay -1` is the previous command).
- `export <file>` — writes the session as a TOML playbook draft. Targeted patterns become plays, become context becomes the play-level become sub-document, ad-hoc commands become ordered tasks, and var mutations become `[plays.let]` blocks.
- `set forks <n>` / `set check on|off` / `set diff on|off` / `set timeout <seconds>` — execution-mode toggles.
- `quit` / Ctrl-D — exits. With `--export-on-exit` set, writes the session draft on the way out.

### Sample session

```
$ runsible-console -i inv.toml webservers
runsible-console 0.1 — inventory: inv.toml — 24 hosts in @webservers
[@webservers] > ping
ok: web01 .. web24 (24/24)

[@webservers] > @webservers:&us-east
[@webservers:&us-east] > vars set rollout_id=2026-04-26-a
ok: vars.rollout_id = "2026-04-26-a"

[@webservers:&us-east] > become root
ok: become user=root method=sudo (password from keyring runsible:sudo:default)

[@webservers:&us-east as root] > service name=nginx state=reloaded
changed: web01.example.com
changed: web02.example.com
ok: web03.example.com    # already reloaded by another operator within the last second

[@webservers:&us-east as root] > ?service
service — manage system services via the local init system
  required: name (string)
  optional: state (enum: started|stopped|restarted|reloaded), enabled (bool)
... (truncated; press space for more)

[@webservers:&us-east as root] > ! tail -1 /var/log/nginx/error.log
ok: web01 (stdout: "2026/04/26 14:22:11 [info] worker process started")
ok: web02 (stdout: "2026/04/26 14:22:11 [info] worker process started")
ok: web03 (stdout: "2026/04/26 14:21:58 [info] worker process started")

[@webservers:&us-east as root] > export reload-nginx-us-east.toml
ok: wrote 18 lines to reload-nginx-us-east.toml

[@webservers:&us-east as root] > quit
session: 7 commands, 4 ad-hoc tasks, 0 errors, 1m 12s wall
```

## 4. Completion

Tab completion is sourced from four catalogs, loaded at REPL startup and refreshed on `inventory reload`:

- **Module names.** Pulled from the engine's module registry plus any package-installed modules visible to the current project. Aliases declared in the project's `runsible.toml` `[imports]` block are also offered.
- **Module flag names.** Parsed from each module's `.doc.toml` schema. Context-aware: `copy <TAB>` offers `src=`, `dest=`, `owner=`; `copy src=<TAB>` falls through to controller-side filesystem completion.
- **Host and group names.** Pulled from the resolved inventory. Available after `@`, after `inventory show`, and as values for any module flag whose schema declares `type = "host"`.
- **Variable names.** Pulled from the session vars table plus inventory-resolved vars on the current target. Offered after `{{` inside any module argument (templating works inside ad-hoc args).

Completion uses a prefix tree; ambiguous prefixes show a one-line candidate hint.

## 5. Output format

Two rendering paths, selected by stdout:

- **NDJSON when piped.** When stdout is not a TTY (or `--output ndjson` is forced), every event is one line of `runsible.event.v1` JSON. Same schema as the rest of the suite. Useful for `runsible-console --script foo.repl > log.ndjson` in CI.
- **Pretty when interactive.** Per-host bordered output. Each host's response renders as a small indented box: `ok|changed|failed: <host>` header, optional `stdout`/`stderr` body, optional structured rendering (e.g. `service` shows the parsed state diff). Long output (>~20 lines) collapses to a summary tail with `... (N more lines, j to expand)`. Navigation is keyboard-driven (`less`-style): `j`/`k` between collapsed cells, `<enter>` to expand, `q` to close. Concurrent host output is buffered per host so lines don't interleave; the renderer flushes a host as a unit on report.

Color on by default on a TTY; `--no-color` disables it. Respects `NO_COLOR=1`.

## 6. CLI surface

Intentionally small.

- `runsible-console -i <inventory> [pattern]` — start the REPL with the given inventory and an initial host pattern (default `all`).
- `-i, --inventory <path>` — repeatable; comma-separated lists also accepted.
- `--no-color` — force-disable color in pretty mode.
- `--no-history` — do not read or write the persistent history file (single-session memory only).
- `--history-file <path>` — override the default history path (`~/.runsible/console-history`).
- `--script <file>` — run a script of REPL commands then exit. One command per line; `#` starts a comment. Stdout defaults to NDJSON in script mode.
- `--export <file>` — after the session ends (`quit` or end of `--script`), write the captured session as a TOML playbook draft.
- `--output <pretty|ndjson>` — force the renderer regardless of TTY status.
- `-e, --extra-vars <kv>` — repeatable; equivalent to `vars set <kv>` at REPL startup.
- `--vault-id`, `--vault-password-file` — identical to the rest of the suite.
- Standard `-v` (repeatable to `-vvv`), `--version`, `-h/--help`.

The console intentionally does not duplicate the runsible-playbook flag surface (no `--tags`, no `--check` at the CLI). `set check on` inside the REPL is the only way to toggle check mode.

## 7. Implementation

- **Library choice.** `rustyline` for the line editor — readline keybindings, completion hooks, history-file support. Defer `ratatui` to M2; the only feature that needs full-screen TUI is expand-collapse of long output. Until then, render pretty output as plain colored text and let the terminal scroll.
- **History storage.** Plain-text at `~/.runsible/console-history`, one command per line, inventory + pattern annotated as a comment header per session. Bounded at 10,000 lines.
- **Module invocation.** In-process call into the runsible engine (same Rust API `runsible-playbook` uses). No subprocess per command. The console holds a long-lived `Engine`, in-memory `Inventory`, and `RuntimeContext` (target/become/vars/connection). Each line builds a one-task `Plan` and submits it.
- **Session capture.** A `SessionRecorder` shadows the dispatcher, capturing every executed task with resolved arguments, target, become context, and outcome. `export <file>` emits a TOML playbook with one play per contiguous run of commands sharing the same target/become.
- **Concurrency.** Fan-out is the engine's job. The console adds a per-host output buffer so the pretty renderer can flush each host atomically.
- **External deps.** `rustyline`, `serde`/`serde_json`/`toml`, `crossterm` for color/keys, `tera`/`minijinja` for argument templating.

## 8. Redesigns vs Ansible

`ansible-console` is functional but minimal. runsible-console diverges as follows:

- **Parallel fan-out with clean rendering.** Ansible interleaves host stdout under default forks. runsible-console buffers per host and flushes complete results, so 100-host fan-out is readable rather than scrambled.
- **`export session.toml` is new.** Ansible offers no way to capture a session as a playbook. This single feature changes the workflow from "explore, then re-do the work in YAML" to "explore, then commit the sequence" — the bridge between ad-hoc and codified.
- **`?<module>` is new.** Ansible's `help <module>` prints a one-line summary; full docs require leaving for `ansible-doc`. runsible-console renders full doc content in-band, with paging.
- **Typed completion.** Ansible relies on Python `readline` and silently degrades when it's missing; the catalog is module-names only. runsible-console always completes, and knows host/group/var names plus per-flag completion via `.doc.toml`.
- **Persistent history.** Ansible has none by default. runsible-console persists at `~/.runsible/console-history` with cross-session `Ctrl-R`.
- **Verb-level consistency.** Ansible's `cd` for changing the current group is a shell metaphor distinct from how the rest of the Ansible CLI targets hosts (`-l`). runsible-console uses `@<pattern>`, matching the positional grammar used elsewhere in the suite.
- **Become declared once.** Ansible has `become true` / `become_user x` / `become_method y` as three toggles. runsible-console wraps them: `become root [method=sudo]` sets all three; `become off` clears.

Intentionally not redesigned: the verb-then-args form, `command`-as-default-module, and `!` for shell — muscle memory matters.

## 9. Milestones

**M0 — usable REPL.**
- `rustyline`-backed input loop.
- Module invocation against a static target supplied at startup.
- Pretty output (colored text, no expand-collapse).
- No completion, no `export`, no script mode.
- `quit` and `Ctrl-D` exit cleanly.

**M1 — completion, history, context switching.**
- Tab completion for modules, flags, hosts, groups, vars.
- Persistent history + `Ctrl-R`.
- `@<pattern>`, `become <user>` (with keyring lookup), `vars set/unset/show`.
- `?<module>` and `help`.
- `set forks/check/diff/timeout`.

**M2 — TUI rich output, export, script mode.**
- Per-host bordered output with collapse/expand of long results (the only place that needs `ratatui`-style layout).
- `export session.toml` writes a clean playbook draft.
- `--script <file>` for canned sessions.
- `replay <n>` and `history`.
- NDJSON output mode for piped stdout.

**M3 — polish.**
- Vault interaction smoothed (cached identities per session).
- `inventory reload` preserves vars context.
- Session statistics line on exit.
- First-class parse-time error rendering for unknown modules/flags so users don't have to launch a task to see the problem.

## 10. Dependencies on other crates

- **`runsible-engine`** — the executor library; the console is a thin wrapper around `Engine::execute_one_task(target, task)`.
- **`runsible-inventory`** — loading, pattern resolution, and the host/group catalog for completion.
- **`runsible-config`** — honoring project `runsible.toml` (default forks, become method, connection).
- **`runsible-vault`** — transparent decryption of vaulted var files referenced by the inventory.
- **`runsible-doc`** — `?<module>` and `help` rendering, library-linked (no subprocess).
- **`runsible-modules-builtin`** plus any installed module packages — for the module catalog and `.doc.toml` schemas used by completion.

No direct dependency on the `runsible-playbook` binary; both use the same engine library.

## 11. Tests

- **REPL command parser unit tests.** Every grammar form (`@pattern`, `become user`, `vars set k=v`, `?module`, `! shell`, etc.) parses to the right IR. Malformed lines produce structured parse errors with suggestions.
- **Golden tests for `--script` mode.** A library of `.repl` script files paired with expected NDJSON event streams; CI runs each against a mock engine and diffs. This is the core regression net.
- **Completion tests.** Given a synthetic catalog and partial input, `Completer::candidates(input)` returns the expected list. Covers the four sources independently and combined.
- **Session export tests.** Run a script, call `export`, assert the resulting TOML round-trips through `runsible-playbook --syntax-check` with no errors.
- **Snapshot tests for pretty rendering.** `insta` (or homegrown) snapshots for representative outcomes (1 host ok, 100 host ok, 1 failed, mixed). Renderer is deterministic given a fixed event sequence, so snapshots are stable.
- **TUI rendering tests (M2+).** Drive the `ratatui` widget with a synthetic event stream and snapshot the buffer where feasible. Anything not feasible in unit form lives in a smoke checklist (`docs/manual-tests/console.md`).
- **Integration smoke test.** Real REPL session against `localhost`: `ping`, `command`, `copy`, clean exit. Runs in CI under a pty harness (`portable-pty` or `expectrl`).

## 12. Risks

- **Niche feature, easy to over-invest.** Most users reach for `runsible` (ad-hoc) before the console. Budget M0+M1 at ~3 engineer-weeks; M2 only if adoption demands it. Do not block other crate releases on console work.
- **TUI testing is awkward.** `ratatui` buffer snapshots are brittle to width and color-mode changes. Mitigation: keep REPL logic (parser/dispatcher/recorder) fully separate from rendering; test logic exhaustively, lean on the smoke checklist for the renderer.
- **Vault prompts in the REPL UX.** A first-time vault prompt mid-REPL must not corrupt the line editor's terminal state. Mitigation: route through `rustyline`'s prompt API, not raw `stdin`.
- **Session export drift.** A captured session is a snapshot of intent; later replay may differ (inventory changed, vars resolved differently). Mitigation: emit a header comment with timestamp, inventory hash, resolved host count. Make the file obviously a "draft."
- **Completion catalogs become stale** when the user installs a module package in another shell. Mitigation: extend `inventory reload` into `catalogs reload` — refreshes inventory + module catalog + doc files in one shot.
- **In-process engine holds resources.** SSH ControlMaster sockets accumulate as the user retargets. Mitigation: engine prunes idle SSH connections (default 5-minute idle); `connect <transport>` forces a reset.

## 13. Open questions

- **Thin wrapper or first-class engine?** Should the console be a thin wrapper around the `runsible` ad-hoc binary (subprocess per command), or its own first-class engine (in-process)? The plan assumes in-process for cold-start reasons. Risk: long-lived state. Decision: in-process by default, escape hatch `--engine subprocess` if the in-process model causes bugs.
- **Tutorial mode?** `runsible-console --tutorial` walks a user through "list your inventory, ping your hosts, copy a file, run a service module" with text between commands. Strong onboarding tool for P5 (bootstrap engineer) and P4 (homelabber). Out of v1 unless we hear demand on the alpha; revisit at 1.0.
- **Multi-target mode?** Should `@web1 @db1` split fan-out per target with separate result panels? Useful for cross-tier comparison. Adds TUI complexity. Defer to 1.x.
- **Embedded `runsible-doc serve`?** When the user types `?<module>`, optionally open the module's full HTML doc in `$BROWSER` via a one-shot local server. Useful for long modules. Risk: surprise web traffic from a CLI tool. Default off; opt-in `--doc-browser`.
- **Session sharing.** Could two engineers share a console session via a side channel (SSH-multiplexed REPL)? Operationally interesting for incident response. Out of v1 — but worth a placeholder so we don't bake single-user assumptions into the recorder.
