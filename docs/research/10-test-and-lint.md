# 10 — Test and Lint: ansible-test and ansible-lint reference

A reference distillation of upstream `ansible-test` and `ansible-lint` for the
benefit of `runsible-test` and `runsible-lint`. The two tools are separate
projects in the Ansible ecosystem and they are wired together by convention,
not by coupling. `ansible-test` ships with `ansible-core`; `ansible-lint` is a
standalone PyPI package maintained by the Ansible Community team.

This document is exhaustive on the surface area we plan to mirror or
deliberately reject. It is organized as:

1. ansible-test — overview, subcommands, sanity catalog, units, integration,
   environments, coverage, `--changed` / CI mode.
2. ansible-lint — installation, CLI, configuration, profiles, rules catalog,
   custom rules, auto-fix, ignore mechanism, integrations.
3. What runsible-test and runsible-lint should NOT inherit.
4. What we should keep, in TOML form.

---

## Part 1 — `ansible-test`

### 1.1 What `ansible-test` is

`ansible-test` is the test runner that ships with `ansible-core`. It is the
official tool used by Ansible upstream, by every collection on Galaxy, and by
Red Hat's certification pipeline. It is invoked from the root of either an
ansible-core checkout or a collection directory.

A collection must be located on disk at
`~/ansible_collections/<NAMESPACE>/<COLLECTION_NAME>` for `ansible-test` to
recognise it. This requirement is structural, not configurable.

`ansible-test` always operates on what `git` tracks. Files ignored by `.gitignore`
are invisible to it.

Python is the runtime. `ansible-test` requires the same Python interpreter
range as `ansible-core` (currently 3.11+ on the controller; 3.7+ on managed
nodes) and pulls in pytest, pylint, yamllint, mypy, etc. as needed via its
internal requirements files at
`test/lib/ansible_test/_data/requirements/*.txt` and
`test/lib/ansible_test/_data/requirements/constraints.txt`.

### 1.2 Top-level subcommands

```
ansible-test sanity              # static analysis
ansible-test units               # pytest-driven unit tests
ansible-test integration         # POSIX integration tests (playbooks)
ansible-test network-integration # network-platform integration tests
ansible-test windows-integration # windows-platform integration tests via WinRM
ansible-test coverage            # code coverage management and reporting
ansible-test env                 # show information about the test environment
ansible-test shell               # open an interactive shell inside a test env
```

Global options that apply to every subcommand:

```
-h, --help                       # subcommand help
--version                        # ansible-test version
-v, -vv, -vvv                    # verbose
--color / --no-color
--debug
--truncate <COLS>
--redact / --no-redact           # control credential redaction in logs
--metadata <PATH>                # pre-existing metadata json (CI integration)
```

Argcomplete is supported for tab completion if the user installs the
`argcomplete` Python package.

### 1.3 Test environments

Every `ansible-test` subcommand accepts an environment specifier. There are
three sources of environment, plus a "controller vs target" distinction for
integration tests where the two halves can differ.

Environment flags:

```
--docker [<IMAGE>] [--docker-privileged] [--docker-network <NET>]
                  [--docker-no-pull] [--docker-keep-git] [--docker-terminate <WHEN>]
--podman [<IMAGE>] [...same modifiers]
--remote <PLATFORM/VERSION> [--remote-terminate <WHEN>]
                            [--remote-stage <STAGE>]
                            [--remote-endpoint <URL>]
--venv [--venv-system-site-packages]
--python <VERSION>
--requirements                   # auto-install python requirements before run
--requirements-mode {only,skip}
--controller {docker,podman,remote,venv,origin}[:<spec>]
--target     {docker,podman,remote,venv,origin}[:<spec>]
```

Containers are recommended for sanity, units, and integration tests. Custom
images must run as `root`, contain `systemd` or another init, `sshd` on port
22, a POSIX `sh`, `sleep`, and a supported Python. They must not declare
`VOLUME`s. Docker is preferred over Podman; set `ANSIBLE_TEST_PREFER_PODMAN=1`
to flip that.

`--venv` builds a managed virtualenv per Python version. `--remote` provisions
ephemeral cloud VMs (requires API credentials). `origin` means the host
itself.

Environment variables are NOT propagated from the host into the test
environment when using `--docker` or `--remote`. To debug,
`ansible-test shell --docker` drops you into the container.

### 1.4 `ansible-test sanity`

Static analysis. Runs all available sanity tests by default.

Selected options:

```
--test <NAME>                    # repeatable; pin to a single sanity test
--skip-test <NAME>               # repeatable
--list-tests
--allow-disabled                 # include tests marked disabled
--enable-test <NAME>             # turn on opt-in tests
--lint                           # machine-readable output for editor integrations
--junit                          # JUnit XML to test/results/junit/
--failure-ok
--prime-venvs                    # populate venv cache without running anything
--changed                        # only test files changed (see CI mode)
--tracked / --untracked
--keep-git
```

#### 1.4.1 Sanity test catalog

The current upstream catalog. Tests marked **(core)** only run against
ansible-core itself; the rest are valid for collections too.

| Test | What it checks |
|------|----------------|
| `action-plugin-docs` | Action plugins must have a paired module file with `DOCUMENTATION`. |
| `ansible-doc` | `ansible-doc` succeeds for every module/plugin advertised. |
| `changelog` | `changelogs/changelog.yaml` and fragment YAML in `changelogs/fragments/` are valid. |
| `compile` | All Python files parse on every supported Python version. |
| `empty-init` | `__init__.py` files used purely for namespace must be empty. |
| `ignores` | Validates `tests/sanity/ignore-x.x.txt` lines. |
| `import` | Modules import cleanly under controlled `sys.path`. |
| `line-endings` | Reject CRLF; UNIX line endings only. |
| `no-assert` | No bare `assert` (stripped under `python -O`). |
| `no-basestring` | No Py2 `basestring`. |
| `no-dict-iteritems` | No Py2 `dict.iteritems()`. |
| `no-dict-iterkeys` | No Py2 `dict.iterkeys()`. |
| `no-dict-itervalues` | No Py2 `dict.itervalues()`. |
| `no-get-exception` | No deprecated `get_exception()` from `module_utils.pycompat24`. |
| `no-illegal-filenames` | Names cross-platform safe (no `:`, no reserved Windows names). |
| `no-main-display` | Modules must not import `Display` from `__main__`. |
| `no-smart-quotes` | ASCII quotes only. |
| `no-unicode-literals` | No `from __future__ import unicode_literals`. |
| `pep8` | PEP-8 style via `pycodestyle`. Selectable codes via `test/sanity/pep8/legacy-files.txt` etc. |
| `pslint` | PowerShell modules linted with PSScriptAnalyzer. |
| `pylint` | Python static analysis with a curated rule set. |
| `replace-urlopen` | Use `open_url()` from `module_utils.urls` instead of `urllib.urlopen`. |
| `runtime-metadata` | `meta/runtime.yml` schema. |
| `shebang` | Modules and scripts have allowed shebangs. |
| `shellcheck` | Shell scripts pass `shellcheck`. |
| `symlinks` | No broken or out-of-tree symlinks. |
| `use-argspec-type-path` | Use `type='path'` (not `type='str'`) for path arguments. |
| `use-compat-six` | Import `six` via `module_utils.six` shim. |
| `validate-modules` | Heaviest check; see 1.4.2. |
| `yamllint` | YAML style/syntax. |
| `ansible-requirements` **(core)** | `requirements.txt` matches packaging. |
| `bin-symlinks` **(core)** | `bin/` entries point to valid targets. |
| `boilerplate` **(core)** | Modules/plugins start with required `# -*- coding: utf-8 -*-` + future imports + GPL header. |
| `integration-aliases` **(core)** | Every integration target's `aliases` file declares supported environments. |
| `mypy` **(core)** | Static type-checking with mypy. |
| `no-unwanted-files` **(core)** | Reject committed artifacts (`.pyc`, `__pycache__`, etc.). |
| `obsolete-files` **(core)** | Files removed in a release must not reappear. |
| `package-data` **(core)** | `MANIFEST.in` / `pyproject.toml` actually ship the right files. |
| `pymarkdown` **(core)** | Markdown files lint. |
| `release-names` **(core)** | Release codenames match the curated list. |
| `required-and-default-attributes` **(core)** | Argspec fields can't be both `required` and have a `default`. |
| `test-constraints` **(core)** | Pinned constraints in test requirements are valid. |

#### 1.4.2 `validate-modules`

The flagship sanity test. Validates an Ansible module against a JSON schema
plus a long list of programmatic checks. Categories:

- Documentation — `DOCUMENTATION`/`RETURN`/`EXAMPLES` blocks parse, options
  documented match `argument_spec`, `version_added` is sane, deprecated modules
  carry valid removal metadata.
- Syntax — Python interpreter line, type-checking idioms, module entry point.
- Imports — module utilities used, no `boto` (must be `boto3`), no direct
  `requests` import in modules, no `os.system`.
- Parameters — `argument_spec` keys valid, `aliases` don't collide,
  `mutually_exclusive`/`required_one_of`/`required_together` reference real
  parameters.
- Naming — module file extension `.py` (or `.ps1`), in correct subdirectory.
- Attributes — check-mode, diff-mode, platform tags, `become`-support
  declared.

Output: `E###` errors and `W###` warnings, all individually ignorable in
`tests/sanity/ignore-x.x.txt`.

### 1.5 `ansible-test units`

pytest under the hood. Test files live at `test/units/` (core) or
`tests/unit/plugins/` (collection), mirroring the layout of the source under
test.

Selected options:

```
--coverage                       # collect coverage
--coverage-check
--num-workers <N>
--tb {auto,long,short,line,native,no}
--keep-failed                    # keep the test environment after a failure
--changed                        # only run tests for changed files
ansible-test units <TARGET>...   # restrict to specific files / module names
```

Module unit-testing pattern (relevant because we will need a Rust analogue):

- `units.modules.utils.set_module_args(dict)` injects mock STDIN.
- `AnsibleExitJson` / `AnsibleFailJson` exceptions intercept
  `module.exit_json` / `module.fail_json` so the test can assert on the result
  dict.
- `unittest.mock.patch.multiple(basic.AnsibleModule, exit_json=..., fail_json=..., run_command=...)`
  is the standard way to stub the AnsibleModule class.
- `get_bin_path` is mocked per-test for binary-resolution control.

### 1.6 `ansible-test integration`

Functional tests written *as Ansible playbooks*. Each test target is a role
under `tests/integration/targets/<name>/`.

Target layout:

```
tests/integration/targets/<name>/
  aliases                # one tag per line: e.g. "destructive", "needs/root",
                         #   "needs/privileged", "skip/freebsd",
                         #   "needs/httptester", "unsupported"
  meta/main.yml          # role metadata, especially `dependencies:`
  defaults/main.yml
  vars/main.yml
  tasks/main.yml         # OR
  runme.sh               # script-style target (network, complex setups)
  runme.yml              # playbook invoked by runme.sh
  files/                 # static fixtures
  templates/             # j2 templates
  handlers/main.yml
  library/               # local custom modules used in tests
  module_utils/
```

Convention names:
- `prepare_<thing>` — pre-test setup target invoked via `meta/main.yml`
  dependencies.
- `setup_<thing>` — same role, by older convention.
- `incidental_<thing>` — opportunistic coverage for things that aren't the
  focus.

The `aliases` file is how a target announces what platforms / privileges /
network access it needs and what kind of test it is. Selected aliases:

```
destructive             # may install/remove packages; do not run on bare metal
non_destructive         # keeps to its own files
needs/root
needs/privileged
needs/httptester        # requires the integration httptester sidecar
needs/target/<name>     # requires another target as a dep
disabled                # never run
unsupported             # only run on opt-in
slow                    # slow path; usually skipped
context/controller      # runs only on the controller side
context/target          # runs only against the target
shippable/<group>/group<n>   # CI grouping
```

Selected `ansible-test integration` options:

```
ansible-test integration <TARGET>...
ansible-test integration --list-targets
ansible-test integration --list-targets --include <NAME>
ansible-test integration --include <NAME>
ansible-test integration --exclude <NAME>
--allow-destructive
--allow-disabled
--allow-root
--allow-unsupported
--retry-on-error
--continue-on-error
--debug-strategy
--changed-all-target <TARGET>
--changed-from <REF>
--changed
--tags <TAG>
--skip-tags <TAG>
--start-at <TARGET>
--start-at-task <TASK NAME>
--diff
--no-temp-workdir
```

Configuration files:

```
tests/integration/integration_config.yml   # tunable variables for tests
tests/integration/cloud-config-aws.yml     # provider creds (and similar)
tests/integration/cloud-config-azure.yml
tests/integration/cloud-config-cs.ini
tests/integration/cloud-config-gcp.yml
tests/integration/cloud-config-vcenter.yml
tests/integration/inventory                # default inventory
tests/integration/inventory.winrm          # for windows-integration
tests/integration/inventory.networking     # for network-integration
tests/integration/network-integration.cfg
```

### 1.7 `ansible-test network-integration` and `ansible-test windows-integration`

Same target layout as POSIX integration. They differ in:

- `network-integration` — uses `inventory.networking`, runs against real or
  virtualised network devices via cli/netconf/httpapi connections, and is
  always invoked through `runme.sh` plus per-platform `--platform` selectors.
- `windows-integration` — uses `inventory.winrm` (or `inventory.psrp`),
  requires PSRemoting on the target, and ships a parallel sanity test
  (`pslint`) for PowerShell modules.

### 1.8 `ansible-test coverage`

Coverage is opt-in; pass `--coverage` to `units` or `integration` and
ansible-test stores raw `.coverage` data in `test/results/coverage/`. Then:

```
ansible-test coverage report           # console summary
ansible-test coverage html             # HTML at test/results/reports/coverage/index.html
ansible-test coverage xml              # cobertura XML for codecov.io
ansible-test coverage erase            # wipe collected data
ansible-test coverage combine          # merge per-py-version data sets
ansible-test coverage analyze targets  # which target hit which line
ansible-test coverage analyze targets generate <OUT>
ansible-test coverage analyze targets expand <IN> <OUT>
ansible-test coverage analyze targets filter
ansible-test coverage analyze targets combine
ansible-test coverage analyze targets missing
```

The `analyze targets` family answers "which integration target exercises
which file?" and supports producing a minimum-target set.

### 1.9 `ansible-test env` and `ansible-test shell`

```
ansible-test env --show
ansible-test env --dump
ansible-test env --list-files
ansible-test shell [--docker | --venv | --remote] [--python <V>] [-- <CMD>]
```

`shell` is the primary debugging affordance — it provisions the same
environment the tests would run in and drops you into it.

### 1.10 `--changed` / CI mode

`ansible-test sanity --changed` and `ansible-test integration --changed` look
at the working tree's git diff (against the base branch by default) to decide
which sanity tests / integration targets are *implicated* by the diff. The
mapping table is built from sanity test prefixes and from integration target
dependency graphs. CI uses this exclusively; full runs are
`ansible-test sanity` with no `--changed`.

Related flags:

```
--changed
--changed-from <REF>
--changed-path <PATH>
--changed-all-target <NAME>
--changed-all-mode {default,include,exclude}
--tracked
--untracked
--ignore-committer
--base-branch <BRANCH>
```

### 1.11 Outputs and artefacts

Everything ansible-test produces lands under `test/results/` (configurable
with `--metadata`):

```
test/results/coverage/         # raw coverage data
test/results/reports/coverage/ # rendered HTML
test/results/junit/            # JUnit XML
test/results/data/             # ansible-test internal state
test/results/logs/             # per-target captured logs
test/results/bot/              # bot-consumable triage info
```

---

## Part 2 — `ansible-lint`

### 2.1 What `ansible-lint` is

A standalone CLI that statically lints Ansible content (playbooks, roles,
collections, `galaxy.yml`, `meta/main.yml`, `requirements.yml`, inventory)
against a curated set of rules organised into progressive profiles. It does
*style and correctness*, not execution. It is the moral counterpart to
`ansible-test sanity` for *user content* rather than for module/plugin code.

It is a Python package, distinct from `ansible-core`, and currently does not
support Windows as an installation target.

### 2.2 Installation

```
pip3 install ansible-lint                 # basic
pip3 install "ansible-lint[lock]"         # with dependency lockfile (Py 3.10+)
pip3 install ansible-dev-tools            # bundle (recommended)
pipx install ansible-lint                 # isolated
dnf install ansible-lint                  # Fedora/RHEL (subscription req. on RHEL)
pip3 install git+https://github.com/ansible/ansible-lint
```

A `community-ansible-dev-tools` container image and an official GitHub Action
are also provided.

### 2.3 CLI

```
ansible-lint [OPTIONS] [LINTABLES...]
```

If `LINTABLES` is empty, ansible-lint auto-detects content from the cwd. It
must be run from project root; running from `roles/` or `tasks/` will under-
report.

Selected options:

```
-c, --config-file <PATH>          # default search order:
                                  #   .ansible-lint, .ansible-lint.yml,
                                  #   .ansible-lint.yaml,
                                  #   .config/ansible-lint.yml,
                                  #   .config/ansible-lint.yaml
-i, --ignore-file <PATH>          # default .ansible-lint-ignore or
                                  #   .config/ansible-lint-ignore.txt
--yamllint-file <PATH>            # custom yamllint config

-f, --format {brief,full,md,json,codeclimate,quiet,pep8,sarif}
--sarif-file <PATH>               # also write SARIF to file
-q, -qq                           # quieter
--nocolor / --force-color

-L, --list-rules                  # show every loaded rule
-T, --list-tags                   # tags and their rules
-P, --list-profiles
--profile {min,basic,moderate,safety,shared,production}
-t, --tags <CSV>                  # only run rules with these tags/IDs
-x, --skip-list <CSV>
-w, --warn-list <CSV>             # default: experimental,jinja[spacing],fqcn[deep]
--enable-list <CSV>               # turn on opt-in rules
-r, --rules-dir <PATH>            # custom rules directory (repeatable)
-R                                # keep default rules even when -r is given

--fix [WRITE_LIST]                # auto-fix; WRITE_LIST = all|none|<CSV of rules>
-s, --strict                      # warnings cause non-zero exit

--exclude <PATH>                  # repeatable
--show-relpath
--project-dir <PATH>
--generate-ignore                 # write current violations into ignore file
--offline / --no-offline          # skip requirements.yml / schema fetching

-v, -vv                           # verbose
--version
-h, --help
```

Exit codes: `0` clean, `2` violation, plus rule-specific propagation in
strict mode.

#### 2.3.1 Output formats

- `brief` — one line per violation.
- `full` — multi-line, with snippet and rationale.
- `md` — markdown tables.
- `pep8` — `path:line:col: rule-id message`. Easy for editors.
- `json` — structured violations.
- `codeclimate` — Code Climate JSON; severity squashed to `minor`/`major`.
- `sarif` — SARIF 2.1.0 JSON; the canonical for security toolchains.
- `quiet` — almost nothing on stdout.

GitHub Actions: when `GITHUB_ACTIONS=true`, ansible-lint also emits
`::error file=...,line=...::message` annotations.

#### 2.3.2 Auto-fix

`--fix` rewrites files in place. Default `WRITE_LIST` is `all`. As of the
current release, the rules that implement transforms are:

```
command-instead-of-shell
deprecated-local-action
fqcn
jinja
key-order
name
no-free-form
no-jinja-when
no-log-password
partial-become
yaml
```

Set `ANSIBLE_LINT_WRITE_TMP=1` to dump fixed files into temp paths instead of
mutating the original (used by tests).

### 2.4 Configuration: `.ansible-lint`

Single YAML file. Schema (every key, with semantics):

```yaml
profile: production            # null | min | basic | moderate | safety | shared | production
strict: false
offline: false
parseable: false               # legacy alias for `--format pep8`
quiet: false
verbosity: 0
use_default_rules: true
progressive: false             # treat new violations as errors, old as warnings

# Rule selection
skip_list:                     # full disable; not even reported
  - yaml[line-length]
warn_list:                     # report but do not fail
  - experimental
  - fqcn[deep]
  - jinja[spacing]
enable_list:                   # turn on opt-in rules
  - empty-string-compare
  - no-log-password
  - no-prompting
  - only-builtins
  - jinja-template-extension
  - no-same-owner
  - loop-var-prefix

# Path scoping
exclude_paths:
  - .cache/
  - .github/
  - tests/output/
project_dir: .

# File-kind override (repeatable maps)
kinds:
  - playbook: "**/playbooks/*.{yml,yaml}"
  - tasks:    "**/tasks/*.{yml,yaml}"
  - vars:     "**/vars/*.{yml,yaml}"
  - meta:     "**/meta/main.{yml,yaml}"
  - yaml:     "**/*.{yml,yaml}"

# Mocking missing collections / modules / roles so syntax-check passes
mock_modules:
  - my_namespace.my_collection.my_module
mock_roles:
  - my_namespace.my_collection.my_role
mock_filters:
  - my_namespace.my_collection.my_filter

# Variable conventions
var_naming_pattern: "^[a-z_][a-z0-9_]*$"
loop_var_prefix:    "^(__|{role}_)"
task_name_prefix:   "{stem} | "

# Ansible version handling
supported_ansible_also:        # treat these EOL versions as still supported
  - "2.14"

# Auto-fix selection
write_list:
  - all
# or
# write_list: none
# write_list: [yaml, fqcn, name]

# Extra variables passed to syntax-check
extra_vars:
  some_required_var: dummy

# Complexity caps
max_block_depth: 20
max_tasks: 100

# Tag-name conventions for galaxy/meta
galaxy_tag_format: "^[a-z0-9]+$"
```

Configuration discovery: searches cwd then walks up parents, never crossing
out of a git repository. `exclude_paths` are *relative to the config file*;
CLI `--exclude` is *relative to cwd*.

#### 2.4.1 Ignore file format

`.ansible-lint-ignore` (or `.config/ansible-lint-ignore.txt`):

```
path/to/file.yml rule-id
path/to/other.yml yaml[line-length] skip
```

Trailing `skip` suppresses the warning entirely; without it the line is
reported as a non-fatal warning.

Inline equivalents:

- `# noqa: rule-id rule-id-2` — suppress on that line.
- `tags: [skip_ansible_lint]` — suppress task-based rules on that task only
  (does not stop yaml-style line rules).

#### 2.4.2 Environment variables

```
ANSIBLE_LINT_CUSTOM_RULESDIR    # additional rule lookup path
ANSIBLE_LINT_IGNORE_FILE        # override default ignore file path
ANSIBLE_LINT_WRITE_TMP          # write fixes to temp instead of in-place
ANSIBLE_LINT_SKIP_SCHEMA_UPDATE # skip schema refresh
ANSIBLE_LINT_NODEPS             # skip dependency install (reports fewer rules)
NO_COLOR / FORCE_COLOR
GITHUB_ACTIONS / GITHUB_WORKFLOW
```

#### 2.4.3 Cache

`{project_dir}/.cache` holds installed/mocked roles, collections, modules,
and downloaded JSON schemas. It is not auto-cleared. Add `.cache/` to
`.gitignore`.

### 2.5 Profiles

Profiles are inheritance-only — `production` includes `shared` includes
`safety` includes `moderate` includes `basic` includes `min`. Selecting a
profile activates every rule from that profile and below. Tags `opt-in` and
`experimental` do not override profile inclusion.

#### `min` — must-haves (cannot be skipped)

```
internal-error
load-failure
parser-error
syntax-check
```

#### `basic` — adds standard styles & fixable issues

```
command-instead-of-module
command-instead-of-shell
deprecated-bare-vars
deprecated-local-action
deprecated-module
inline-env-var
key-order
literal-compare
jinja
no-free-form
no-jinja-when
no-tabs
partial-become
playbook-extension
role-name
schema
name
var-naming
yaml
```

#### `moderate` — naming/casing nuance

Adds:

```
name[template]
name[imperative]
name[casing]
spell-var-name
```

#### `safety` — non-determinism / risk

Adds:

```
avoid-implicit
latest
package-latest
risky-file-permissions
risky-octal
risky-shell-pipe
```

#### `shared` — galaxy publishing readiness

Adds:

```
galaxy
ignore-errors
layout
meta-incorrect
meta-no-tags
meta-video-links
meta-version
meta-runtime
no-changed-when
no-handler
no-relative-paths
max-block-depth
max-tasks
unsafe-loop
```

#### `production` — Red Hat AAP certification bar

Adds:

```
avoid-dot-notation
sanity
fqcn
import-task-no-when
meta-no-dependencies
single-entry-point
use-loop
```

### 2.6 Rules catalog (default + opt-in)

Each entry is `rule-id` — what it flags. Sub-IDs are listed inline.

#### Structural / parser rules (always on)

- `internal-error` — A linter exception escaped while processing a file.
  Once raised, no other rules run on that file. Common cause: an invalid
  Jinja template or an out-of-range host pattern (`hosts: all[1]` with no
  fallback). Add to `warn_list` while debugging.
- `load-failure` — File could not be loaded: non-UTF-8 encoding, unsupported
  custom YAML tags (`!!`), undecryptable inline `!vault`. Sub-ID
  `load-failure[not-found]` for missing referenced files. Cannot be
  `skip_list`-ed; only `exclude_paths`.
- `parser-error` — `AnsibleParserError`; malformed YAML or invalid Ansible
  structure.
- `syntax-check` — Runs `ansible-playbook --syntax-check`. Cannot be
  skipped. Sub-IDs:
  - `syntax-check[empty-playbook]` — Empty playbook.
  - `syntax-check[malformed]` — Malformed block while loading.
  - `syntax-check[missing-file]` — Referenced file not found.
  - `syntax-check[unknown-module]` — Module/action could not be resolved
    (collection probably not installed; declare it in
    `collections/requirements.yml`).
  - `syntax-check[specific]` — All other syntax errors.

#### Schema validation

- `schema` — Validates JSON Schema for known files. Cannot be `noqa`-ed.
  Sub-IDs (each backed by a separate schema):
  - `schema[ansible-lint-config]`
  - `schema[role-arg-spec]`
  - `schema[execution-environment]`
  - `schema[galaxy]`
  - `schema[inventory]` (matches `inventory/*.yml`)
  - `schema[meta-runtime]`
  - `schema[meta]` (requires `galaxy_info.standalone` for non-collection roles)
  - `schema[play-argspec]`
  - `schema[playbook]`
  - `schema[requirements]`
  - `schema[tasks]` (matches `tasks/**/*.yml`)
  - `schema[vars]` (matches `vars/*.yml`, `defaults/*.yml`)
  - `schema[ansible-navigator]` (maintained out-of-tree)

#### Style / formatting

- `yaml` — yamllint-driven. Auto-fixable. Sub-IDs:
  `yaml[brackets]`, `yaml[colons]`, `yaml[commas]`, `yaml[comments]`,
  `yaml[comments-indentation]`, `yaml[document-start]`, `yaml[empty-lines]`,
  `yaml[indentation]`, `yaml[key-duplicates]`, `yaml[line-length]` (default
  160), `yaml[new-line-at-end-of-file]`, `yaml[octal-values]`,
  `yaml[syntax]`, `yaml[trailing-spaces]`, `yaml[truthy]`.
- `no-tabs` — No `\t` characters. Exception: inside `ansible.builtin.lineinfile`.
- `key-order` — Canonical key order. `name` first; `block`/`rescue`/`always`
  last; `when`/`tags`/`become` etc. before `block` so they don't get
  visually misattributed when a block grows. Auto-fixable.
- `playbook-extension` — Playbook files must be `.yml` or `.yaml`.
- `jinja-template-extension` — Opt-in. `template:` source files should end
  in `.j2`.

#### Naming

- `name` — Auto-fixable for casing only. Sub-IDs:
  - `name[missing]` — Tasks must have `name:`.
  - `name[play]` — Plays must have `name:`.
  - `name[casing]` — Names start with uppercase.
  - `name[template]` — Jinja `{{ }}` allowed only at end of name (so the
    static prefix is searchable in logs).
  - `name[unique]` — Names unique across `pre_tasks`/`tasks`/
    `post_tasks`/`handlers` of one play. Required for `--start-at-task`.
  - `name[prefix]` — Opt-in. Included task files (excluding `main.yml`) get a
    file-stem prefix. Useful for tracing tasks back to the file they came
    from.
- `role-name` — Lowercase alphanumerics + `_`, must start with a letter.
- `var-naming` — Sub-IDs:
  - `var-naming[non-string]`
  - `var-naming[non-ascii]`
  - `var-naming[no-keyword]`
  - `var-naming[no-jinja]`
  - `var-naming[pattern]` (default `^[a-z_][a-z0-9_]*$`)
  - `var-naming[no-role-prefix]` — In a role, vars must be prefixed
    `<role_name>_`.
  - `var-naming[no-reserved]`
  - `var-naming[read-only]`
- `loop-var-prefix` — Opt-in. In a role, loops must rename `item` via
  `loop_control.loop_var` and the new name must match
  `^(__|{role}_)`.

#### FQCN / module reference

- `fqcn` — Auto-fixable. Sub-IDs:
  - `fqcn[action]` — Use `ansible.builtin.X` rather than bare `X`.
  - `fqcn[action-core]` — Specifically `ansible.legacy.X` vs
    `ansible.builtin.X` distinction.
  - `fqcn[canonical]` — Use the canonical name, not an alias.
  - `fqcn[deep]` — Avoid `collection.subdir.subdir.module`-style
    deep-nested plugin paths. Default-warning, not error.
  - `fqcn[keyword]` — Prefer FQCNs over the `collections:` keyword.

#### Conditional / control flow

- `literal-compare` — `when: var == True` ⇒ `when: var`.
- `empty-string-compare` — Opt-in. `when: var == ""` ⇒ `when: var | length == 0`.
- `no-jinja-when` — Don't wrap `when:` in `{{ }}`. Auto-fixable.
- `no-handler` — `when: result.changed` ⇒ use `notify:` and a real handler.
- `ignore-errors` — Bare `ignore_errors: true` is a smell. Use
  `ignore_errors: "{{ ansible_check_mode }}"` or `failed_when:` instead.
- `run-once` — `run_once` with `strategy: free` is undefined.
- `no-changed-when` — Tasks that mutate must report `changed_when` or
  `creates`/`removes`. Tasks that only read must `changed_when: false`.

#### Module recommendation

- `command-instead-of-module` — `command: apt-get update` ⇒ `apt:`.
- `command-instead-of-shell` — Auto-fixable. Use `command:` unless you
  actually need a shell feature (pipes, env-var expansion, redirection).
- `deprecated-module` — Module is scheduled for removal.
- `deprecated-bare-vars` — `with_items: foo` is ambiguous; either
  `with_items: "{{ foo }}"` or use a literal list.
- `deprecated-local-action` — Auto-fixable. `local_action:` ⇒
  `delegate_to: localhost`.
- `inline-env-var` — Don't set env vars inline in
  `ansible.builtin.command`; use `environment:` or `shell`.
- `no-free-form` — Auto-fixable. Free-form
  `command: chdir=/tmp touch foo` ⇒ structured `cmd:`/`chdir:`. Sub-IDs:
  - `no-free-form[raw]` — `executable=` in `raw`.
  - `no-free-form[raw-non-string]` — non-string args to `raw`.
- `only-builtins` — Opt-in. Disallow non-`ansible.builtin` modules.

#### Risk / safety

- `risky-file-permissions` — `assemble`/`copy`/`file`/`get_url`/
  `replace`/`template`/`archive`/`ini_file` need an explicit `mode:` or
  `create: false`, or `mode: preserve` for `copy`. Doesn't honour
  `module_defaults`.
- `risky-octal` — `mode: 644` ⇒ `mode: "0644"` or `mode: "0o644"` or
  symbolic `u=rw,g=r,o=r`.
- `risky-shell-pipe` — Pipelines need `set -o pipefail`. Skipped when
  `executable: pwsh`.
- `no-log-password` — Opt-in. Tasks looping over secret vars must set
  `no_log: true`. Auto-fixable.
- `no-prompting` — Opt-in. No `vars_prompt` or `ansible.builtin.pause`
  (CI/CD friendliness).
- `no-relative-paths` — `copy.src` / `template.src` should resolve via the
  role's `files/` or `templates/` rather than `../`.
- `no-same-owner` — Opt-in. `synchronize` needs `owner: false`/
  `group: false`; `unarchive` needs `--no-same-owner`.
- `latest` — `version: HEAD` etc. forbidden. Sub-IDs: `latest[git]`,
  `latest[hg]`.
- `package-latest` — `state: latest` warns; prefer pinned version, or
  `state: latest` + `update_only: true` (dnf) / `only_upgrade: true` (apt).
- `partial-become` — Auto-fixable. `become_user` requires `become: true` at
  the same level. Sub-IDs `partial-become[play]`, `partial-become[task]`.
- `avoid-implicit` — Sub-ID `avoid-implicit[copy-content]`. Don't pass a
  dict to `copy.content`; explicitly `to_json` it.

#### Jinja content

- `jinja` — Auto-fixable for spacing only. Sub-IDs:
  - `jinja[spacing]` — Black-style spacing inside `{{ }}` and `{% %}`. In
    `warn_list` by default.
  - `jinja[invalid]` — Template doesn't parse.

#### Galaxy / collection metadata

- `galaxy` — Sub-IDs:
  - `galaxy[version-missing]`
  - `galaxy[version-incorrect]` — `version` must be ≥ `1.0.0`.
  - `galaxy[no-changelog]` — One of `CHANGELOG.md`, `CHANGELOG.rst`,
    `changelogs/changelog.yaml`, `changelogs/changelog.yml` required.
  - `galaxy[no-runtime]` — `meta/runtime.yml` required.
  - `galaxy[tags]` — At least one canonical tag from
    {application, cloud, database, infrastructure, linux, monitoring,
    networking, security, storage, tools, windows}.
  - `galaxy[tags-format]`
  - `galaxy[tags-length]` — ≤ 64 chars.
  - `galaxy[tags-count]` — ≤ 20 tags.
  - `galaxy[invalid-dependency-version]`
- `meta-incorrect` — `meta/main.yml` placeholder text in
  `author`/`description`/`company`/`license`.
- `meta-no-tags` — Tags must be lowercase alphanumeric, no special chars.
- `meta-video-links` — Each video link must be a dict with `url:` and
  `title:` and the `url` must be YouTube/Vimeo/Google Drive shared link.
- `meta-runtime` — Sub-IDs:
  - `meta-runtime[unsupported-version]` — `requires_ansible` must reference
    a currently-supported `ansible-core` version.
  - `meta-runtime[invalid-version]` — Must be a full version (`>=2.17.0`,
    not `>=2.17`).

#### Sanity / certification

- `sanity` — Validates `tests/sanity/ignore-x.x.txt` only contains entries
  from the Red Hat-approved allowlist. Sub-IDs: `sanity[cannot-ignore]`,
  `sanity[bad-ignore]`. Allowlist (current):
  `validate-modules:missing-gplv3-license`, `action-plugin-docs`,
  `import-2.6`, `import-2.6!skip`, `import-2.7`, `import-2.7!skip`,
  `import-3.5`, `import-3.5!skip`, `compile-2.6`, `compile-2.6!skip`,
  `compile-2.7`, `compile-2.7!skip`, `compile-3.5`, `compile-3.5!skip`,
  `shellcheck`, `shebang`, `pylint:used-before-assignment`.

#### Complexity

- `complexity` — Sub-IDs:
  - `complexity[tasks]` — > `max_tasks` (default 100) tasks in one file.
  - `complexity[play]` — Per-play task count.
  - `complexity[nesting]` — Block depth > `max_block_depth` (default 20).

#### Internal reporting

- `warning` — Generic informational warning. Sub-ID
  `warning[raw-non-string]`.

### 2.7 Custom rules

Custom rules are Python classes inheriting from
`ansiblelint.rules.AnsibleLintRule`. Required class attributes:

```python
from ansiblelint.rules import AnsibleLintRule

class ExampleRule(AnsibleLintRule):
    id = "EXAMPLE001"
    description = "Sample custom rule"
    tags = ["custom"]
    severity = "MEDIUM"     # VERY_HIGH | HIGH | MEDIUM | LOW | VERY_LOW
    version_added = "1.0.0"

    def matchplay(self, file, data):
        ...
    def matchtask(self, task, file=None):
        ...
    def matchyaml(self, file):
        ...
    def matchtext(self, file, line):
        ...
```

A rule provides at least one `match*` method. `matchtask` receives a
`Task` object whose `module`/`module_arguments`/keys-of-interest are
normalised — modifiers like `when`, `with_items`, etc. surface as accessible
attributes.

Discovery: `ansible-lint` auto-loads rules from
`ansiblelint/rules/custom/`, from `--rules-dir <DIR>` (repeatable), and from
`ANSIBLE_LINT_CUSTOM_RULESDIR`. Distributable plugin packages register under
the entry-point group `ansiblelint.rules.custom`.

New rules ship with the `experimental` tag for at least two weeks before
graduating; `experimental` is in the default `warn_list`.

### 2.8 Integrations

- **pre-commit**: `repo: https://github.com/ansible/ansible-lint`,
  `id: ansible-lint`. Honours the same config file. Recommended `--fix`
  mode for opt-in formatting.
- **GitHub Actions**: `ansible/ansible-lint@main` action; sets
  `GITHUB_ACTIONS=true` so violations are emitted as
  `::error file=...`. Often combined with `--sarif-file` and
  `github/codeql-action/upload-sarif`.
- **Editor LSPs**: `ansible-language-server` invokes ansible-lint behind the
  scenes for VS Code, Neovim, Sublime, Emacs.
- **Galaxy / Automation Hub**: scoring on submission uses the `production`
  profile.

---

## Part 3 — What `runsible-test` and `runsible-lint` should NOT inherit

The following are baked into ansible-test/ansible-lint because Ansible itself
is Python. They make no sense for runsible.

### 3.1 Python-language sanity tests

Every entry below exists to police Python source files. Runsible has no user
Python:

```
compile                # Python compile-on-every-version
empty-init             # __init__.py policy
import                 # Python import resolution
no-assert              # bare assert under -O
no-basestring          # Py2 vs Py3
no-dict-iteritems
no-dict-iterkeys
no-dict-itervalues
no-get-exception
no-main-display
no-smart-quotes        # only because Py source allowed it
no-unicode-literals
pep8
pylint
mypy
replace-urlopen        # urllib idioms
use-argspec-type-path  # Python AnsibleModule argspec
use-compat-six
boilerplate            # Py module GPL header
ansible-requirements   # Python packaging metadata
package-data
test-constraints
release-names          # ansible-core release codenames
no-unwanted-files      # .pyc, __pycache__
```

These should not have direct equivalents. Where a *concern* is real (e.g.
"don't ship binary cruft"), it folds into `cargo` and `git` hygiene, not
into a separate sanity test.

### 3.2 PowerShell sanity tests

```
pslint
no-illegal-filenames    # only because of Windows-driver modules
windows-integration     # WinRM/PSRP transport
shebang                 # in its current form (PowerShell + Python shebangs)
```

If runsible grows a PowerShell connection plugin, this will resurface; until
then, drop.

### 3.3 Python plugin-architecture tests

These exist because Ansible loads collections by walking
`plugins/{action,callback,connection,filter,inventory,lookup,modules,vars,...}`
on the Python import path:

```
action-plugin-docs
ansible-doc            # in its current form (it imports the plugin to ask for docs)
runtime-metadata       # specifically the meta/runtime.yml plugin_routing redirects
required-and-default-attributes  # Python AnsibleModule argspec
validate-modules       # entirely Python-module-shape-aware
```

Runsible plugins are Rust crates with a typed trait surface. Doc validation
happens at compile time; redirects are handled by the dispatch table, not by
a YAML schema.

### 3.4 ansible-lint rules tied to Python execution model

Most rules port cleanly because they are about user content, not internals.
The exceptions:

- `ansible-doc`-driven validation (`schema[ansible-navigator]` etc.) is
  Python-tooling-shaped. Replace with TOML schema.
- `meta-no-dependencies` (production profile) — Galaxy-specific; only
  meaningful if we adopt Galaxy's role-vs-collection split, which we
  shouldn't.
- The `mock_modules` / `mock_filters` mechanism is solving a problem
  (missing-collection syntax-check) caused by Ansible's late binding. Rust
  collection resolution is build-time; if a module isn't present, the
  playbook doesn't compile in the first place.
- `sanity` rule (the lint rule named `sanity`) — only relevant to Red Hat
  certification of Python collections. Skip.

### 3.5 Configuration / packaging surface to drop

- `ansible-test`'s `--remote` cloud-VM provisioner — that's a paid Red Hat
  service. We can run on whatever the user has.
- `tests/sanity/ignore-*.txt` — needed because Ansible can't move fast on
  fixing legacy issues. Rust forces fixes at compile time, so per-version
  ignore files are unnecessary; add a single `runsible-test.toml` with a
  `skip` table if at all.
- `changelogs/changelog.yaml` plus `changelogs/fragments/*.yml` — Galaxy
  expectation. We use `CHANGELOG.md` driven by `git-cliff`/equivalent.
- `meta/runtime.yml` `plugin_routing` redirects — Ansible's collection-
  rename mechanism. We do collection rename via Cargo `[package].rename`.
- The Docker image requirement set (root user, init, sshd on 22, no
  VOLUMEs) — `runsible-test` should be happy in any reasonable container,
  including rootless and ephemeral.

---

## Part 4 — What runsible should keep, in TOML

A short pointer at how the surface translates. Detailed design lives in the
respective design docs; here are the load-bearing decisions.

### 4.1 `runsible-test` subcommand surface

```
runsible-test sanity         # static checks over a collection / playbook tree
runsible-test units          # cargo test (built-in plugins) + plugin author tests
runsible-test integration    # role-target-style functional tests
runsible-test coverage       # llvm-cov / cargo-llvm-cov rendering
runsible-test env            # show environment metadata
runsible-test shell          # drop into a built test environment
```

Drop `network-integration` and `windows-integration` for v1; merge them into
`integration` with target aliases (`needs/network`, `needs/windows`).

Keep:
- `--changed` mode driven by `git diff` + a generated implication graph.
- The aliases-file convention for integration targets, with TOML
  syntax (`aliases = ["destructive", "needs/root"]` in
  `tests/integration/targets/<name>/runsible.toml`).
- `ansible-test integration` target layout, except all `*.yml` become
  `*.toml`, and `runme.sh` stays as the script-target escape hatch.
- Coverage: `runsible-test --coverage` writes `target/runsible-coverage/`
  and the `coverage` subcommand renders HTML/XML/JSON.

### 4.2 `runsible-test sanity` candidate checks (Rust-native)

What's worth keeping, rephrased for our world:

| ansible-test            | runsible-test                                                     |
|-------------------------|-------------------------------------------------------------------|
| `validate-modules`      | `validate-tasks` — module trait impl matches doc/argspec TOML.    |
| `yamllint`              | `tomllint` — consistency / formatting via `taplo`.                |
| `changelog`             | `changelog` — `CHANGELOG.md` parses & has an `[unreleased]` head. |
| `line-endings`          | Same — UNIX endings only.                                         |
| `shellcheck`            | Same, gated to `runme.sh` files.                                  |
| `symlinks`              | Same.                                                             |
| `integration-aliases`   | Same — every target declares aliases.                             |
| `ignores`               | Replaced by per-target TOML `[skip]`.                             |
| `runtime-metadata`      | Schema-validate `runsible.toml` itself.                           |

Drop everything Python-shape (Section 3.1 / 3.2 / 3.3).

### 4.3 `runsible-lint` surface

Match the CLI shape of `ansible-lint` precisely (muscle memory) but operate
on TOML. The format flags (`pep8`, `json`, `sarif`, `codeclimate`) carry
over verbatim.

Profiles `min`/`basic`/`moderate`/`safety`/`shared`/`production` carry over
with the same names; rules are renamed where the Python-/YAML-isms made the
old name nonsensical (e.g. `yaml` → `toml`, `no-tabs` drops, `risky-octal`
becomes `risky-mode` because TOML can express integers exactly), but rule
*intent* stays.

Configuration file: `.runsible-lint.toml` (TOML, naturally), keeping every
config key from §2.4 in TOML form.

### 4.4 Auto-fix

Keep `--fix`. The transforms that map across:

- `yaml` → `toml` (taplo formatting)
- `name` (casing)
- `key-order` (tasks, plays)
- `fqcn` (always rewrite to fully-qualified module names)
- `no-jinja-when` (drop `{{ }}` from `when:`)
- `no-free-form` (rewrite shorthand args to structured form)
- `partial-become` (insert `become = true` next to `become_user`)
- `command-instead-of-shell`
- `deprecated-local-action`
- `no-log-password`

These are the same eleven we import from upstream, minus the ones that go
away with TOML (`yaml` is replaced; `jinja` rewrites become Tera/MiniJinja
spacing rewrites and only if we keep that surface).

### 4.5 Custom rules

Rust trait, not Python class. Rough sketch of the contract:

```rust
pub trait LintRule: Send + Sync {
    const ID: &'static str;
    const TAGS: &'static [&'static str];
    const SEVERITY: Severity;

    fn match_play(&self, _play: &Play) -> Vec<Violation> { vec![] }
    fn match_task(&self, _task: &Task) -> Vec<Violation> { vec![] }
    fn match_file(&self, _file: &Lintable) -> Vec<Violation> { vec![] }
}
```

Distributed as cdylib plugins discovered via `~/.config/runsible/plugins/`
or via Cargo workspace dependencies, depending on whether we ship a stable
ABI in v1.

### 4.6 Ignore mechanism

Keep both:
- `# noqa: rule-id` inline (in TOML comments).
- `.runsible-lint-ignore` file at the project root.

Drop `tags = ["skip_ansible_lint"]` — superseded by the `noqa` comment.

---

## Appendix A — `ansible-test` quick-reference cheatsheet

```
# Sanity
ansible-test sanity --docker default
ansible-test sanity --test pep8 --test yamllint
ansible-test sanity --list-tests

# Units
ansible-test units --docker default --python 3.13
ansible-test units --docker default --python 3.13 path/to/test_thing.py
ansible-test units --coverage --num-workers 4

# Integration (POSIX)
ansible-test integration --list-targets
ansible-test integration shippable/posix/ --docker fedora
ansible-test integration ping --docker ubuntu -vvv
ansible-test integration --exclude git --allow-destructive

# Coverage
ansible-test units --coverage
ansible-test integration --coverage <target>
ansible-test coverage report
ansible-test coverage html
ansible-test coverage xml
ansible-test coverage erase

# Debug
ansible-test shell --docker default --python 3.13
ansible-test env --show

# CI
ansible-test sanity --changed
ansible-test integration --changed --base-branch main
```

## Appendix B — `ansible-lint` quick-reference cheatsheet

```
# One-shots
ansible-lint
ansible-lint -p                   # parseable / pep8 format
ansible-lint -f sarif --sarif-file out.sarif
ansible-lint -L                   # list every loaded rule
ansible-lint -T                   # list tags
ansible-lint -P                   # list profiles

# Profile / scoping
ansible-lint --profile production
ansible-lint -t idempotency,fqcn
ansible-lint -x yaml,no-changed-when
ansible-lint --enable-list no-log-password,empty-string-compare

# Auto-fix
ansible-lint --fix
ansible-lint --fix all
ansible-lint --fix yaml,fqcn,key-order

# Ignore
ansible-lint --generate-ignore
ansible-lint -i .ansible-lint-ignore

# CI
ansible-lint --strict             # warnings → exit non-zero
ANSIBLE_LINT_NODEPS=1 ansible-lint
```

---

## Appendix C — Source URLs consulted

ansible-test:
- `https://docs.ansible.com/ansible/latest/dev_guide/testing.html`
- `https://docs.ansible.com/ansible/latest/dev_guide/testing_sanity.html`
- `https://docs.ansible.com/ansible/latest/dev_guide/testing_units.html`
- `https://docs.ansible.com/ansible/latest/dev_guide/testing_units_modules.html`
- `https://docs.ansible.com/ansible/latest/dev_guide/testing_integration.html`
- `https://docs.ansible.com/ansible/latest/dev_guide/testing_running_locally.html`
- `https://docs.ansible.com/ansible/latest/dev_guide/testing/sanity/index.html`
- `https://docs.ansible.com/ansible/latest/dev_guide/testing/sanity/validate-modules.html`
- `https://docs.ansible.com/ansible/latest/dev_guide/developing_collections_testing.html`

ansible-lint:
- `https://docs.ansible.com/projects/lint/`
- `https://docs.ansible.com/projects/lint/installing/`
- `https://docs.ansible.com/projects/lint/usage/`
- `https://docs.ansible.com/projects/lint/configuring/`
- `https://docs.ansible.com/projects/lint/profiles/`
- `https://docs.ansible.com/projects/lint/rules/` (every per-rule sub-page)
- `https://docs.ansible.com/projects/lint/autofix/`
- `https://docs.ansible.com/projects/lint/custom-rules/`
