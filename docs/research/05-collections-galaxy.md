# Ansible Collections and Galaxy: Exhaustive Reference

This document is an exhaustive reference for Ansible Collections, Ansible
Galaxy, the legacy role distribution channel, and the `ansible-galaxy` CLI.
It is sourced from the official documentation under
`https://docs.ansible.com/ansible/latest/collections_guide/`,
`https://docs.ansible.com/ansible/latest/galaxy/`, the
`ansible-galaxy` CLI reference, and the developer guide for collection
metadata. It is the design authority for the `runsible-galaxy` crate and
informs how runsible discovers, installs, verifies, and runs third-party
content.

---

## 1. What a Collection Is

A **collection** is "a distribution format for Ansible content. You can
package and distribute playbooks, roles, modules, and plugins using
collections." It is the modern unit of redistribution; the legacy "Galaxy
role" concept still exists but is, in effect, a degenerate single-namespace
artifact.

Collections are referenced by **Fully Qualified Collection Name** (FQCN) in
the form `namespace.collection`, and the content inside them is referenced
as `namespace.collection.module` (or `namespace.collection.role`,
`namespace.collection.playbook`, `namespace.collection.filter`, etc.).
Collections live in `ansible_collections/<namespace>/<collection>/` on disk,
inside one of the configured `COLLECTIONS_PATH` roots.

---

## 2. Collection Directory Layout

A collection's source tree is rooted at the directory containing
`galaxy.yml`. The full canonical layout, drawn from the developer guide:

```
my_namespace/
  my_collection/
    galaxy.yml            # required metadata
    README.md             # required description
    LICENSE               # or COPYING; conventional
    meta/
      runtime.yml         # Ansible version requirement, plugin routing,
                          # action groups, import_redirect
      execution-environment.yml  # optional EE deps
    docs/                 # documentation
                          # certified: .md only at top-level
                          # community: .rst under docsite/rst/
                          # optional docsite/extra-docs.yml
                          # optional docsite/links.yml
    plugins/
      modules/            # modules: foo.py
      action/             # action plugins
      become/             # become plugins (privilege escalation)
      cache/              # fact / inventory cache plugins
      callback/           # event callback plugins
      cliconf/            # network device CLI configs
      connection/         # transport connection plugins
      doc_fragments/      # reusable docstring fragments
      filter/             # Jinja2 filters
      httpapi/            # HTTP API plugins
      inventory/          # inventory plugins
      lookup/             # lookup plugins
      module_utils/       # shared library code, accessible via FQCN
      netconf/            # NETCONF plugins
      shell/              # remote shell plugins
      strategy/           # execution strategy plugins
      terminal/           # network device terminal plugins
      test/               # Jinja2 tests
      vars/               # vars plugins (must be enabled)
    roles/
      role_one/           # name == directory name
        meta/main.yml
        defaults/main.yml
        vars/main.yml
        tasks/main.yml
        handlers/main.yml
        templates/
        files/
        README.md
    playbooks/
      site.yml
      files/              # OK
      vars/               # OK
      templates/          # OK
                          # NOT permitted: roles/ inside playbooks/
    tests/                # ansible-test suites; integration/, unit/, sanity/
    changelogs/           # antsibull-changelog input; changelogs/fragments/
                          # and changelogs/changelog.yaml
    MANIFEST.json         # written by ansible-galaxy collection build
    FILES.json            # written by ansible-galaxy collection build
```

Notable constraints:

- **Role names** in `roles/<name>/` must be lowercase alphanumeric with
  underscores, starting with an alpha. Hyphens are forbidden inside a
  collection. (Standalone Galaxy roles can have hyphens.)
- **Roles inside collections cannot ship plugins**; they use the parent
  collection's `plugins/` tree.
- **Playbooks inside `playbooks/`** cannot have a sibling `roles/`
  subdirectory inside that folder; roles for playbooks come from the
  collection's top-level `roles/` (or any other collection on the path).
- **Documentation file types** depend on collection certification: certified
  (Red Hat Automation Hub) collections accept Markdown in `docs/`
  top-level only (no subdirs); community collections use reStructuredText
  inside `docsite/rst/`.
- **`docs/`** also accepts `docsite/extra-docs.yml` (linking external
  pages) and `docsite/links.yml` (issue trackers, forums, etc.).
- **`module_utils`** is special: its contents are importable from modules
  in *other* collections via FQCN (`from ansible_collections.ns.coll.plugins.module_utils.foo import ...`).
- **`vars/`** plugins must be explicitly enabled via config; they are not
  loaded just because they exist.
- **`cache/`** plugins for inventory caching are not supported via
  collections; only fact-cache plugins are.
- **`README.md`** is required for the collection root and recommended for
  each role in `roles/`.

### 2.1 `meta/runtime.yml`

A YAML file declaring:

- `requires_ansible:` - a version constraint like `">=2.14.0"`. If the
  current Ansible is outside this range the behaviour follows
  `COLLECTIONS_ON_ANSIBLE_VERSION_MISMATCH` (default `warning`; can be
  `error` or `ignore`).
- `plugin_routing:` - per-plugin-type redirects, deprecations, removed
  notices. Used during the great content migration of 2.10+. Supports
  `redirect`, `tombstone` (removed), `deprecation` for each plugin.
- `action_groups:` - named groups of action plugins for
  module-defaults batching.
- `import_redirection:` - rewrites for plugin import paths during
  refactors.

### 2.2 `meta/execution-environment.yml`

Optional; lets EE builds (`ansible-builder`) discover python and system
package dependencies for the collection.

### 2.3 `MANIFEST.json` and `FILES.json`

Both files are generated by `ansible-galaxy collection build`; they are
present in the released tarball, in any extracted-tarball install on disk,
and missing in source checkouts (where `galaxy.yml` is the source of truth).

- **`MANIFEST.json`** is a single JSON document containing:
  - `collection_info`: a snapshot of the relevant `galaxy.yml` fields
    (namespace, name, version, authors, readme, description, license,
    license_file, tags, repository, documentation, homepage, issues,
    dependencies).
  - `file_manifest_file`: `{ name: "FILES.json", ftype, chksum_type:
    "sha256", chksum_sha256, format }` - the SHA-256 of `FILES.json`,
    locking it into the manifest.
  - `format`: an integer (currently `1`).
- **`FILES.json`** is a single JSON document with one entry per file in the
  artifact, each carrying:
  - `name` - relative path inside the tarball.
  - `ftype` - `file` or `dir`.
  - `chksum_type` - currently `sha256` for files; `null` for dirs.
  - `chksum_sha256` - hex digest of the file contents; `null` for dirs.
  - `format` - integer matching `MANIFEST.json`.

`ansible-galaxy collection verify` works by re-hashing each installed
file, comparing it against `FILES.json`, and ensuring the in-place
`MANIFEST.json` matches the server copy. Manifest tampering is detected
because `MANIFEST.json` carries the hash of `FILES.json`; file tampering
is detected because `FILES.json` carries each per-file hash.

---

## 3. `galaxy.yml` Schema

The single source of truth for collection metadata. All fields are
top-level YAML keys.

| Field | Required | Type | Notes |
|-------|----------|------|-------|
| `namespace` | yes | string | Lowercase alphanumeric + `_`. Must not start with a digit or `_`, no consecutive `_`. |
| `name` | yes | string | Same character rules as `namespace`. |
| `version` | yes | string | Semantic version (`MAJOR.MINOR.PATCH`, optionally `-prerelease+build`). |
| `readme` | yes | string | Path to a Markdown README, relative to collection root. |
| `authors` | yes | list of strings | Free-form; convention is `Name <email> (URL)` or IRC handle. |
| `description` | no | string | One-paragraph summary. |
| `license` | no | list of strings | SPDX identifiers only. Mutually exclusive with `license_file`. |
| `license_file` | no | string | Path to license file, relative to collection root. Mutually exclusive with `license`. |
| `tags` | no | list of strings | Same charset as namespace; for searchability. |
| `dependencies` | no | dict | `{ "namespace.collection": "version_spec" }`. Recursive; not for OS deps. |
| `repository` | no | string | URL of source SCM. |
| `documentation` | no | string | URL of online docs. |
| `homepage` | no | string | URL of project homepage. |
| `issues` | no | string | URL of issue tracker. |
| `build_ignore` | no | list of strings | Glob patterns excluded from the built tarball. Available since 2.10. Mutually exclusive with `manifest`. |
| `manifest` | no | dict | MANIFEST.in-style directives. Available since 2.14. Mutually exclusive with `build_ignore`. |
| `manifest.directives` | no | list of strings | Lines of MANIFEST.in syntax (`include`, `recursive-include`, `prune`, etc.). |
| `manifest.omit_default_directives` | no | bool | If true, drop the implicit defaults (see 4.3). |

Things to remember:

- Galaxy enforces SPDX licences only when validating `license`. Bespoke
  licences must use `license_file`.
- `dependencies` here is for **other collections** (`namespace.coll`) - not
  Python packages, not system packages, not Galaxy roles.
- The same field name (`dependencies`) means something different in
  `roles/<role>/meta/main.yml` (see section 7.2).

---

## 4. Building, Publishing, and Distributing Collections

### 4.1 Building

```
ansible-galaxy collection build [--output-path PATH] [-f]
```

run from the collection root produces
`<namespace>-<name>-<version>.tar.gz`. Build steps:

1. Walk the source tree.
2. Apply exclusions: `build_ignore` glob list **or** `manifest` directives
   plus the implicit defaults.
3. Compute SHA-256 of every included file; emit `FILES.json`.
4. Compute SHA-256 of `FILES.json`; emit `MANIFEST.json` containing both
   the `collection_info` and the FILES.json hash.
5. Tar+gzip everything (including the two JSON files) into the artifact.

The current Galaxy upload limit is **20 MB** per tarball.

### 4.2 Publishing

```
ansible-galaxy collection publish path/to/ns-name-ver.tar.gz [options]
```

Uploads the artifact to the configured server. The CLI uses any token
configured in `ansible.cfg` for the chosen server. Direct `--token` on the
command line works but is discouraged because it ends up in shell history.

For **Pulp 3 Galaxy** servers (and Automation Hub), collections may be GPG
signed before upload. The signature is computed over `MANIFEST.json`:

```
tar -Oxzf ns-name-1.0.0.tar.gz MANIFEST.json \
  | gpg --output ns-name-1.0.0.asc --detach-sign --armor \
        --local-user me@example.com -
```

The `.asc` is a *detached* signature; servers store it alongside the
tarball.

A version, once uploaded, **cannot** be modified or removed via the API
(re-uploading the same version returns a conflict). The only path forward
is to publish a new version.

### 4.3 Default `manifest` Directives

When `manifest` is used (or implicitly when neither is set), Ansible applies
this default exclusion list before any user directives:

- `*.pyc`, `*.retry`
- `tests/output`
- VCS dirs (`.git`, `.hg`, `.svn`, `.bzr`)
- Editor leftovers (`.swp`, `.bak`, `~`)
- Previously built tarballs in the repo root

`manifest.omit_default_directives: true` disables this and gives you a clean
slate.

`build_ignore` is a simpler globbing alternative; it does **not** carry the
implicit defaults, so tarballs can balloon if you forget to list `*.pyc`
etc.

---

## 5. Installing, Listing, Verifying, Downloading

### 5.1 Install

```
ansible-galaxy collection install <coll-or-source> [...]
ansible-galaxy collection install -r requirements.yml
```

Default behaviour:

- Resolves against the configured Galaxy server list (see 6).
- Picks the highest stable version satisfying the spec.
- Installs to the first writable directory in `COLLECTIONS_PATH`,
  defaulting to `~/.ansible/collections/`.
- Auto-appends `ansible_collections/` to `-p` if not already present.

Install sources:

- **By name** (`namespace.collection`) - resolved via Galaxy server list.
- **By name with version** - `namespace.collection:1.2.3`,
  `namespace.collection:>=1.0.0,<2.0.0`. Operators: `*` (any, default),
  `==`, `!=`, `>=`, `>`, `<=`, `<`. Pre-releases are excluded by default
  unless `--pre` or an explicit `==1.0.0-beta.1` pin is used.
- **From a local tarball** -
  `ansible-galaxy collection install path/to/ns-name-1.0.0.tar.gz`.
- **From a local directory** -
  `ansible-galaxy collection install /path/to/collection_root`. Requires a
  valid `galaxy.yml`.
- **From a local namespace directory** -
  `ansible-galaxy collection install /path/to/ns_dir` installs every
  collection it contains.
- **From git** -
  `git+https://...`, `git@host:org/repo.git`, `git+file:///...`.
  Append `,branch_or_tag_or_sha` to pin. Append `#/subdir/` to install a
  collection that lives in a subdirectory of the repo (Git URL fragment).
- **From a URL** - any HTTP(S) URL serving a tarball.

Install flags (full list):

| Flag | Purpose |
|------|---------|
| `-p, --collections-path PATH` | Where to write. Default: first entry in `COLLECTIONS_PATH`. |
| `-r, --requirements-file FILE` | Bulk install from a requirements.yml. |
| `-f, --force` | Overwrite the named collection if installed. |
| `--force-with-deps` | `-f` plus overwrite all dependencies. |
| `-U, --upgrade` | Re-resolve and install higher versions if available. |
| `-n, --no-deps` | Skip dependency resolution. |
| `-i, --ignore-errors` | Continue past per-collection failures. |
| `--pre` | Include pre-release versions when resolving. |
| `--offline` | Don't contact any server; only install local sources. |
| `-s, --server URL_OR_NAME` | Pick a single Galaxy server (URL or named server in `[galaxy_server.<name>]`). |
| `--token, --api-key TOKEN` | Use this token for the chosen server. |
| `--timeout SECONDS` | Per-request timeout (default 60). |
| `-c, --ignore-certs` | Skip TLS validation. |
| `--clear-response-cache` | Wipe the `GALAXY_CACHE_DIR` before resolving. |
| `--no-cache` | Bypass the response cache for this run. |
| `--keyring PATH` | GPG keyring for signature verification. |
| `--signature URL_OR_FILE` | Extra detached signatures to require (repeatable). |
| `--required-valid-signature-count N \| all \| +N \| +all` | How many sigs must verify; `+` prefix means "fail if zero". |
| `--ignore-signature-status-codes CODES` | Space-separated GnuPG status codes to treat as non-fatal. |
| `--disable-gpg-verify` | Skip signature verification entirely. |

### 5.2 Adjacent Installation

Project-local install pattern that the docs explicitly bless:

```
project_root/
  play.yml
  ansible.cfg                       # COLLECTIONS_PATH = ./collections
  collections/
    ansible_collections/
      ns/
        coll/
          ...
```

Running `ansible-playbook play.yml` from `project_root/` then resolves
collections from inside the repo, which is the standard pattern for
reproducible environments and CI.

### 5.3 Download

```
ansible-galaxy collection download <coll-or-source> [...]
ansible-galaxy collection download -r requirements.yml [-p DIR]
```

Same source resolution as `install`, but instead of unpacking into the
collections path, it just drops tarballs into `./collections/` (or
`-p`/`--download-path`) and writes a `requirements.yml` alongside that
re-references the local tarballs by `type: file`. The intended workflow is
"download on a connected box, scp to airgapped host, run `install -r
requirements.yml --offline`."

### 5.4 List

```
ansible-galaxy collection list [name]
```

Shows every collection found in every `COLLECTIONS_PATH` root. With a name
argument, shows every install of that collection (so you can spot
shadowing). With `-vvv`, includes dependency-installed collections and
remote URLs.

A collection installed from source (no `MANIFEST.json`, only `galaxy.yml`)
shows version `*` rather than the actual semver, because `MANIFEST.json`
is the only place the build-time version is pinned.

Flags: `-p` (extra search paths, colon-separated), `--format`, the usual
server flags (mostly inert for `list`).

### 5.5 Verify

```
ansible-galaxy collection verify <name>[:version] [...]
ansible-galaxy collection verify -r requirements.yml
```

Re-hashes every installed file and compares against `FILES.json`, then
contacts the server to confirm `MANIFEST.json` matches the server copy.
Output is silent on success; modified files are listed under their
collection.

Flags: `--keyring`, `--signature`, `--required-valid-signature-count`,
`--ignore-signature-status-codes`, `--offline` (skip server hit, verify
only against local `FILES.json`/`MANIFEST.json`), `-i, --ignore-errors`,
`-p, --collections-path`, the server flags. Verify does **not** pull
dependencies; only the explicitly-named collection is checked.

### 5.6 Init

```
ansible-galaxy collection init <namespace>.<collection> [--init-path DIR] [--collection-skeleton DIR] [-f]
```

Creates a skeleton tree at `DIR/<namespace>/<collection>/` populated with
the standard layout, an empty `galaxy.yml`, a starter `README.md`, etc. A
custom skeleton may be supplied; user variables come via `-e/--extra-vars`.

### 5.7 `requirements.yml` Schema

A single file declaring both collections and roles:

```yaml
---
collections:
  - my_namespace.simple                       # short form

  - name: my_namespace.with_version
    version: ">=1.2.0,<2.0.0"
    source: https://galaxy.ansible.com        # default if omitted
    type: galaxy                              # default
    signatures:
      - https://example.com/detached.asc
      - file:///etc/keys/local.asc

  - name: ./vendored/my_namespace/my_collection/
    type: dir

  - name: ./vendored/my_namespace/
    type: subdirs                             # install every coll under

  - name: /opt/cache/ns-coll-1.0.0.tar.gz
    type: file

  - name: https://github.com/org/repo.git
    type: git
    version: devel                            # branch/tag/SHA

  - name: https://example.com/coll-1.0.0.tar.gz
    type: url

roles:
  - name: geerlingguy.java                    # short Galaxy form
    version: "1.9.6"

  - src: https://github.com/example/role.git  # SCM
    name: example_role
    scm: git
    version: main

  - include: webserver.yml                    # split file
```

Collection entry keys: `name`, `version`, `source`, `type`
(`galaxy | git | url | dir | subdirs | file`), `signatures`.

Role entry keys: `src` (or `name` for short form), `name` (install-as),
`scm` (`git` or `hg`; default `git`), `version` (tag/branch/SHA).

The `include:` directive (roles only) splits requirements across files.

### 5.8 Resolution Semantics

Collection version specs use a SemVer-ish operator grammar (`==`, `!=`,
`>=`, `>`, `<=`, `<`, `*`). Multiple operators may be comma-joined
(`>=1.0.0,<2.0.0`). The resolver picks "the most recent version that
satisfies the constraints from any configured server, excluding
pre-releases unless `--pre` or a `==<prerelease>` pin is given."

Conflict policy: if two installed collections require incompatible
versions of a third, the resolver fails; `--ignore-errors` continues anyway
and leaves the path in whatever state the partial install achieved.

Roles do **not** support version ranges. A role's `version:` must be a
single tag, branch, or SHA (or omitted, meaning the repository default).

---

## 6. Galaxy Server Configuration

Servers are configured in `ansible.cfg`:

```ini
[galaxy]
server_list = my_org_hub, release_galaxy, test_galaxy, my_galaxy_ng

[galaxy_server.my_org_hub]
url = https://automation.my_org/
username = my_user
password = my_pass

[galaxy_server.release_galaxy]
url = https://galaxy.ansible.com/
token = my_token

[galaxy_server.test_galaxy]
url = https://galaxy-dev.ansible.com/
token = my_test_token

[galaxy_server.my_galaxy_ng]
url = http://my_galaxy_ng:8000/api/automation-hub/
auth_url = http://my_keycloak:8080/auth/realms/myco/protocol/openid-connect/token
client_id = galaxy-ng
token = my_keycloak_access_token
```

Server config keys:

| Key | Notes |
|-----|-------|
| `url` | Required. **Must end with `/`**. |
| `token` | API token; mutually exclusive with `username`. |
| `username` | Basic auth; mutually exclusive with `token`. |
| `password` | Basic auth password. |
| `auth_url` | Keycloak SSO token endpoint; needs `token`. |
| `client_id` | Keycloak client; default `cloud-services`. |
| `validate_certs` | Default `true`. |
| `timeout` | Per-request timeout. |

`server_list` controls precedence: the resolver tries each named server in
order until it finds the requested collection.

Per-server values may also be set via env vars in the form
`ANSIBLE_GALAXY_SERVER_{ID}_{KEY}`, e.g.
`ANSIBLE_GALAXY_SERVER_RELEASE_GALAXY_TOKEN=secret`.

Global Galaxy config (in `[galaxy]`):

| Setting | Env | Default | Notes |
|---------|-----|---------|-------|
| `server` (`GALAXY_SERVER`) | `ANSIBLE_GALAXY_SERVER` | `https://galaxy.ansible.com` | Used when no `server_list`. |
| `token_path` (`GALAXY_TOKEN_PATH`) | `ANSIBLE_GALAXY_TOKEN_PATH` | `~/.ansible/galaxy_token` | Local cache of API tokens. |
| `cache_dir` (`GALAXY_CACHE_DIR`) | `ANSIBLE_GALAXY_CACHE_DIR` | `~/.ansible/galaxy_cache` | Cached server responses. |
| `ignore_certs` (`GALAXY_IGNORE_CERTS`) | `ANSIBLE_GALAXY_IGNORE` | unset | Skip TLS validation. |
| `disable_gpg_verify` (`GALAXY_DISABLE_GPG_VERIFY`) | `ANSIBLE_GALAXY_DISABLE_GPG_VERIFY` | false | Skip signature verification. |
| `gpg_keyring` (`GALAXY_GPG_KEYRING`) | `ANSIBLE_GALAXY_GPG_KEYRING` | unset | Keyring for signatures. |
| `required_valid_signature_count` | `ANSIBLE_GALAXY_REQUIRED_VALID_SIGNATURE_COUNT` | `1` | `1`, `all`, `+1`, `+all`. |
| `ignore_signature_status_codes` | `ANSIBLE_GALAXY_IGNORE_SIGNATURE_STATUS_CODES` | unset | Space-separated GnuPG codes. |
| `display_progress` (`GALAXY_DISPLAY_PROGRESS`) | `ANSIBLE_GALAXY_DISPLAY_PROGRESS` | unset | Show the spinner. |
| `collections_path_warning` | `ANSIBLE_GALAXY_COLLECTIONS_PATH_WARNING` | true | Warn if `-p` is suspicious. |

Collection-discovery config (in `[defaults]`):

| Setting | Env | Default | Notes |
|---------|-----|---------|-------|
| `collections_path` (`COLLECTIONS_PATHS`) | `ANSIBLE_COLLECTIONS_PATH` | `~/.ansible/collections:/usr/share/ansible/collections` | Colon-separated. |
| `collections_scan_sys_path` | `ANSIBLE_COLLECTIONS_SCAN_SYS_PATH` | true | Also scan Python sys.path. |
| `collections_on_ansible_version_mismatch` | `ANSIBLE_COLLECTIONS_ON_ANSIBLE_VERSION_MISMATCH` | `warning` | One of `error`, `warning`, `ignore`. |

---

## 7. Roles - Both Worlds

### 7.1 Roles Inside Collections

A role lives at `roles/<role_name>/` inside the collection. It is invoked
by FQCN: `namespace.collection.role_name`. It uses the **standard** role
sub-tree (`tasks/`, `handlers/`, `defaults/`, `vars/`, `templates/`,
`files/`, `meta/`) but it **cannot** carry its own `library/`,
`module_utils/`, or `lookup_plugins/` - those go in the collection's
`plugins/` instead. A collection-role's `meta/main.yml` is metadata only
(dependencies, role tags, allow_duplicates); the legacy galaxy_info block
is meaningless because the collection (not the role) is the publishable
unit.

### 7.2 Standalone Galaxy Roles (Legacy)

A standalone role is a directory with the same shape, plus permission to
embed plugins:

```
my_role/
  README.md
  defaults/main.yml
  vars/main.yml
  tasks/main.yml
  handlers/main.yml
  templates/
  files/
  meta/main.yml
  library/             # standalone-only: in-tree custom modules
  module_utils/        # standalone-only
  lookup_plugins/      # standalone-only
  filter_plugins/      # standalone-only
  tests/
    inventory
    test.yml
```

Discoverable role paths default to
`~/.ansible/roles:/usr/share/ansible/roles:/etc/ansible/roles` (override
via `ANSIBLE_ROLES_PATH`, `roles_path` in `[defaults]`, or `--roles-path`
on the CLI).

#### `meta/main.yml` for Standalone Roles

```yaml
galaxy_info:
  role_name: my_role
  namespace: my_namespace
  author: Jane Roe
  description: Configures the foo service.
  company: Acme Corp
  issue_tracker_url: https://github.com/me/my_role/issues
  license: MIT
  min_ansible_version: "2.14"
  min_ansible_container_version: "1.0"
  github_branch: main
  platforms:
    - name: EL
      versions:
        - "8"
        - "9"
    - name: Debian
      versions:
        - bullseye
        - bookworm
  galaxy_tags:
    - system
    - networking

dependencies:
  - role: geerlingguy.java
  - name: composer
    src: git+https://github.com/geerlingguy/ansible-role-composer.git
    version: COMMIT_HASH
```

Field reference (sourced from the galaxy-importer `LegacyGalaxyInfo`
schema):

| Field | Notes |
|-------|-------|
| `role_name` | Short name, no dots, no hyphens (collection rules apply). |
| `namespace` | Galaxy namespace owning the role. |
| `author` | Author name (string or list). |
| `description` | One-line summary. |
| `company` | Optional. |
| `issue_tracker_url` | URL. |
| `license` | SPDX identifier or descriptive string. |
| `min_ansible_version` | String (`"2.14"`). |
| `min_ansible_container_version` | For Ansible Container compatibility. |
| `github_branch` | Default branch when imported. |
| `platforms` | List of `{ name, versions }` dicts; values come from Galaxy's controlled vocabulary (EL, Debian, Ubuntu, FreeBSD, Windows, ...). |
| `galaxy_tags` | List of strings; one or two words each, used for search filters. |

`dependencies:` may be either a list of strings (`namespace.role_name`),
which Galaxy expects to exist on Galaxy, or a list of dicts with `role`,
`name`, or `src` (plus optional `version`, `scm`).

`allow_duplicates: true` (top-level, not inside `galaxy_info`) lets the
same role be applied multiple times in the same play with different vars.
Without this, Ansible runs each role at most once per host per play.

### 7.3 Installing Roles

```
ansible-galaxy role install namespace.role_name           # from Galaxy
ansible-galaxy role install namespace.role_name,1.2.3     # version pin
ansible-galaxy role install -r requirements.yml
ansible-galaxy role install --roles-path ./roles ns.role
```

The role `requirements.yml` schema (see 5.7) supports `src`, `name`,
`scm`, `version`. Roles may not use SemVer ranges; a single tag/branch/SHA
only.

### 7.4 Other Role Subcommands

| Subcommand | Purpose |
|------------|---------|
| `role init <name>` | Skeleton tree (`--type container`, `--type apb`, `--type network` for variants). |
| `role list` | Show installed roles. |
| `role info <name>` | Local + Galaxy metadata. `--offline` skips Galaxy. |
| `role remove <name>` | Delete from local roles_path. |
| `role search <terms>` | Galaxy search (`--author`, `--galaxy-tags`, `--platforms`). |
| `role import <github_user> <github_repo>` | Trigger Galaxy to import from GitHub. `--branch`, `--no-wait`, `--role-name`, `--status`. |
| `role delete <github_user> <github_repo>` | Remove from Galaxy (does not touch GitHub). |
| `role setup <source> <user> <repo> [token]` | GitHub/Travis integration for auto-imports. `--list`, `--remove ID`. |

(Older docs reference `role login`; current Ansible authenticates by token
written to `GALAXY_TOKEN_PATH`, so the subcommand is effectively retired
on modern releases.)

---

## 8. The `ansible-galaxy` CLI - All Subcommands at a Glance

```
ansible-galaxy [-h] [--version] [-v]
  collection {download, init, build, publish, install, list, verify} ...
  role       {init, remove, delete, list, search, import, setup, info, install} ...
```

Global flags: `-h`, `--version`, `-v` (repeatable up to `-vvvvvv`).

Common per-subcommand flags (almost everywhere):

| Flag | Purpose |
|------|---------|
| `-c, --ignore-certs` | Skip TLS validation. |
| `-s, --server URL` | Galaxy API server. |
| `--token, --api-key TOKEN` | Auth token. |
| `--timeout SECONDS` | Per-request (default 60). |

### 8.1 `collection` Subcommands - Full Flag Inventory

#### `collection download`

`--clear-response-cache`, `--no-cache`, `--pre`, `-c`, `-n, --no-deps`,
`-p, --download-path PATH`, `-r, --requirements-file FILE`, `-s`,
`--token`, `--timeout`.

#### `collection init`

`--collection-skeleton PATH`, `--init-path PATH`, `-c`, `-e, --extra-vars`,
`-f, --force`, `-s`, `--token`, `--timeout`.

#### `collection build`

`--output-path PATH`, `-c`, `-f, --force`, `-s`, `--token`, `--timeout`.

#### `collection publish`

`--import-timeout SECS`, `--no-wait`, `-c`, `-s`, `--token`, `--timeout`.

#### `collection install`

(See section 5.1 for the full list, including all signature flags.)

#### `collection list`

`--format FORMAT`, `-c`, `-p, --collections-path PATHS`, `-s`, `--token`,
`--timeout`.

#### `collection verify`

(See 5.5 for the list.)

### 8.2 `role` Subcommands - Full Flag Inventory

#### `role init`

`--init-path PATH`, `--offline`, `--role-skeleton PATH`,
`--type {container, apb, network}`, `-c`, `-e, --extra-vars`, `-f`, `-s`,
`--token`, `--timeout`.

#### `role remove`

`-c`, `-p, --roles-path` (repeatable), `-s`, `--token`, `--timeout`.

#### `role delete`

`-c`, `-s`, `--token`, `--timeout`.

#### `role list`

`-c`, `-p, --roles-path` (repeatable), `-s`, `--token`, `--timeout`.

#### `role search`

`--author USER`, `--galaxy-tags TAGS`, `--platforms PLATFORMS`, `-c`, `-s`,
`--token`, `--timeout`.

#### `role import`

`--branch REF`, `--no-wait`, `--role-name NAME`, `--status`, `-c`, `-s`,
`--token`, `--timeout`.

#### `role setup`

`--list`, `--remove ID`, `-c`, `-p, --roles-path`, `-s`, `--token`,
`--timeout`.

#### `role info`

`--offline`, `-c`, `-p, --roles-path`, `-s`, `--token`, `--timeout`.

#### `role install`

`--force-with-deps`, `-c`, `-f, --force`, `-g, --keep-scm-meta`, `-i,
--ignore-errors`, `-n, --no-deps`, `-p, --roles-path`, `-r, --role-file
FILE`, `-s`, `--token`, `--timeout`.

### 8.3 Concurrency Note

The CLI reference flags this prominently: "None of the CLI tools are
designed to run concurrently with themselves. Use an external scheduler
and/or locking to ensure there are no clashing operations." Two
simultaneous `collection install` runs into the same path can corrupt
each other's writes.

---

## 9. Using Collections in Playbooks

### 9.1 FQCN Everywhere

The recommended idiom is to write every plugin reference as
`namespace.collection.name`:

```yaml
- hosts: all
  tasks:
    - ansible.builtin.copy:
        src: foo
        dest: /etc/foo
    - community.general.ufw:
        rule: allow
        port: "22"
```

`ansible.builtin` (the standard library) and `ansible.legacy` (compat
shim) are always available without listing. Everything else has to be
installed via `ansible-galaxy` or be on the `COLLECTIONS_PATH`.

### 9.2 The `collections:` Keyword

```yaml
- hosts: all
  collections:
    - community.general
    - my_namespace.utils
  tasks:
    - ufw: { rule: allow, port: "22" }      # resolves via search path
```

`collections:` defines a **search path** for unqualified plugin names.
Listed in order, the loader tries each prefix until it finds a match.

The big quirk: **roles do not inherit the playbook's `collections:`.** A
role in `namespace.collection.role_name` carries its own search order
(defined in the role's `meta/main.yml`'s `collections:` block, or implicit
from the role's parent collection). Adding `collections:` at the play
level does **not** propagate. This is the most-cited footgun in modern
Ansible code review and the reason the docs explicitly recommend FQCN
over `collections:`.

### 9.3 Playbooks Inside Collections

Playbooks shipped at `playbooks/foo.yml` inside collection `ns.coll` are
runnable by FQCN:

```
ansible-playbook ns.coll.foo
```

Playbook names must be lowercase alphanumeric or underscore and must start
with an alpha; **hyphens are invalid** in playbook names that are
addressable this way.

### 9.4 Roles in Collections from Playbooks

```yaml
- hosts: all
  roles:
    - role: ns.coll.role_name
      vars: { x: 1 }
```

or with the `import_role` / `include_role` task:

```yaml
- name: do thing
  ansible.builtin.import_role:
    name: ns.coll.role_name
```

---

## 10. Galaxy NG vs galaxy.ansible.com

The Galaxy ecosystem now has two parallel implementations:

- **galaxy.ansible.com** is the original public Galaxy service. It hosts
  community collections and standalone roles. Authentication uses GitHub
  OAuth and an API token written to `GALAXY_TOKEN_PATH`. It accepts
  unsigned collection uploads (and never enforces signatures on download).
- **Galaxy NG** ("Next Gen") is a Pulp-3-based reimplementation that
  powers Red Hat's **Automation Hub** (the certified content channel) and
  any private on-premise hub. It supports GPG signing/verification of
  collections, fine-grained RBAC, namespace ownership, sync-from-upstream,
  and a Keycloak-based SSO. The CLI talks to it through the same
  `ansible-galaxy collection` subcommands; only the server URL and auth
  options change.

When configuring `[galaxy_server.<name>]` for an NG instance you typically
need `url`, `auth_url`, `client_id`, and `token` (the SSO token), as
shown in the multi-server example earlier. The community Galaxy server is
a simpler `url` + `token` (or anonymous read-only).

Practical implications for runsible:

- Treat both as opaque "Galaxy v3 API" peers.
- The auth model differs (token-only on community, OAuth/Keycloak on NG)
  but the install/download/verify wire format is the same.
- Signature verification is opt-in on community, often required on NG.

---

## 11. Versioning, Dependency Resolution, and Conflicts

- Collection versions are **SemVer** (`MAJOR.MINOR.PATCH[-pre][+build]`).
- The resolver prefers the highest version satisfying constraints from any
  configured server. When several servers can serve the same name, the
  first server in `server_list` order that has a satisfying version wins.
- Pre-releases are excluded by default; opt in with `--pre` or pin
  exactly with `==1.0.0-beta.1`.
- Transitive deps come from each collection's `dependencies:` map in
  `galaxy.yml`. The resolver walks the graph, picking versions that
  simultaneously satisfy every constraint. If no such assignment exists,
  it errors out (or with `--ignore-errors`, half-installs and warns).
- `--upgrade` re-resolves the entire graph and replaces installed
  versions with their highest-satisfying versions, even if currently
  installed copies still satisfy the constraints.
- `--no-deps` short-circuits resolution; only the named collection is
  installed, even if its `dependencies:` reference others not present.
- `--force` re-installs the named collection over an existing copy
  (potentially downgrading); `--force-with-deps` does the same for the
  whole dep graph.

Roles, again, do not get any of this: the role resolver is a flat list
that installs whatever single version (tag/branch/SHA) is asked for,
recursing into `meta/main.yml`'s `dependencies` only as a flat name list.

---

## 12. Signature Verification (Detailed)

Signing is detached-PGP-over-`MANIFEST.json`. Verification flow:

1. Server provides one or more `.asc` signatures alongside the tarball.
2. `ansible-galaxy collection install` (or `verify`) downloads them.
3. Each signature is verified against `MANIFEST.json` using the keyring
   at `--keyring` / `GALAXY_GPG_KEYRING`.
4. Each verification produces a GnuPG status code; codes listed in
   `--ignore-signature-status-codes` are not counted as failures.
5. The number of *successful* verifications is compared against
   `--required-valid-signature-count`:
   - `1` (default): need at least one good signature.
   - `all`: every signature must verify.
   - `+1`, `+all`: same as above but **also fail if zero signatures
     were available** (hard requirement that the server signed at all).
6. `--disable-gpg-verify` bypasses the entire flow.

Local detached signatures (provided via `--signature file:///...`) are
stacked on top of any server-provided ones; the count threshold applies
to the union.

`--ignore-signature-status-codes` accepts space-separated GnuPG status
codes (e.g. `NO_PUBKEY`, `KEYEXPIRED`); each one whitelisted means
"don't count this kind of failure against the threshold."

The keyring should be a GnuPG keybox or keyring file (typical:
`~/.ansible/pubring.kbx`), populated with `gpg --import --no-default-keyring
--keyring <path> <key.asc>`.

---

## 13. Quirks and Gotchas

A list of specific pitfalls a runsible-galaxy reimplementation must
either replicate or expose with diagnostics.

1. **`collections:` does not propagate into roles.** As above; the most
   commonly broken assumption.
2. **Role-in-collection vs standalone role.** They have different
   constraints (no plugins inside collection roles; hyphens forbidden in
   collection-role names; `meta/main.yml` `galaxy_info` is meaningful only
   for standalone). Telling them apart matters for any tool that walks
   the tree.
3. **`build_ignore` does not include defaults.** If you switch from the
   implicit defaults to a custom `build_ignore` list, you suddenly start
   shipping `*.pyc`, `.git/`, etc. unless you list them yourself.
4. **`manifest.directives` follow the MANIFEST.in syntax** from Python
   sdist tooling: `include`, `exclude`, `recursive-include`,
   `recursive-exclude`, `global-include`, `global-exclude`, `prune`,
   `graft`. Not glob lists.
5. **`MANIFEST.json` is missing in source trees.** Tools that assume it's
   always present break on freshly-cloned collections; check for
   `galaxy.yml` first, fall back to `MANIFEST.json`.
6. **`FILES.json` SHA-256 is per-file.** Edits to one file invalidate
   only that entry; the manifest itself doesn't carry a top-level
   "tree-hash". Tampering with both `FILES.json` and a single file is
   detectable only because `MANIFEST.json` separately hashes
   `FILES.json`.
7. **20 MB tarball limit.** Public Galaxy enforces it; private hubs may
   not, leading to "works in my hub, fails on upstream sync."
8. **Pre-releases require explicit opt-in or `==` pin.** A collection that
   only ever publishes `0.x.y-rc1` will appear "uninstallable" without
   `--pre`.
9. **Roles cannot use SemVer ranges.** `"1.9.6"` works, `">=1.9,<2"`
   does not.
10. **Adjacent install path handling.** `-p ./collections` appends
    `ansible_collections` automatically *unless* the path already ends in
    `ansible_collections`. So `-p ./collections/ansible_collections`
    works, but `-p ./collections/ansible_collections/sub` will append
    again, producing
    `./collections/ansible_collections/sub/ansible_collections/`. Ugly.
11. **`server_list` order matters more than version.** A higher version on
    a later server is ignored if an acceptable lower version exists on an
    earlier one.
12. **Signature verification on `verify` is independent of install-time
    verification.** A collection installed with `--disable-gpg-verify`
    can still be `verify`'d later (or not).
13. **`token_path` is shared between Galaxy CLIs.** The token written by
    one `ansible-galaxy login` (or by the Galaxy web UI's "API token"
    feature) is reused by every subsequent invocation - including
    sub-shells in a CI runner.
14. **Importing from GitHub** (`role import`, `collection publish` via
    Galaxy's GitHub-trigger import) requires a webhook integration: the
    docs note that "Using the `import`, `delete` and `setup` commands to
    manage your roles on the Galaxy website requires authentication in
    the form of an API key."
15. **Role `dependencies:`** in `meta/main.yml` are name-only references
    that "Galaxy expects all role dependencies to exist in Galaxy" -
    deps that are only in a private SCM cannot be auto-resolved this way
    without a SCM-form entry.
16. **`collections:` in `meta/main.yml` of a role inside a collection is
    redundant** for the role's own collection (it's auto-added) but
    needed for any *other* collection the role uses.
17. **Playbook names must be valid Python identifiers** when shipped from
    a collection - lowercase alphanumeric and underscore only, starting
    with alpha.
18. **`role login` is effectively dead.** Modern auth flows pre-populate
    `GALAXY_TOKEN_PATH` from the web UI; the subcommand is documented
    but rarely used.
19. **`include:` in role requirements is roles-only**; collections do
    not have an equivalent splitting mechanism inside a single
    `requirements.yml`.

---

## 14. Implementation Notes for `runsible-galaxy`

Constraints for a runsible reimplementation that wants to interoperate
seamlessly with existing collections and Galaxy infrastructure:

- **Tree layout:** read and write the canonical layout in section 2
  exactly. Treat `MANIFEST.json` as authoritative when present and
  `galaxy.yml` as the development-time source.
- **Build:** produce tarballs whose SHA-256 inputs and JSON encoding
  match the upstream `ansible-galaxy collection build` byte-for-byte
  (modulo file ordering, which is deterministic). Honour
  `build_ignore` and `manifest` (including the implicit defaults)
  identically.
- **Resolver:** SemVer-range resolution across multiple Galaxy v3
  servers, in `server_list` order, with `--upgrade`, `--pre`,
  `--no-deps`, `--force-with-deps` semantics matching upstream.
- **Sources:** support `galaxy`, `git` (with `,ref` and `#/subdir/`),
  `url`, `dir`, `subdirs`, `file`. Honour the role-style `src` / `name`
  / `scm` / `version` keys for the role half.
- **Signature verify:** GPG detached over `MANIFEST.json`, with the same
  count thresholds (`1`, `all`, `+1`, `+all`) and the same
  status-code-ignore mechanism.
- **Loader / runtime:** when an unqualified plugin name is used inside a
  play's `collections:` block, search those collections in order before
  falling back to `ansible.builtin` and `ansible.legacy`. Do **not**
  propagate the play-level `collections:` into invoked roles - reproduce
  the upstream scoping bug rather than silently fixing it (provide a
  config flag `runsible.fix_role_collection_scope = true` for users who
  want the saner behaviour, defaulting off).
- **Galaxy NG vs community:** abstract behind a common Galaxy v3 client.
  Auth: token-only (community), Keycloak/OAuth (NG). Keep the wire
  format detection identical.
- **Cache:** respect `GALAXY_CACHE_DIR`, `--no-cache`,
  `--clear-response-cache`. The cache is a JSON-on-disk store of
  per-URL responses with a TTL.
- **Concurrency:** the upstream tools are not concurrency-safe; runsible
  should provide a lockfile under `~/.ansible/.galaxy.lock` (or
  configurable) and serialise install/verify/build operations against
  the same path. This is an *improvement* over upstream and is worth
  keeping on by default.
- **Roles - both flavours:** support both standalone-role and
  collection-role layouts; understand that the former may carry
  `library/`, `module_utils/`, `lookup_plugins/`, `filter_plugins/`,
  while the latter must not.
- **TOML port:** runsible's house style is TOML-first. The sane mapping
  is `galaxy.toml` and `requirements.toml` as native, with full
  bidirectional conversion to/from `galaxy.yml` and `requirements.yml`
  for compatibility. The on-disk artefact format (the tarball and its
  embedded `MANIFEST.json` / `FILES.json`) must remain JSON to keep
  upstream tooling working.

---

## 15. Summary Table

| Aspect | Value |
|--------|-------|
| Distribution unit | Collection (`namespace.collection`) |
| Reference syntax | FQCN: `namespace.collection.plugin_or_role_or_playbook` |
| Metadata file | `galaxy.yml` (source) / `MANIFEST.json` (built) |
| Per-file integrity | `FILES.json` (sha256 per entry) |
| Manifest integrity | `MANIFEST.json` carries sha256 of `FILES.json` |
| Signing | Detached PGP/ASCII-armor over `MANIFEST.json` |
| Versioning | SemVer with full operator range (`==`, `>=`, ...) |
| Pre-releases | Excluded by default; need `--pre` or `==pin` |
| Default install path | `~/.ansible/collections/ansible_collections/` |
| Default servers | `https://galaxy.ansible.com` |
| Tarball name | `<namespace>-<name>-<version>.tar.gz` |
| Tarball size limit | 20 MB (community Galaxy) |
| Roles in collections | `roles/<name>/`, no plugins, no hyphens |
| Standalone roles | `~/.ansible/roles/...`, may carry `library/` etc. |
| Role versioning | Single tag/branch/SHA only - no ranges |
| Role metadata | `meta/main.yml` with `galaxy_info` + `dependencies` |
| Plugin types | 19 (action, become, cache, callback, cliconf, connection, doc_fragments, filter, httpapi, inventory, lookup, module_utils, modules, netconf, shell, strategy, terminal, test, vars) |
| Concurrency | Not safe; the docs say so |

This is the entire surface a compatible implementation must replicate;
runsible's improvements (lockfile concurrency, TOML-native metadata,
strict-by-default `collections:` scoping) should layer cleanly on top
without breaking interoperability with vanilla Ansible artefacts.
