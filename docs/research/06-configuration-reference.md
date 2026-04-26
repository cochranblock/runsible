# Ansible Configuration Reference - Exhaustive Survey

> Research dossier for `runsible-config`. Sourced from `docs.ansible.com/ansible/latest/reference_appendices/*` and the corresponding plugin pages, current as of ansible-core 2.20 (Nov 2025 release). Targets implementation requirements for the Rust+TOML reimagining; not user documentation.

---

## 1. ansible.cfg Search Precedence

Ansible looks for `ansible.cfg` (the legacy INI configuration file) in a strict, **first-match-wins** order. Once a file is found, all other locations are ignored - settings are NOT merged across files.

### 1.1 Search order (highest to lowest)

| Order | Source | Notes |
|-------|--------|-------|
| 1 | `ANSIBLE_CONFIG` environment variable | Absolute path to the chosen config file. Wins unconditionally if set and pointing at a readable file. |
| 2 | `./ansible.cfg` | Located in the current working directory. Most common in repo-local usage. |
| 3 | `~/.ansible.cfg` | Per-user fallback in the invoking user's home directory. |
| 4 | `/etc/ansible/ansible.cfg` | System-wide default. Shipped by package managers. |

If none of the above exist, Ansible runs with built-in defaults only. There is no implicit Galaxy server config in that case.

### 1.2 Quirks and security caveats

- **World-writable directory rule**: Ansible refuses to load `./ansible.cfg` if the current working directory is world-writable (mode `o+w`). It silently skips that source and continues to `~/.ansible.cfg`. Setting `ANSIBLE_CONFIG=./ansible.cfg` bypasses the check, but is a documented foot-gun.
- **No merging**: There is no concept of layered config in Ansible. Unlike Git, you cannot have a global `~/.ansible.cfg` augmented by a per-repo `./ansible.cfg`. The most-specific found file is the only one read. (The same INI keys can still be overridden by environment variables and CLI flags downstream.)
- **Comment markers**: Both `#` and `;` work at the start of a line; only `;` works for inline comments after a value.
- **Relative paths inside the file**: Path-typed keys may be relative; they resolve relative to the config file's directory, not CWD. The `{{CWD}}` macro expands to the current working directory but is discouraged for security reasons.
- **Stale precedence inversion**: Environment variables ALWAYS override values read from `ansible.cfg`. The reverse never applies. This is the source of most "why is my config not taking effect?" support tickets.
- **`ansible_config_file` magic var**: The full path of the file actually loaded is exposed as the magic variable `ansible_config_file` at runtime, so plays can introspect their own config.
- **`ANSIBLE_HOME` macro**: Many path defaults are expressed as `{{ ANSIBLE_HOME ~ "/..." }}`. `ANSIBLE_HOME` defaults to `~/.ansible`; setting it relocates the cache, plugins, galaxy_token, and persistent connection sockets in a single sweep.

### 1.3 What "first-match" means for runsible

For runsible's TOML-native equivalent, the equivalent rule should be explicit and documented: pick a single config file (env var > CWD > XDG_CONFIG_HOME > /etc), but CONSIDER providing an opt-in merge mode (Cargo-style inheritance) since Ansible's lack of merging is widely regretted.

---

## 2. Configuration Keys (the big table)

The Ansible config reference lists ~250 keys across about a dozen INI sections. Below is the most exhaustive practical extract, grouped by section. Where a key is both an INI option and an environment variable, both names are given. Defaults reference the values shipped with ansible-core 2.18-2.20.

Type abbreviations: `bool`, `int`, `float`, `str`, `path` (single path, may be relative), `pathspec` (colon-separated path list), `pathlist` (comma-separated path list), `list` (typed sequence), `tmppath` (auto-created temp directory), `raw` (passthrough, no coercion).

### 2.1 [defaults] - core engine settings

| Key | Env Var | Type | Default | Purpose |
|---|---|---|---|---|
| `action_warnings` | `ANSIBLE_ACTION_WARNINGS` | bool | `True` | Toggle warnings emitted by task actions. |
| `agnostic_become_prompt` | `ANSIBLE_AGNOSTIC_BECOME_PROMPT` | bool | `True` | Display generic become prompt instead of method-specific. |
| `allow_broken_conditionals` | `ANSIBLE_ALLOW_BROKEN_CONDITIONALS` | bool | `False` | Allow non-boolean conditional results (deprecation warning). |
| `allow_embedded_templates` | `ANSIBLE_ALLOW_EMBEDDED_TEMPLATES` | bool | `True` | Allow templates inside specific backward-compat scenarios. |
| `ansible_managed` | n/a | str | `Ansible managed` | Macro substituted into the `template` module's `{{ ansible_managed }}` (deprecated key). |
| `any_errors_fatal` | `ANSIBLE_ANY_ERRORS_FATAL` | bool | `False` | Make any task failure fatal play-wide. |
| `ask_pass` | `ANSIBLE_ASK_PASS` | bool | `False` | Prompt for the SSH password. |
| `ask_vault_pass` | `ANSIBLE_ASK_VAULT_PASS` | bool | `False` | Prompt for the vault password. |
| `become_password_file` | `ANSIBLE_BECOME_PASSWORD_FILE` | path | `None` | File (or executable that prints) the become password. |
| `become_plugins` | `ANSIBLE_BECOME_PLUGINS` | pathspec | `~/.ansible/plugins/become:/usr/share/ansible/plugins/become` | Search path for become plugins. |
| `cache_plugin` | `ANSIBLE_CACHE_PLUGIN` | str | `memory` | Fact cache backend (`memory`, `jsonfile`, `redis`, ...). |
| `cache_plugin_connection` | `ANSIBLE_CACHE_PLUGIN_CONNECTION` | str | `None` | Connection string/path for fact cache backend. |
| `cache_plugin_prefix` | `ANSIBLE_CACHE_PLUGIN_PREFIX` | str | `ansible_facts` | Prefix for fact cache keys/files. |
| `cache_plugin_timeout` | `ANSIBLE_CACHE_PLUGIN_TIMEOUT` | int | `86400` | Fact cache TTL in seconds. |
| `cache_plugins` | `ANSIBLE_CACHE_PLUGINS` | pathspec | `~/.ansible/plugins/cache:/usr/share/ansible/plugins/cache` | Search path for cache plugins. |
| `callback_plugins` | `ANSIBLE_CALLBACK_PLUGINS` | pathspec | `~/.ansible/plugins/callback:/usr/share/ansible/plugins/callback` | Search path for callback plugins. |
| `callbacks_enabled` | `ANSIBLE_CALLBACKS_ENABLED` | list | `[]` | Whitelist of non-stdout callbacks to load. |
| `cliconf_plugins` | `ANSIBLE_CLICONF_PLUGINS` | pathspec | `~/.ansible/plugins/cliconf:/usr/share/ansible/plugins/cliconf` | Search path for cliconf plugins. |
| `collections_on_ansible_version_mismatch` | `ANSIBLE_COLLECTIONS_ON_ANSIBLE_VERSION_MISMATCH` | str | `warning` | Action on version-mismatched collections (`warning`/`ignore`/`error`). |
| `collections_path` | `ANSIBLE_COLLECTIONS_PATH` (alias `COLLECTIONS_PATHS`) | pathspec | `~/.ansible/collections:/usr/share/ansible/collections` | Search path for installed collections. |
| `collections_scan_sys_path` | `ANSIBLE_COLLECTIONS_SCAN_SYS_PATH` | bool | `True` | Scan Python sys.path for installed collections. |
| `connection_password_file` | `ANSIBLE_CONNECTION_PASSWORD_FILE` | path | `None` | File providing the connection plugin password. |
| `connection_plugins` | `ANSIBLE_CONNECTION_PLUGINS` | pathspec | `~/.ansible/plugins/connection:/usr/share/ansible/plugins/connection` | Search path for connection plugins. |
| `cowpath` | `ANSIBLE_COW_PATH` | str | `None` | Path to a custom cowsay binary. |
| `cow_selection` | `ANSIBLE_COW_SELECTION` | str | `default` | Cow stencil to use, or `random`. |
| `cowsay_enabled_stencils` | `ANSIBLE_COW_ACCEPTLIST` | list | `[bud-frogs, bunny, cheese, daemon, ...]` | Allowed cowsay stencils when `cow_selection=random`. |
| `debug` | `ANSIBLE_DEBUG` | bool | `False` | Enable verbose internal debug output. |
| `deprecation_warnings` | `ANSIBLE_DEPRECATION_WARNINGS` | bool | `True` | Show deprecation warnings to the user. |
| `devel_warning` | `ANSIBLE_DEVEL_WARNING` | bool | `True` | Warn when running an unstable/devel build. |
| `display_args_to_stdout` | `ANSIBLE_DISPLAY_ARGS_TO_STDOUT` | bool | `False` | Echo task args in the per-task header line. |
| `display_skipped_hosts` | `ANSIBLE_DISPLAY_SKIPPED_HOSTS` | bool | `True` | Print `skipping: [host]` for skipped tasks. |
| `display_ok_hosts` | `ANSIBLE_DISPLAY_OK_HOSTS` | bool | `True` | Print `ok: [host]` lines (`default` callback option). |
| `display_failed_stderr` | `ANSIBLE_DISPLAY_FAILED_STDERR` | bool | `False` | Send failures to stderr stream. |
| `display_traceback` | `ANSIBLE_DISPLAY_TRACEBACK` | list | `[never]` | When to attach Python tracebacks (`never`, `error`, `always`, ...). |
| `doc_fragment_plugins` | `ANSIBLE_DOC_FRAGMENT_PLUGINS` | pathspec | `~/.ansible/plugins/doc_fragments:/usr/share/ansible/plugins/doc_fragments` | Search path for doc fragment plugins. |
| `docsite_root_url` | n/a | str | `https://docs.ansible.com/ansible-core/` | Base URL for ansible-doc deep links. |
| `duplicate_dict_key` | `ANSIBLE_DUPLICATE_YAML_DICT_KEY` | str | `warn` | Action on duplicate YAML keys (`warn`/`error`/`ignore`). |
| `editor` | `ANSIBLE_EDITOR` (or `EDITOR`) | str | `vi` | Editor for `ansible-vault edit`, etc. |
| `enable_task_debugger` | `ANSIBLE_ENABLE_TASK_DEBUGGER` | bool | `False` | Drop into the interactive task debugger on failure. |
| `error_on_missing_handler` | `ANSIBLE_ERROR_ON_MISSING_HANDLER` | bool | `True` | Fail when `notify` references a handler that doesn't exist. |
| `error_on_undefined_vars` | `ANSIBLE_ERROR_ON_UNDEFINED_VARS` | bool | `True` | Fail on undefined variable use (deprecated; effectively always true). |
| `executable` | `ANSIBLE_EXECUTABLE` | path | `/bin/sh` | Shell used for command execution on remote. |
| `fact_caching` | alias for `cache_plugin` | str | `memory` | Same as `cache_plugin`; older name. |
| `fact_caching_connection` | alias for `cache_plugin_connection` | str | `None` | Same as `cache_plugin_connection`. |
| `fact_caching_prefix` | alias for `cache_plugin_prefix` | str | `ansible_facts` | Same as `cache_plugin_prefix`. |
| `fact_caching_timeout` | alias for `cache_plugin_timeout` | int | `86400` | Same as `cache_plugin_timeout`. |
| `facts_modules` | `ANSIBLE_FACTS_MODULES` | list | `[smart]` | Modules executed during fact gathering. |
| `filter_plugins` | `ANSIBLE_FILTER_PLUGINS` | pathspec | `~/.ansible/plugins/filter:/usr/share/ansible/plugins/filter` | Search path for Jinja2 filter plugins. |
| `force_color` | `ANSIBLE_FORCE_COLOR` | bool | `False` | Force ANSI color even when stdout is not a TTY. |
| `force_handlers` | `ANSIBLE_FORCE_HANDLERS` | bool | `False` | Run handlers even after task failures. |
| `force_valid_group_names` | `ANSIBLE_TRANSFORM_INVALID_GROUP_CHARS` | str | `never` | Sanitize characters in group names (`never`/`always`/`silently`/`ignore`). |
| `forks` | `ANSIBLE_FORKS` | int | `5` | Max parallel host workers per play. |
| `gather_subset` | `ANSIBLE_GATHER_SUBSET` | list | `['all']` | Default subset(s) for the `setup` module. |
| `gather_timeout` | `ANSIBLE_GATHER_TIMEOUT` | int | `10` | Per-host fact gather timeout. |
| `gathering` | `ANSIBLE_GATHERING` | str | `implicit` | Gather facts policy (`implicit`/`explicit`/`smart`). |
| `hash_behaviour` | `ANSIBLE_HASH_BEHAVIOUR` | str | `replace` | Dict merge mode (`replace` or `merge`). |
| `home` | `ANSIBLE_HOME` | path | `~/.ansible` | Root of Ansible's per-user state. |
| `host_key_checking` | `ANSIBLE_HOST_KEY_CHECKING` | bool | `True` | Validate SSH known_hosts. |
| `httpapi_plugins` | `ANSIBLE_HTTPAPI_PLUGINS` | pathspec | `~/.ansible/plugins/httpapi:/usr/share/ansible/plugins/httpapi` | Search path for HTTP API plugins. |
| `inject_facts_as_vars` | `ANSIBLE_INJECT_FACT_VARS` | bool | `True` | Expose facts as bare top-level variables (legacy behavior). |
| `internal_poll_interval` | n/a | float | `0.001` | Sleep granularity in Ansible's main loop (seconds). |
| `interpreter_python` | `ANSIBLE_PYTHON_INTERPRETER` | str | `auto` | Python interpreter strategy on managed nodes. |
| `interpreter_python_fallback` | n/a | list | `[python3.14, python3.13, python3.12, python3.11, python3.10, python3.9, python3.8, /usr/bin/python3, python3]` | Fallback list when discovery is needed. |
| `invalid_task_attribute_failed` | `ANSIBLE_INVALID_TASK_ATTRIBUTE_FAILED` | bool | `True` | Fail on unknown task keywords. |
| `inventory` | `ANSIBLE_INVENTORY` | pathlist | `[/etc/ansible/hosts]` | Default inventory source(s). |
| `inventory_plugins` | `ANSIBLE_INVENTORY_PLUGINS` | pathspec | `~/.ansible/plugins/inventory:/usr/share/ansible/plugins/inventory` | Search path for inventory plugins. |
| `jinja2_extensions` | `ANSIBLE_JINJA2_EXTENSIONS` | list | `[]` | Extra Jinja2 extensions to load (deprecated for direct use). |
| `jinja2_native` | `ANSIBLE_JINJA2_NATIVE` | bool | `True` | Preserve Python types from templates instead of stringifying. |
| `keep_remote_files` | `ANSIBLE_KEEP_REMOTE_FILES` | bool | `False` | Don't delete temp files on managed node after task. |
| `library` | `ANSIBLE_LIBRARY` | pathspec | `~/.ansible/plugins/modules:/usr/share/ansible/plugins/modules` | Search path for legacy (non-collection) modules. |
| `local_tmp` | `ANSIBLE_LOCAL_TEMP` | tmppath | `~/.ansible/tmp` | Controller-side scratch dir. |
| `localhost_warning` | `ANSIBLE_LOCALHOST_WARNING` | bool | `True` | Warn when implicit localhost-only inventory is used. |
| `log_filter` | `ANSIBLE_LOG_FILTER` | list | `[]` | Logger names to drop from log output. |
| `log_path` | `ANSIBLE_LOG_PATH` | path | `None` | Log file (logging is off by default). |
| `log_verbosity` | `ANSIBLE_LOG_VERBOSITY` | int | `3` | Verbosity level for the log file. |
| `lookup_plugins` | `ANSIBLE_LOOKUP_PLUGINS` | pathspec | `~/.ansible/plugins/lookup:/usr/share/ansible/plugins/lookup` | Search path for lookup plugins. |
| `max_diff_size` | `ANSIBLE_MAX_DIFF_SIZE` | int | `104448` | Max byte size for `--diff` output (≈100KB). |
| `module_args` | `ANSIBLE_MODULE_ARGS` | str | `None` | Default args for `ansible` ad-hoc invocations. |
| `module_compression` | n/a | str | `ZIP_DEFLATED` | Compression for the AnsibleZ module bundle. |
| `module_ignore_exts` | `ANSIBLE_MODULE_IGNORE_EXTS` | list | `[.pyc, .pyo, .swp, .bak, ~, .rpm, .md, .txt, .rst]` | Extensions skipped while loading a module dir. |
| `module_name` | n/a | str | `command` | Default module for `ansible -m`. |
| `module_strict_utf8_response` | `ANSIBLE_MODULE_STRICT_UTF8_RESPONSE` | bool | `True` | Reject non-UTF-8 module responses. |
| `module_utils` | `ANSIBLE_MODULE_UTILS` | pathspec | `~/.ansible/plugins/module_utils:/usr/share/ansible/plugins/module_utils` | Search path for shared module utilities. |
| `netconf_plugins` | `ANSIBLE_NETCONF_PLUGINS` | pathspec | `~/.ansible/plugins/netconf:/usr/share/ansible/plugins/netconf` | Search path for Netconf plugins. |
| `network_group_modules` | `ANSIBLE_NETWORK_GROUP_MODULES` | list | `[eos, nxos, ios, iosxr, junos, enos, ce, vyos, sros, ...]` | Module name prefixes treated as network/CLI. |
| `no_log` | `ANSIBLE_NO_LOG` | bool | `False` | Globally suppress task arguments/results in logs. |
| `no_target_syslog` | `ANSIBLE_NO_TARGET_SYSLOG` | bool | `False` | Disable syslog on managed nodes. |
| `nocolor` | `ANSIBLE_NOCOLOR` (or `NO_COLOR`) | bool | `False` | Disable ANSI color output. |
| `nocows` | `ANSIBLE_NOCOWS` | bool | `False` | Disable cowsay banners. |
| `null_representation` | `ANSIBLE_NULL_REPRESENTATION` | raw | `None` | What `None` becomes after templating (deprecated). |
| `old_plugin_cache_clear` | `ANSIBLE_OLD_PLUGIN_CACHE_CLEAR` | bool | `False` | Use legacy plugin cache invalidation logic. |
| `pager` | `ANSIBLE_PAGER` (or `PAGER`) | str | `less` | Pager for ansible-doc output. |
| `playbook_dir` | `ANSIBLE_PLAYBOOK_DIR` | path | `None` | Default `--playbook-dir` for ad-hoc tools. |
| `playbook_vars_root` | `ANSIBLE_PLAYBOOK_VARS_ROOT` | str | `top` | Where to look for `host_vars`/`group_vars` (`top`/`bottom`/`all`). |
| `plugin_filters_cfg` | n/a | path | `None` | YAML file blocking specific modules/plugins by name. |
| `poll_interval` | `ANSIBLE_POLL_INTERVAL` | int | `15` | Polling interval (seconds) for async task status. |
| `private_key_file` | `ANSIBLE_PRIVATE_KEY_FILE` | path | `None` | Default SSH private key. |
| `private_role_vars` | `ANSIBLE_PRIVATE_ROLE_VARS` | bool | `False` | Don't promote role vars/defaults out of the role. |
| `python_module_rlimit_nofile` | `ANSIBLE_PYTHON_MODULE_RLIMIT_NOFILE` | int | `0` | Soft `RLIMIT_NOFILE` for spawned Python modules (0 = leave alone). |
| `remote_port` | `ANSIBLE_REMOTE_PORT` | int | `None` | Default remote port (delegates to plugin default if unset). |
| `remote_user` | `ANSIBLE_REMOTE_USER` | str | (current user) | Default remote login user. |
| `retry_files_enabled` | `ANSIBLE_RETRY_FILES_ENABLED` | bool | `False` | Generate `.retry` files on partial play failure. |
| `retry_files_save_path` | `ANSIBLE_RETRY_FILES_SAVE_PATH` | path | `None` | Directory for `.retry` files (CWD if unset). |
| `roles_path` | `ANSIBLE_ROLES_PATH` | pathspec | `~/.ansible/roles:/usr/share/ansible/roles:/etc/ansible/roles` | Search path for standalone (non-collection) roles. |
| `run_vars_plugins` | `ANSIBLE_RUN_VARS_PLUGINS` | str | `demand` | When to invoke vars plugins (`demand` = lazily, `start` = eager). |
| `show_custom_stats` | `ANSIBLE_SHOW_CUSTOM_STATS` | bool | `False` | Print `set_stats` in the `PLAY RECAP`. |
| `stdout_callback` | `ANSIBLE_STDOUT_CALLBACK` | str | `default` | Primary callback that owns stdout (`default`/`yaml`/`json`/`minimal`/`oneline`/`unixy`/etc.). |
| `strategy` | `ANSIBLE_STRATEGY` | str | `linear` | Default play execution strategy (`linear`/`free`/`debug`/`host_pinned`). |
| `strategy_plugins` | `ANSIBLE_STRATEGY_PLUGINS` | pathspec | `~/.ansible/plugins/strategy:/usr/share/ansible/plugins/strategy` | Search path for strategy plugins. |
| `su` | `ANSIBLE_SU` | bool | `False` | (Legacy) Use `su` for privilege escalation. |
| `syslog_facility` | `ANSIBLE_SYSLOG_FACILITY` | str | `LOG_USER` | Syslog facility on managed nodes. |
| `system_warnings` | `ANSIBLE_SYSTEM_WARNINGS` | bool | `True` | Show warnings about controller environment. |
| `target_log_info` | `ANSIBLE_TARGET_LOG_INFO` | str | `None` | Free-form string injected into target syslog logging. |
| `task_debugger_ignore_errors` | `ANSIBLE_TASK_DEBUGGER_IGNORE_ERRORS` | bool | `True` | Honor task `ignore_errors` even when debugger is enabled. |
| `task_timeout` | `ANSIBLE_TASK_TIMEOUT` | int | `0` | Hard wall-clock per-task timeout in seconds (0 = unlimited). |
| `terminal_plugins` | `ANSIBLE_TERMINAL_PLUGINS` | pathspec | `~/.ansible/plugins/terminal:/usr/share/ansible/plugins/terminal` | Search path for terminal plugins. |
| `test_plugins` | `ANSIBLE_TEST_PLUGINS` | pathspec | `~/.ansible/plugins/test:/usr/share/ansible/plugins/test` | Search path for Jinja2 test plugins. |
| `timeout` | `ANSIBLE_TIMEOUT` | int | `10` | Default connection timeout (seconds). |
| `transport` | `ANSIBLE_TRANSPORT` | str | `ssh` | Default connection plugin name. |
| `use_persistent_connections` | `ANSIBLE_USE_PERSISTENT_CONNECTIONS` | bool | `False` | Toggle persistent connection feature. |
| `vars_plugins` | `ANSIBLE_VARS_PLUGINS` | pathspec | `~/.ansible/plugins/vars:/usr/share/ansible/plugins/vars` | Search path for vars plugins. |
| `vault_encrypt_identity` | `ANSIBLE_VAULT_ENCRYPT_IDENTITY` | str | `None` | Vault ID used by `ansible-vault encrypt` by default. |
| `vault_id_match` | `ANSIBLE_VAULT_ID_MATCH` | bool | `False` | Only attempt the matching vault ID on decrypt (security). |
| `vault_identity` | `ANSIBLE_VAULT_IDENTITY` | str | `default` | Default vault ID label. |
| `vault_identity_list` | `ANSIBLE_VAULT_IDENTITY_LIST` | list | `[]` | Default `--vault-id` set. |
| `vault_password_file` | `ANSIBLE_VAULT_PASSWORD_FILE` | path | `None` | Default file (or executable) yielding the vault password. |
| `verbosity` | `ANSIBLE_VERBOSITY` | int | `0` | Default verbosity (0-7, mirrors `-v` count). |
| `verbose_to_stderr` | `ANSIBLE_VERBOSE_TO_STDERR` | bool | `False` | Send verbose lines to stderr instead of stdout. |
| `win_async_startup_timeout` | `ANSIBLE_WIN_ASYNC_STARTUP_TIMEOUT` | int | `5` | Seconds Windows async wrapper waits for backgrounded task to start. |
| `worker_shutdown_poll_count` | `ANSIBLE_WORKER_SHUTDOWN_POLL_COUNT` | int | `0` | How many times to poll a worker before forcibly killing it on shutdown. |
| `worker_shutdown_poll_delay` | `ANSIBLE_WORKER_SHUTDOWN_POLL_DELAY` | float | `0.1` | Delay (seconds) between worker shutdown polls. |
| `yaml_filename_extensions` | `ANSIBLE_YAML_FILENAME_EXT` | list | `['.yml', '.yaml', '.json']` | Extensions matched as YAML/JSON when reading vars dirs. |

### 2.2 [inventory]

| Key | Env Var | Type | Default | Purpose |
|---|---|---|---|---|
| `cache` | `ANSIBLE_INVENTORY_CACHE` | bool | `False` | Enable inventory plugin caching. |
| `cache_connection` | `ANSIBLE_INVENTORY_CACHE_CONNECTION` | str | `None` | Backend connection for inventory cache. |
| `cache_plugin` | `ANSIBLE_INVENTORY_CACHE_PLUGIN` | str | `memory` | Cache plugin used by inventory plugins. |
| `cache_prefix` | `ANSIBLE_INVENTORY_CACHE_PLUGIN_PREFIX` | str | `ansible_inventory_` | Prefix for inventory cache entries. |
| `cache_timeout` | `ANSIBLE_INVENTORY_CACHE_TIMEOUT` | int | `3600` | Inventory cache TTL (seconds). |
| `enable_plugins` | `ANSIBLE_INVENTORY_ENABLED` | list | `[host_list, script, auto, yaml, ini, toml]` | Inventory plugins enabled (in order tried). |
| `export` | `ANSIBLE_INVENTORY_EXPORT` | bool | `False` | Make `ansible-inventory` produce export-friendly output. |
| `host_pattern_mismatch` | `ANSIBLE_HOST_PATTERN_MISMATCH` | str | `warning` | Action on patterns matching no hosts (`warning`/`error`/`ignore`). |
| `ignore_extensions` | `ANSIBLE_INVENTORY_IGNORE` | list | `[.orig, .cfg, .retry, .pyc, .pyo, ...]` | Extensions skipped when reading inventory dirs. |
| `ignore_patterns` | `ANSIBLE_INVENTORY_IGNORE_REGEX` | list | `[]` | Regex patterns skipped when reading inventory dirs. |
| `any_unparsed_is_failed` | `ANSIBLE_INVENTORY_ANY_UNPARSED_IS_FAILED` | bool | `False` | Fail if **any** inventory source can't be parsed. |
| `unparsed_is_failed` | `ANSIBLE_INVENTORY_UNPARSED_FAILED` | bool | `False` | Fail if **all** inventory sources fail to parse. |
| `unparsed_warning` | `ANSIBLE_INVENTORY_UNPARSED_WARNING` | bool | `True` | Warn when inventory ends up empty. |

### 2.3 [privilege_escalation]

| Key | Env Var | Type | Default | Purpose |
|---|---|---|---|---|
| `agnostic_become_prompt` | `ANSIBLE_AGNOSTIC_BECOME_PROMPT` | bool | `True` | Use generic prompt instead of method-specific. |
| `become` | `ANSIBLE_BECOME` | bool | `False` | Globally enable privilege escalation. |
| `become_allow_same_user` | `ANSIBLE_BECOME_ALLOW_SAME_USER` | bool | `False` | Force become even when remote user equals become user. |
| `become_ask_pass` | `ANSIBLE_BECOME_ASK_PASS` | bool | `False` | Prompt for the become password. |
| `become_exe` | `ANSIBLE_BECOME_EXE` | path | `None` | Override become executable (`sudo`, `doas`, ...). |
| `become_flags` | `ANSIBLE_BECOME_FLAGS` | str | `''` | Flags appended to the become command. |
| `become_method` | `ANSIBLE_BECOME_METHOD` | str | `sudo` | Become plugin (`sudo`, `su`, `doas`, `pbrun`, `pfexec`, `runas`, `dzdo`, `ksu`, `machinectl`, `pmrun`, `enable`, `sesu`). |
| `become_user` | `ANSIBLE_BECOME_USER` | str | `root` | Target user for become. |

### 2.4 [ssh_connection] (the `ssh` plugin)

| Key | Env Var | Type | Default | Purpose |
|---|---|---|---|---|
| `control_path` | `ANSIBLE_SSH_CONTROL_PATH` | str | (auto-generated MD5) | Path for SSH `ControlPath` socket. |
| `control_path_dir` | `ANSIBLE_SSH_CONTROL_PATH_DIR` | path | `~/.ansible/cp` | Directory containing `ControlPath` sockets. |
| `host_key_checking` | `ANSIBLE_SSH_HOST_KEY_CHECKING` | bool | `True` | Per-plugin host-key check toggle. |
| `password` | `ANSIBLE_SSH_PASSWORD` | str | `None` | SSH password (use sshpass or ssh-askpass). |
| `password_mechanism` | `ANSIBLE_SSH_PASSWORD_MECHANISM` | str | `ssh_askpass` | How passwords reach ssh (`ssh_askpass`, `sshpass`, `disable`). |
| `pipelining` | `ANSIBLE_SSH_PIPELINING` (or `ANSIBLE_PIPELINING`) | bool | `False` | Pipeline module exec without first transferring (requires no-tty sudo). |
| `pkcs11_provider` | `ANSIBLE_PKCS11_PROVIDER` | str | `''` | Smartcard PKCS#11 library (e.g., `/usr/lib/opensc-pkcs11.so`). |
| `port` | `ANSIBLE_REMOTE_PORT` | int | `22` | Remote SSH port. |
| `private_key` | `ANSIBLE_PRIVATE_KEY` | str | `None` | Inline PEM private key. |
| `private_key_file` | `ANSIBLE_PRIVATE_KEY_FILE` | path | `None` | Path to private key. |
| `private_key_passphrase` | `ANSIBLE_PRIVATE_KEY_PASSPHRASE` | str | `None` | Passphrase for encrypted private key. |
| `reconnection_retries` | `ANSIBLE_SSH_RETRIES` | int | `0` | Times to retry an SSH connection on failure. |
| `remote_user` | `ANSIBLE_REMOTE_USER` | str | (current user) | SSH login user. |
| `scp_executable` | `ANSIBLE_SCP_EXECUTABLE` | path | `scp` | Path to `scp` binary. |
| `scp_extra_args` | `ANSIBLE_SCP_EXTRA_ARGS` | str | `''` | Extra args passed only to `scp`. |
| `sftp_batch_mode` | `ANSIBLE_SFTP_BATCH_MODE` | bool | `True` | Use sftp batch mode for failure detection. |
| `sftp_executable` | `ANSIBLE_SFTP_EXECUTABLE` | path | `sftp` | Path to `sftp` binary. |
| `sftp_extra_args` | `ANSIBLE_SFTP_EXTRA_ARGS` | str | `''` | Extra args passed only to `sftp`. |
| `ssh_args` | `ANSIBLE_SSH_ARGS` | str | `-C -o ControlMaster=auto -o ControlPersist=60s` | Args passed to ALL ssh CLI tools. |
| `ssh_common_args` | `ANSIBLE_SSH_COMMON_ARGS` | str | `''` | Common args appended to ssh/scp/sftp. |
| `ssh_executable` | `ANSIBLE_SSH_EXECUTABLE` | path | `ssh` | Path to `ssh` binary. |
| `ssh_extra_args` | `ANSIBLE_SSH_EXTRA_ARGS` | str | `''` | Extra args passed only to `ssh`. |
| `ssh_transfer_method` | `ANSIBLE_SSH_TRANSFER_METHOD` | str | `smart` | File transfer method (`sftp`, `scp`, `piped`, `smart`). |
| `sshpass_prompt` | `ANSIBLE_SSHPASS_PROMPT` | str | `''` | Custom prompt string for `sshpass`/`SSH_ASKPASS`. |
| `timeout` | `ANSIBLE_SSH_TIMEOUT` | int | `10` | TCP connect timeout in seconds. |
| `use_tty` | `ANSIBLE_SSH_USETTY` | bool | `True` | Add `-tt` to allocate a TTY. |
| `verbosity` | `ANSIBLE_SSH_VERBOSITY` | int | `0` | Underlying ssh verbosity (`-v`, `-vv`, ...). |

### 2.5 [paramiko_connection] (the pure-Python `paramiko_ssh` plugin)

| Key | Env Var | Type | Default | Purpose |
|---|---|---|---|---|
| `banner_timeout` | `ANSIBLE_PARAMIKO_BANNER_TIMEOUT` | float | `30.0` | Seconds to wait for the SSH banner. |
| `host_key_auto_add` | `ANSIBLE_PARAMIKO_HOST_KEY_AUTO_ADD` | bool | `False` | Auto-add unknown host keys. |
| `host_key_checking` | `ANSIBLE_PARAMIKO_HOST_KEY_CHECKING` | bool | `True` | Per-plugin host-key check. |
| `look_for_keys` | `ANSIBLE_PARAMIKO_LOOK_FOR_KEYS` | bool | `True` | Search `~/.ssh/` for private keys. |
| `port` | `ANSIBLE_REMOTE_PARAMIKO_PORT` | int | `22` | Remote port. |
| `private_key_file` | `ANSIBLE_PARAMIKO_PRIVATE_KEY_FILE` | path | `None` | Private key file. |
| `proxy_command` | `ANSIBLE_PARAMIKO_PROXY_COMMAND` | str | `''` | `ProxyCommand`-style jumphost. |
| `pty` | `ANSIBLE_PARAMIKO_PTY` | bool | `True` | Allocate a PTY (often required by sudo). |
| `record_host_keys` | `ANSIBLE_PARAMIKO_RECORD_HOST_KEYS` | bool | `True` | Persist accepted host keys to known_hosts. |
| `remote_user` | `ANSIBLE_PARAMIKO_REMOTE_USER` | str | (current user) | Login user. |
| `timeout` | `ANSIBLE_PARAMIKO_TIMEOUT` | int | `10` | TCP connect timeout. |
| `use_rsa_sha2_algorithms` | `ANSIBLE_PARAMIKO_USE_RSA_SHA2_ALGORITHMS` | bool | `True` | Enable RSA-SHA2 host/pubkey algorithms. |

### 2.6 [persistent_connection] (network device persistence)

| Key | Env Var | Type | Default | Purpose |
|---|---|---|---|---|
| `ansible_connection_path` | `ANSIBLE_CONNECTION_PATH` | path | `None` | (Deprecated) `ansible-connection` script location. |
| `command_timeout` | `ANSIBLE_PERSISTENT_COMMAND_TIMEOUT` | int | `30` | Per-command response timeout. |
| `connect_retry_timeout` | `ANSIBLE_PERSISTENT_CONNECT_RETRY_TIMEOUT` | int | `15` | Retry timeout for the local socket. |
| `connect_timeout` | `ANSIBLE_PERSISTENT_CONNECT_TIMEOUT` | int | `30` | Idle timeout before tearing down the socket. |
| `control_path_dir` | `ANSIBLE_PERSISTENT_CONTROL_PATH_DIR` | path | `~/.ansible/pc` | Directory holding the persistence sockets. |
| `log_messages` | `ANSIBLE_PERSISTENT_LOG_MESSAGES` | bool | `False` | Log all wire messages (very verbose). |

### 2.7 [connection] (modern connection-plugin shared keys)

| Key | Env Var | Type | Default | Purpose |
|---|---|---|---|---|
| `pipelining` | `ANSIBLE_PIPELINING` | bool | `False` | Pipelining toggle, plugin-agnostic. |
| `ssh_agent` | `ANSIBLE_SSH_AGENT` | str | `none` | Manage an SSH agent (`none`/`auto`/<path>). |
| `ssh_agent_executable` | `ANSIBLE_SSH_AGENT_EXECUTABLE` | str | `ssh-agent` | ssh-agent binary path. |
| `ssh_agent_key_lifetime` | `ANSIBLE_SSH_AGENT_KEY_LIFETIME` | int | `None` | Lifetime (seconds) for keys added to the agent. |

### 2.8 [colors]

All keys live in `[colors]` and use the env var pattern `ANSIBLE_COLOR_<KEY>`. Values are color names (`black`, `bright gray`, `blue`, `bright blue`, `green`, `bright green`, `cyan`, `bright cyan`, `red`, `bright red`, `purple`, `bright purple`, `yellow`, `bright yellow`, `white`, `dark gray`, `magenta`, `normal`).

| Key | Default | Purpose |
|---|---|---|
| `changed` | `yellow` | Task `changed` status. |
| `console_prompt` | `white` | `ansible-console` prompt color. |
| `debug` | `dark gray` | Debug messages. |
| `deprecate` | `purple` | Deprecation warnings. |
| `diff_add` | `green` | Added lines in `--diff`. |
| `diff_lines` | `cyan` | Diff metadata/headers. |
| `diff_remove` | `red` | Removed lines in `--diff`. |
| `doc_constant` | `dark gray` | `ansible-doc` constants. |
| `doc_deprecated` | `magenta` | `ansible-doc` deprecated values. |
| `doc_link` | `cyan` | `ansible-doc` hyperlinks. |
| `doc_module` | `yellow` | `ansible-doc` module names. |
| `doc_plugin` | `yellow` | `ansible-doc` plugin names. |
| `doc_reference` | `magenta` | `ansible-doc` cross-refs. |
| `error` | `red` | Errors. |
| `highlight` | `white` | Generic highlight. |
| `included` | `cyan` | "included:" status. |
| `ok` | `green` | Task `ok` status. |
| `skip` | `cyan` | Task `skipping` status. |
| `unreachable` | `bright red` | "unreachable:" status. |
| `verbose` | `blue` | `-v` lines. |
| `warn` | `bright purple` | Warnings. |

### 2.9 [diff]

| Key | Env Var | Type | Default | Purpose |
|---|---|---|---|---|
| `always` | `ANSIBLE_DIFF_ALWAYS` | bool | `False` | Implicit `--diff`. |
| `context` | `ANSIBLE_DIFF_CONTEXT` | int | `3` | Lines of context in unified diff output. |

### 2.10 [selinux]

| Key | Env Var | Type | Default | Purpose |
|---|---|---|---|---|
| `libvirt_lxc_noseclabel` | `ANSIBLE_LIBVIRT_LXC_NOSECLABEL` | bool | `False` | Pass `--noseclabel` to virsh for LXC on non-SELinux hosts (deprecated). |
| `special_context_filesystems` | `ANSIBLE_SELINUX_SPECIAL_FS` | list | `[fuse, nfs, vboxsf, ramfs, 9p, vfat]` | Filesystems exempted from SELinux context errors. |

### 2.11 [galaxy]

| Key | Env Var | Type | Default | Purpose |
|---|---|---|---|---|
| `cache_dir` | `ANSIBLE_GALAXY_CACHE_DIR` | path | `~/.ansible/galaxy_cache` | Galaxy response cache directory. |
| `collection_skeleton` | `ANSIBLE_GALAXY_COLLECTION_SKELETON` | path | `None` | Template dir for `ansible-galaxy collection init`. |
| `collection_skeleton_ignore` | `ANSIBLE_GALAXY_COLLECTION_SKELETON_IGNORE` | list | `[^.git$, ^.*/.git_keep$]` | Patterns ignored when copying skeleton. |
| `collections_path_warning` | `ANSIBLE_GALAXY_COLLECTIONS_PATH_WARNING` | bool | `True` | Warn when `--collections-path` not in `COLLECTIONS_PATHS`. |
| `disable_gpg_verify` | `ANSIBLE_GALAXY_DISABLE_GPG_VERIFY` | bool | `False` | Skip GPG verification on collection install. |
| `display_progress` | `ANSIBLE_GALAXY_DISPLAY_PROGRESS` | bool | `None` (auto) | Show ASCII spinner in galaxy operations. |
| `gpg_keyring` | `ANSIBLE_GALAXY_GPG_KEYRING` | path | `None` | Custom GPG keyring for verification. |
| `ignore_certs` | `ANSIBLE_GALAXY_IGNORE` | bool | `False` | Skip TLS validation against the galaxy server. |
| `ignore_signature_status_codes` | `ANSIBLE_GALAXY_IGNORE_SIGNATURE_STATUS_CODES` | list | `[]` | gpgv status codes to whitelist. |
| `import_poll_factor` | `ANSIBLE_GALAXY_COLLECTION_IMPORT_POLL_FACTOR` | float | `1.5` | Backoff multiplier for collection import polling. |
| `import_poll_interval` | `ANSIBLE_GALAXY_COLLECTION_IMPORT_POLL_INTERVAL` | float | `2.0` | Initial poll interval (seconds). |
| `required_valid_signature_count` | `ANSIBLE_GALAXY_REQUIRED_VALID_SIGNATURE_COUNT` | str | `1` | Minimum valid signatures (`+N` requires N to all sign). |
| `role_skeleton` | `ANSIBLE_GALAXY_ROLE_SKELETON` | path | `None` | Template dir for `ansible-galaxy role init`. |
| `role_skeleton_ignore` | `ANSIBLE_GALAXY_ROLE_SKELETON_IGNORE` | list | `[^.git$, ^.*/.git_keep$]` | Patterns ignored when copying role skeleton. |
| `server` | `ANSIBLE_GALAXY_SERVER` | str | `https://galaxy.ansible.com` | Single-server fallback. |
| `server_list` | `ANSIBLE_GALAXY_SERVER_LIST` | list | `None` | Ordered list of named server stanzas (see below). |
| `server_timeout` | `ANSIBLE_GALAXY_SERVER_TIMEOUT` | int | `60` | Default API call timeout. |
| `token_path` | `ANSIBLE_GALAXY_TOKEN_PATH` | path | `~/.ansible/galaxy_token` | Token cache path. |

When `server_list` is set, the named items map to `[galaxy_server.<name>]` sections that hold per-server `url`, `token`, `username`, `password`, `auth_url`, `client_id`, `validate_certs`, `api_version`, `timeout`, `priority`. Example:

```ini
[galaxy]
server_list = automation_hub, my_org, galaxy

[galaxy_server.automation_hub]
url=https://console.redhat.com/api/automation-hub/content/published/
auth_url=https://sso.redhat.com/auth/realms/redhat-external/protocol/openid-connect/token
token=<offline-token>

[galaxy_server.my_org]
url=https://hub.example.com/api/galaxy/content/inhouse/
token=<token>
validate_certs=true

[galaxy_server.galaxy]
url=https://galaxy.ansible.com/
```

### 2.12 [tags]

| Key | Env Var | Type | Default | Purpose |
|---|---|---|---|---|
| `run` | `ANSIBLE_RUN_TAGS` | list | `[]` | Implicit `--tags`. |
| `skip` | `ANSIBLE_SKIP_TAGS` | list | `[]` | Implicit `--skip-tags` (always wins over `run`). |

### 2.13 [callback_*] (per-callback subsections)

Each callback plugin defines its own `[callback_<name>]` section. Notable ones:

`[callback_default]`

| Key | Env Var | Type | Default | Purpose |
|---|---|---|---|---|
| `check_mode_markers` | `ANSIBLE_CHECK_MODE_MARKERS` | bool | `False` | Print `[CHECK MODE]` headers per task. |
| `display_failed_stderr` | `ANSIBLE_DISPLAY_FAILED_STDERR` | bool | `False` | Send failures to stderr. |
| `display_ok_hosts` | `ANSIBLE_DISPLAY_OK_HOSTS` | bool | `True` | Show `ok:` host lines. |
| `display_skipped_hosts` | `ANSIBLE_DISPLAY_SKIPPED_HOSTS` | bool | `True` | Show `skipping:` lines. |
| `result_format` | `ANSIBLE_CALLBACK_RESULT_FORMAT` | str | `json` | Format for serialized results (`json`/`yaml`). |
| `format_pretty` | `ANSIBLE_CALLBACK_FORMAT_PRETTY` | bool | varies | Pretty-print result format. |
| `result_indentation` | `ANSIBLE_CALLBACK_RESULT_INDENTATION` | int | `4` | Indent depth for result_format. |
| `show_custom_stats` | `ANSIBLE_SHOW_CUSTOM_STATS` | bool | `False` | Show `set_stats` data. |
| `show_per_host_start` | `ANSIBLE_SHOW_PER_HOST_START` | bool | `False` | Print a line at task start per host. |
| `show_task_path_on_failure` | `ANSIBLE_SHOW_TASK_PATH_ON_FAILURE` | bool | `False` | Include file:line in failure output. |

`[callback_log_plays]`

| Key | Env Var | Default | Purpose |
|---|---|---|---|
| `log_folder` | `ANSIBLE_LOG_FOLDER` | `/var/log/ansible/hosts` | Per-host log directory. |

`[callback_profile_tasks]`

| Key | Env Var | Default | Purpose |
|---|---|---|---|
| `task_output_limit` | `ANSIBLE_PROFILE_TASKS_TASK_OUTPUT_LIMIT` | `20` | Show top-N slow tasks. |
| `sort_order` | `ANSIBLE_PROFILE_TASKS_SORT_ORDER` | `descending` | Sort direction. |
| `output_format` | `ANSIBLE_CALLBACK_PROFILE_TASKS_OUTPUT_FORMAT` | `string` | Output style. |

`[callback_junit]`

| Key | Env Var | Default | Purpose |
|---|---|---|---|
| `output_dir` | `JUNIT_OUTPUT_DIR` | `~/.ansible.log` | Where JUnit XML is written. |
| `task_class` | `JUNIT_TASK_CLASS` | `False` | Group test cases by task class. |
| `task_relative_path` | `JUNIT_TASK_RELATIVE_PATH` | `''` | Relative path stripped from test names. |
| `fail_on_change` | `JUNIT_FAIL_ON_CHANGE` | `False` | Treat changed as failure. |
| `fail_on_ignore` | `JUNIT_FAIL_ON_IGNORE` | `False` | Treat ignored as failure. |
| `include_setup_tasks_in_report` | `JUNIT_INCLUDE_SETUP_TASKS_IN_REPORT` | `True` | Include `setup` (gather facts) in report. |
| `hide_task_arguments` | `JUNIT_HIDE_TASK_ARGUMENTS` | `False` | Strip task args from XML. |
| `test_case_prefix` | `JUNIT_TEST_CASE_PREFIX` | `''` | Prefix for test case names. |

Other built-in callbacks (`yaml`, `minimal`, `oneline`, `json`, `null`, `tree`, `unixy`) reuse `[defaults]` keys but may add small per-callback knobs.

### 2.14 Total counts and notes

The set above covers ~250 distinct keys across `[defaults]`, `[inventory]`, `[privilege_escalation]`, `[ssh_connection]`, `[paramiko_connection]`, `[persistent_connection]`, `[connection]`, `[colors]`, `[diff]`, `[selinux]`, `[galaxy]` (+ `[galaxy_server.*]`), `[tags]`, and the various `[callback_*]` stanzas. Additional plugin-defined sections appear as collections install (e.g., `[community.aws]`, `[netconf_connection]`). Use `ansible-config init -t all --disabled > ansible.cfg` to dump every key the running ansible-core knows about, including those installed via collections.

---

## 3. General Precedence

### 3.1 Variable precedence (the canonical 22 levels)

Ansible documents 22 explicit variable scopes. From lowest to highest precedence:

1. Command-line values (e.g., `-u my_user`, but **not** `-e`).
2. Role defaults (`roles/<role>/defaults/main.yml`).
3. Inventory file or script group vars (in the inventory file).
4. Inventory `group_vars/all`.
5. Playbook `group_vars/all`.
6. Inventory `group_vars/*` (specific group dirs).
7. Playbook `group_vars/*`.
8. Inventory file or script host vars.
9. Inventory `host_vars/*`.
10. Playbook `host_vars/*`.
11. Host facts and cached `set_fact` (when `cacheable=true`).
12. Play `vars`.
13. Play `vars_prompt`.
14. Play `vars_files`.
15. Role `vars` (`roles/<role>/vars/main.yml`).
16. Block `vars` (only the tasks inside).
17. Task `vars` (only that task).
18. `include_vars` task.
19. `set_fact` and registered vars (in-memory only).
20. Role and `include_role` parameters.
21. `include_tasks`/`import_tasks` parameters.
22. Extra vars (`-e key=value`, `-e @file.yml`) - **always win**.

Notes that affect implementation:

- Connection variables (`ansible_user`, `ansible_host`, `ansible_port`, `ansible_password`, `ansible_become*`, `ansible_python_interpreter`, etc.) follow the same 22-level rules. If both a CLI flag (`-u`) and an inventory `ansible_user` are present, the inventory value (level 8/9) wins because it sits higher on the ladder than CLI (level 1).
- `set_fact` is level 19 unless `cacheable=true` (then it ALSO populates fact cache at level 11 for future plays).
- Magic variables (those listed in section 4) cannot be overridden by users at all - they sit above the 22-level ladder.
- Within the same level, last-defined wins (alphabetical/load order).
- `hash_behaviour=merge` recursively merges dicts at the same level; the default `replace` overwrites.

### 3.2 Configuration source precedence

For non-variable settings (the keys in section 2):

1. Built-in default (compiled into ansible-core).
2. `ansible.cfg` value (whichever single file was found per section 1).
3. Environment variable (`ANSIBLE_*`).
4. CLI flag (`--forks`, `--timeout`, etc.).
5. Playbook keyword (e.g., `gather_facts: false` overrides `gathering`).
6. Per-task / per-block keyword.
7. Connection variable (highest, applies per host).

**Important asymmetry**: env vars override `ansible.cfg`, but CLI flags do NOT override playbook keywords. If a play sets `gather_facts: false`, passing `--gather-facts` does nothing. CLI flags only override config sources, not playbook source.

### 3.3 Discovery via `ansible-config`

Resolved values per source can be inspected at runtime:

- `ansible-config dump --only-changed` - show only values that differ from defaults (the "what is actually configured" view).
- `ansible-config dump` - full snapshot, with the source annotation `(<source>)` after each value (e.g., `default`, `env: ANSIBLE_FORKS`, `/etc/ansible/ansible.cfg`).
- `ansible-config view` - dump the raw text of the active config file.
- `ansible-config list` - emit the schema with descriptions, types, defaults, and choices in YAML.
- `ansible-config init --disabled` - create a starter file with every key commented out.
- `ansible-config init --disabled -t all` - extend the starter file with keys contributed by all installed plugins/collections.
- `ansible-config validate` - syntactically validate a config file against the live schema.

---

## 4. Special Variables ("magic" vars)

Magic variables are owned by the engine and cannot be set by users. They are exposed in templating and conditional expressions.

### 4.1 Run/play introspection

| Variable | Type | Owner | Purpose |
|---|---|---|---|
| `ansible_check_mode` | bool | engine (per task) | True when `--check` is in effect. |
| `ansible_collection_name` | str | engine | `namespace.collection` of the executing task. |
| `ansible_config_file` | str | engine | Absolute path of the loaded `ansible.cfg`. |
| `ansible_dependent_role_names` | list | engine | Roles pulled in only as dependencies. |
| `ansible_diff_mode` | bool | engine | True when `--diff` is in effect. |
| `ansible_facts` | dict | facts subsystem | Gathered facts (mirrors `setup` output). |
| `ansible_forks` | int | engine | Effective `--forks` for the run. |
| `ansible_index_var` | str | loop subsystem | Name set by `loop_control.index_var`. |
| `ansible_inventory_sources` | list | engine | Resolved inventory source paths. |
| `ansible_limit` | str | engine | Raw value of `--limit`. |
| `ansible_local` | dict | facts subsystem | Custom facts in `/etc/ansible/facts.d`. |
| `ansible_loop` | dict | loop subsystem | Extended loop info when `loop_control.extended=true`. |
| `ansible_loop_var` | str | loop subsystem | Name set by `loop_control.loop_var` (default `item`). |
| `ansible_parent_role_names` | list | engine | All parent roles, most-recent first. |
| `ansible_parent_role_paths` | list | engine | Disk paths matching `ansible_parent_role_names`. |
| `ansible_play_batch` | list | engine | Active hosts in current batch (post-`serial`). |
| `ansible_play_hosts` | list | engine | Hosts in the current play (pre-`serial`). |
| `ansible_play_hosts_all` | list | engine | All hosts targeted by the play (pre-`limit`). |
| `ansible_play_name` | str | engine | Current play's `name`. |
| `ansible_play_role_names` | list | engine | Roles imported into the play (excludes implicit deps). |
| `ansible_playbook_python` | str | engine | Path of the controller Python. |
| `ansible_role_name` | str | engine | Fully qualified role name (`namespace.collection.role`). |
| `ansible_role_names` | list | engine | All roles in the play, including dependencies. |
| `ansible_run_tags` | list | engine | Effective `--tags` (`['all']` if none). |
| `ansible_search_path` | list | engine | Search path for action plugins / lookups. |
| `ansible_skip_tags` | list | engine | Effective `--skip-tags`. |
| `ansible_verbosity` | int | engine | Effective verbosity (0-7). |
| `ansible_version` | dict | engine | `{full, major, minor, revision, string}`. |

### 4.2 Inventory introspection

| Variable | Type | Owner | Purpose |
|---|---|---|---|
| `group_names` | list | inventory | Groups containing the current host (sorted). |
| `groups` | dict | inventory | Map of group name to list of host names. |
| `hostvars` | dict | inventory | Map of host name to host's resolved vars. |
| `inventory_dir` | str | inventory | Directory of the inventory source first defining this host. |
| `inventory_file` | str | inventory | File of the inventory source first defining this host. |
| `inventory_hostname` | str | inventory | Inventory name of the current host (delegation-immune). |
| `inventory_hostname_short` | str | inventory | First DNS label of `inventory_hostname`. |

### 4.3 Playbook keyword surface

| Variable | Type | Owner | Purpose |
|---|---|---|---|
| `playbook_dir` | str | engine | Directory of the playbook being executed. |
| `play_hosts` | list | engine | Deprecated alias for `ansible_play_batch`. |
| `role_name` | str | engine | Bare name of currently executing role. |
| `role_names` | list | engine | Deprecated alias for `ansible_play_role_names`. |
| `role_path` | str | engine | Absolute path of currently executing role. |
| `omit` | sentinel | engine | Special value; `default(omit)` causes the option to be skipped entirely. |

### 4.4 Connection vars (settable; magic only in that engine reads them by name)

| Variable | Type | Purpose |
|---|---|---|
| `ansible_connection` | str | Connection plugin (`ssh`, `local`, `winrm`, `podman`, `kubectl`, ...). |
| `ansible_host` | str | Real network address (overrides `inventory_hostname` for connection). |
| `ansible_port` | int | Remote port. |
| `ansible_user` | str | Login user. |
| `ansible_password` | str | Login password. |
| `ansible_ssh_pass` | str | Legacy alias for `ansible_password`. |
| `ansible_ssh_private_key_file` | path | Per-host private key. |
| `ansible_ssh_common_args` | str | Per-host extra ssh/scp/sftp args. |
| `ansible_ssh_extra_args` | str | Per-host ssh-only args. |
| `ansible_sftp_extra_args` | str | Per-host sftp-only args. |
| `ansible_scp_extra_args` | str | Per-host scp-only args. |
| `ansible_ssh_pipelining` | bool | Per-host pipelining override. |
| `ansible_become` | bool | Per-host become toggle. |
| `ansible_become_method` | str | Per-host become method. |
| `ansible_become_user` | str | Per-host become target user. |
| `ansible_become_password` | str | Per-host become password. |
| `ansible_become_pass` | str | Legacy alias for `ansible_become_password`. |
| `ansible_become_exe` | path | Per-host become executable. |
| `ansible_become_flags` | str | Per-host become flags. |
| `ansible_shell_type` | str | Remote shell family (`sh`, `csh`, `fish`, `powershell`). |
| `ansible_shell_executable` | path | Remote shell binary. |
| `ansible_python_interpreter` | path/str | Python interpreter on remote (`auto`, `auto_silent`, or a path). |
| `ansible_*_interpreter` | path | Per-language interpreter override (`ansible_perl_interpreter`, `ansible_ruby_interpreter`, ...). |

---

## 5. Common Return Values

Standard keys that any module is permitted (and often expected) to return. Implementations can extend with module-specific keys, but these are universal.

### 5.1 Always available

| Key | Type | Purpose |
|---|---|---|
| `changed` | bool | Did the module mutate anything? Drives handler notification. |
| `failed` | bool | True when the module failed (also implied by raising). |
| `failed_when_result` | bool | True when `failed_when:` evaluated true post-run. |
| `skipped` | bool | True when `when:`/`tags:` skipped the task. |
| `msg` | str | Human-readable status; almost always present on failure. |
| `invocation` | dict | Echo of how the module was called (`module_args`, etc.). Sanitized for `no_log`. |

### 5.2 Command-style modules (`command`, `shell`, `raw`, `script`)

| Key | Type | Purpose |
|---|---|---|
| `rc` | int | Process exit code. |
| `stdout` | str | Captured stdout. |
| `stdout_lines` | list[str] | `stdout.splitlines()`. |
| `stderr` | str | Captured stderr. |
| `stderr_lines` | list[str] | `stderr.splitlines()`. |
| `cmd` | list/str | The actual command line that ran. |
| `start` / `end` / `delta` | str | ISO timestamps and `HH:MM:SS.uuuuuu` duration. |

### 5.3 File-mutating modules (`copy`, `template`, `lineinfile`, `replace`, ...)

| Key | Type | Purpose |
|---|---|---|
| `backup_file` | str | Path to backup created when `backup=true`. |
| `dest` | path | Final path written. |
| `src` | path | Source path. |
| `mode` | str | Final mode (`"0644"`). |
| `owner` / `group` / `uid` / `gid` | str/int | Final ownership. |
| `size` | int | Final byte size. |
| `state` | str | Resulting state (`file`, `directory`, `absent`). |
| `checksum` / `md5sum` | str | Content hash. |

### 5.4 Loop and check-mode shapes

| Key | Type | Purpose |
|---|---|---|
| `results` | list[dict] | Per-iteration result dicts when a loop is present. |
| `diff` | dict | `{before, after, before_header, after_header}` or list of those. Surfaced by `--diff`. |
| `_ansible_no_log` | bool | Internal: signals that the result must be redacted. |

### 5.5 Internal/special keys (stripped from registered vars)

| Key | Type | Purpose |
|---|---|---|
| `ansible_facts` | dict | Promoted into host facts; not retained on the result dict. |
| `ansible_stats` | dict | `{data, per_host, aggregate}`; consumed by `set_stats`. |
| `exception` | str | Python traceback; only shown at `-vvv` or higher. |
| `warnings` | list[str] | Non-fatal warnings to surface. |
| `deprecations` | list[dict] | `[{msg, version, collection_name}]` items. |

---

## 6. YAML Syntax Notes

Ansible's YAML is YAML 1.1 (via PyYAML) with a few engine-imposed conventions.

### 6.1 Boolean coercion

The full list of strings that PyYAML 1.1 accepts as booleans (and Ansible inherits): `y`, `Y`, `yes`, `Yes`, `YES`, `n`, `N`, `no`, `No`, `NO`, `true`, `True`, `TRUE`, `false`, `False`, `FALSE`, `on`, `On`, `ON`, `off`, `Off`, `OFF`. This is the source of the famous "Norway problem" (`country: NO` becomes `country: false`). Workaround: quote any literal yes/no/on/off string. Ansible recommends lowercase `true`/`false` in dict values to satisfy yamllint defaults.

### 6.2 Numerics, octals, and version strings

- Plain integers parse as ints; `0644` parses as **decimal 644** in YAML 1.2 but as **octal 420** in YAML 1.1, depending on loader. Ansible's recommendation: ALWAYS quote file modes (`mode: '0644'`) to bypass ambiguity. The `file` family of modules accepts symbolic notation (`u=rw,g=r,o=r`) too.
- Float-like strings like `1.0`, `1.10` are coerced to floats and lose trailing zeros. Quote them when you want them as strings (`version: "1.10"`).
- Numeric-looking strings beginning with `0` (`0123`) are treated as octals or strings depending on loader. Quote.

### 6.3 Strings with structural characters

- Values containing `:` followed by a space need quoting (`'a: b'` or `"a: b"`).
- Values starting with `{` or `[` look like inline collections; quote (`"{{ var }}"` is the canonical case).
- Values starting with `*`, `&`, `!`, `|`, `>`, `?`, `%`, `@`, `` ` ``, `,`, `#` (with leading space), or reserved chars need quoting.
- Values containing unescaped `#` after whitespace start a comment.

### 6.4 Multiline strings

Block scalar indicators:

- `|` literal block - preserves newlines.
- `>` folded block - newlines become spaces, blank lines preserved.
- `|-` or `>-` - chomp trailing newline.
- `|+` or `>+` - keep all trailing newlines.
- `|2`, `>4`, ... - explicit indentation indicator (rarely needed).

### 6.5 Anchors and aliases

YAML anchors (`&name`) and aliases (`*name`) work, plus the merge key (`<<: *base`). Ansible's templating evaluates AFTER YAML parsing, so anchors copy the literal value; if the anchored value contains `{{ }}`, every consumer renders independently. Tagged types (`!!str`, `!!int`, etc.) work; custom Ansible tags include `!unsafe` (template once and treat output as raw text - prevents recursive templating, important for vault-stored content) and `!vault` (block-scalar payload is decrypted at use).

### 6.6 Dates

YAML 1.1 auto-detects ISO 8601 dates and timestamps; `date: 2024-01-01` becomes a Python `datetime.date`. To keep as string, quote it. Vault and module args containing dates almost always need quoting.

### 6.7 Documents and explicit markers

- `---` opens a document (optional but recommended at file top).
- `...` closes a document (rarely used).
- A single file may contain multiple documents separated by `---`; Ansible reads only the first for most loaders.

### 6.8 Encoding

UTF-8 is required. BOM is permitted but discouraged. Ansible defaults to UTF-8 for module response (`module_strict_utf8_response=true`); modules emitting non-UTF-8 fail unless that key is flipped.

---

## 7. Interpreter Discovery

### 7.1 The four modes

`ansible_python_interpreter` (per host/group) or `interpreter_python` in `[defaults]` (global) accepts:

| Mode | Behavior |
|---|---|
| `auto` (default) | Run the discovery probe; if a known distro maps to a "preferred" Python and that interpreter exists, use it. Otherwise iterate `interpreter_python_fallback`. Issues a warning the FIRST time the probe runs against a host so users know Ansible picked a Python. |
| `auto_silent` | Same probe as `auto`; suppresses the warning entirely. |
| `auto_legacy` | Deprecated alias for `auto`. Historically allowed `/usr/bin/python` (Python 2) as a final fallback; that fallback is gone now. |
| `auto_legacy_silent` | Deprecated alias for `auto_silent`. |
| `<absolute path>` | Use that path verbatim. Fails fast if missing. No probe. |

### 7.2 The discovery probe

The probe is an in-band shell command (`/bin/sh`-compatible) that reads `/etc/os-release` plus `uname -m` and returns OS family + version. Ansible then consults its built-in distro map (the `INTERPRETER_PYTHON_DISTRO_MAP` constant). Each distro maps to a list of candidate Python versions in priority order. Examples (current values, subject to per-release tweaks):

| Distro family | Default candidates |
|---|---|
| RHEL/CentOS/Rocky/Alma 9, 10 | `/usr/bin/python3.12`, `/usr/bin/python3.11`, `/usr/bin/python3.9`, `/usr/libexec/platform-python`, `/usr/bin/python3` |
| RHEL/CentOS 8 | `/usr/libexec/platform-python`, `/usr/bin/python3.6`, `/usr/bin/python3` |
| Fedora 39+ | `/usr/bin/python3.12`, `/usr/bin/python3.11`, `/usr/bin/python3` |
| Ubuntu 24.04 | `/usr/bin/python3.12`, `/usr/bin/python3` |
| Ubuntu 22.04 | `/usr/bin/python3.10`, `/usr/bin/python3` |
| Ubuntu 20.04 | `/usr/bin/python3.8`, `/usr/bin/python3` |
| Debian 12 | `/usr/bin/python3.11`, `/usr/bin/python3` |
| Debian 11 | `/usr/bin/python3.9`, `/usr/bin/python3` |
| Generic / unmatched | `interpreter_python_fallback` list (see below) |

### 7.3 The fallback list (`interpreter_python_fallback`)

Used when the distro map lookup fails or yields no installed candidate. Default in ansible-core 2.20:

```
python3.14, python3.13, python3.12, python3.11, python3.10, python3.9, python3.8, /usr/bin/python3, python3
```

Each entry is checked via `command -v` on the remote.

### 7.4 Selection algorithm

1. If `ansible_python_interpreter` is an absolute path, use it. Done.
2. If the value is `auto`/`auto_silent`/`auto_legacy`/`auto_legacy_silent`:
   a. SSH a tiny probe script that emits `{"platform_dist_result": [...], "preferred_interpreter_list": [...]}`.
   b. Walk `preferred_interpreter_list` from the probe (which Ansible derived from `INTERPRETER_PYTHON_DISTRO_MAP`).
   c. For each candidate, check if it exists and is executable on the remote.
   d. First hit wins; record on the host's discovered facts so future tasks reuse it without reprobing.
   e. If none hit, walk `interpreter_python_fallback`.
   f. If everything fails, raise `INTERPRETER_PYTHON_FALLBACK exhausted` and abort the host.
3. If `auto` (not `auto_silent`), emit a one-time warning per host showing the chosen interpreter and the fact that the choice may change in future ansible-core releases.

### 7.5 Per-host overrides

```ini
# inventory.ini
[web]
web1 ansible_python_interpreter=/usr/bin/python3.11
web2 ansible_python_interpreter=auto_silent
```

```yaml
# group_vars/db.yml
ansible_python_interpreter: /opt/python3.12/bin/python
```

### 7.6 Non-Python interpreters

`ansible_*_interpreter` follows the same idea for scripted modules in other languages: `ansible_perl_interpreter`, `ansible_ruby_interpreter`. No discovery probe; you provide a path or the module fails.

---

## 8. ansible-config CLI

`ansible-config` exposes the configuration system from the command line.

### 8.1 Synopsis

```
ansible-config [-h] [--version] [-v] {list, dump, view, init, validate} [...]
```

### 8.2 Global flags

| Flag | Purpose |
|---|---|
| `-h`, `--help` | Show help. |
| `--version` | Print version, config file location, configured module path, executable location, and Python version. |
| `-v`/`-vv`/.../`-vvvvvv` | Increase verbosity. |
| `-c CONFIG_FILE`, `--config CONFIG_FILE` | Override which `ansible.cfg` to read. |
| `-t TYPE`, `--type TYPE` | Restrict to a plugin type (`all`, `base`, `become`, `cache`, `callback`, `cliconf`, `connection`, `httpapi`, `inventory`, `lookup`, `netconf`, `shell`, `vars`, `module`, `strategy`, `terminal`, `test`, `filter`, `keyword`). |

### 8.3 Subcommands

`list` - Enumerate config keys.

```
ansible-config list [-c CONFIG] [-t TYPE] [--format {yaml,json,ini,env,vars}]
```

Outputs the schema with descriptions, defaults, choices, env vars, INI section, and version added. Default format is `yaml`. `--format env` is useful for emitting an `export ANSIBLE_*` script.

`dump` - Show the current resolved values.

```
ansible-config dump [-c CONFIG] [-t TYPE] [--only-changed/--changed-only] [--format {yaml,json,ini,env,vars}]
```

Each value is annotated with its source: `(default)`, `(env: ANSIBLE_FOO)`, `(/path/to/ansible.cfg)`, or a per-plugin source. `--only-changed` (also accepts `--changed-only`) hides anything still at default.

`view` - Cat the active config file (no resolution).

```
ansible-config view [-c CONFIG] [-t TYPE]
```

Uses the configured `pager`.

`init` - Generate a sample config.

```
ansible-config init [--disabled] [-c CONFIG] [-t TYPE] [--format {ini,env,vars}]
```

`--disabled` comments out every line so the file is inert until you uncomment specific keys. Combine with `-t all` to include keys provided by every installed plugin and collection.

`validate` - Lint a config file.

```
ansible-config validate [-c CONFIG] [-t TYPE] [--format {...}]
```

Flags unknown keys, type mismatches, deprecated values, and choice violations.

---

## 9. ansible-doc CLI

`ansible-doc` is the in-tree help tool for plugins, modules, roles, and keywords.

### 9.1 Synopsis

```
ansible-doc [-h] [--version] [-v] [-M MODULE_PATH] [-r ROLES_PATH]
            [--playbook-dir BASEDIR] [-t {plugin_type}]
            [-l | -F | -s | --metadata-dump] [-j] [-e ENTRY_POINT]
            [plugin [plugin ...]]
```

### 9.2 Plugin types `-t TYPE` accepts

`become`, `cache`, `callback`, `cliconf`, `connection`, `httpapi`, `inventory`, `lookup`, `netconf`, `shell`, `vars`, `module` (default), `strategy`, `test`, `filter`, `role`, `keyword`.

### 9.3 Output modes

| Flag | Purpose |
|---|---|
| (none) | Render full documentation for the named plugin(s). |
| `-l`, `--list` | List available plugins of `-t TYPE`. Accepts a namespace/collection prefix to filter. |
| `-F`, `--list_files` | Like `--list` but also shows the source file. |
| `-s`, `--snippet` | Emit a playbook snippet skeleton (works for module, inventory, lookup). |
| `-j`, `--json` | Emit machine-readable JSON. Combinable with `--list`, `--snippet`, plain doc. |
| `--metadata-dump` | Internal use; emits the entire collection metadata graph. |
| `--no-fail-on-errors` | Tolerate plugin import failures during `--metadata-dump`. |

### 9.4 Locating plugins

| Flag | Purpose |
|---|---|
| `-M`, `--module-path PATH` | Prepend module directories. |
| `-r`, `--roles-path PATH` | Override `roles_path`. |
| `--playbook-dir DIR` | Anchor relative paths and Galaxy/collection resolution to this dir. |
| `-e ENTRY_POINT` | When `-t role`, render the named entry point (default `main`). |

### 9.5 Searching

- `ansible-doc -l community.aws` filters by collection.
- `ansible-doc -l -t lookup` lists every lookup plugin available.
- `ansible-doc community.crypto.openssl_privatekey` shows full docs for a module.
- `ansible-doc -t keyword loop` shows the docs for a playbook keyword.
- `ansible-doc -j -t module ansible.builtin.copy` returns the doc dict as JSON, suitable for piping into a generator.

---

## 10. Quirks and Implementation Hazards

A condensed list of behaviors that bite users and that runsible should choose to either preserve, fix, or document.

### 10.1 Env vars override `ansible.cfg`, not the other way around

Setting `ANSIBLE_FORKS=10` in your shell silently beats `forks=20` in `ansible.cfg`. There is no way to make the cfg "win" short of unsetting the env var. This is intentional and matches Twelve-Factor sensibilities, but it surprises users who think config files are authoritative.

### 10.2 `ANSIBLE_*` prefix conventions

- Almost every key has an env var named `ANSIBLE_<UPPER_KEY>` where dots/dashes become underscores.
- A few keys break the pattern (`ANSIBLE_INVENTORY` for `inventory`, `ANSIBLE_LIBRARY` for `library`, `ANSIBLE_NOCOLOR` for `nocolor`). They predate the convention.
- Some env vars have aliases (`ANSIBLE_NOCOLOR` and `NO_COLOR`; `ANSIBLE_PAGER` and `PAGER`; `ANSIBLE_EDITOR` and `EDITOR`).
- Plugin-specific keys gain the prefix `ANSIBLE_<PLUGIN>_<KEY>` (`ANSIBLE_PARAMIKO_BANNER_TIMEOUT`).
- Some env-var-only keys exist with no corresponding INI section (rare, mostly callbacks).

### 10.3 First-match-wins file lookup

Section 1 already covered this. The lack of layering means orgs cannot ship a global baseline and let teams override - they have to copy the entire file.

### 10.4 World-writable CWD silently disables `./ansible.cfg`

The check is meant to prevent privilege escalation when `cd`ing into someone else's directory; in containers and CI runners this often fires unexpectedly because `/workspace` is mounted with broad perms. Fix: `chmod o-w .` or set `ANSIBLE_CONFIG` explicitly.

### 10.5 Keys that have moved or been renamed

- `sudo`, `sudo_user`, `sudo_exe`, `sudo_flags`, `sudo_pass`, `ask_sudo_pass` → `become*` family (since Ansible 2.0; old names accepted with a deprecation warning).
- `accelerate*` → fully removed (Accelerate transport killed in 2.4).
- `default_*` historic constant prefix - still present internally (e.g., `DEFAULT_BECOME`) but the INI key is the unprefixed name (`become`). Source attributions in `ansible-config dump` use the prefixed form.
- `inventory_ignore_extensions`/`inventory_ignore_patterns` accept both `[defaults]` and `[inventory]` sections during the transition; `[inventory]` is preferred.
- `cache_plugin*` keys mirror `fact_caching*` keys (older alias kept indefinitely).
- `null_representation`, `error_on_undefined_vars`, `jinja2_extensions`, `jinja2_native`, `ansible_managed`, `ansible_connection_path`, `libvirt_lxc_noseclabel` are all flagged deprecated.
- Smart Inventory plugins replaced legacy `inventory` script logic (2.4+); script support is preserved but secondary to the YAML/INI/auto plugins.

### 10.6 Settings that look the same but are not

- `pipelining` exists in `[connection]`, `[ssh_connection]`, `[paramiko_connection]`, and `[winrm_connection]`. The most-specific section wins for that connection plugin.
- `host_key_checking` exists in `[defaults]` (engine-wide), `[ssh_connection]`, and `[paramiko_connection]`. Plugin section overrides the global.
- `port` exists implicitly per plugin. `ANSIBLE_REMOTE_PORT` is the most-shared but `ANSIBLE_REMOTE_PARAMIKO_PORT` exists as paramiko's parallel.

### 10.7 Boolean trap for typed values

YAML's lax boolean coercion (section 6.1) means inventory values like `ansible_become: no` evaluate to false even when written for clarity. Ansible's recommendation: always use `true`/`false` and keep yamllint strict.

### 10.8 `gather_facts` vs `gathering`

`gathering: smart` (the recommended global) only re-gathers facts when the host's facts aren't already cached. `gather_facts: false` on a play disables gathering entirely regardless. CLI flags can't override either.

### 10.9 `-e` always wins, but `set_fact` is below `-e`

This sometimes surprises Terraform/Salt converts: registering a fact does not override an extra var. If you need a runtime override, you have to use `--extra-vars` or write a vars plugin.

### 10.10 `omit` is a string sentinel, not Python None

`omit` only works in module argument templating (`mode: "{{ desired_mode | default(omit) }}"`). It cannot be used in conditionals or shell commands. Misuse silently produces the literal string `__omit_place_holder__`.

### 10.11 `ansible-config list -t all` reaches into collections

Running it before installing collections will list only ansible-core's keys (~250). After installing the community package, the list balloons (3000+ keys is normal). runsible's equivalent should make this scaling explicit.

### 10.12 Python deprecation on the controller

ansible-core 2.20 requires Python 3.11+ on the controller; managed nodes still accept 3.8+. There is no Python 2 support of any kind. Watch for `interpreter_python_fallback` evolution; the 2.20 list drops 3.7 entirely.

### 10.13 Release schedule shapes the config surface

ansible-core ships every ~6 months (May / Nov). Three majors are always supported (GA / Critical / Security phases). The community ansible package ships ~every 4 weeks but only ONE major is supported at a time. This is why config keys can be added or marked deprecated frequently; runsible should pin a single ansible-core release as its semantic baseline (2.20 is the latest GA as of Nov 2025).

### 10.14 Automation Hub vs Galaxy

Automation Hub is the Red Hat-supported downstream of Galaxy. Configuration is via `[galaxy] server_list` (section 2.11). Required keys: `url`, plus either `token` (offline token) or username/password OAuth pairs. There is no auto-discovery; the `[galaxy_server.<name>]` stanzas are mandatory.

### 10.15 Test-strategy keys

There is no "test mode" config section. Testing leans on `--check` (driven by `ansible_check_mode`), `--diff`, the `assert` module, and the `wait_for` module. The `[diff]` section governs only output formatting, not test behavior. `task_timeout` (`[defaults]`) is the only built-in wall-clock guard, and it is per task, not per play.

### 10.16 ansible-doc is the schema source of truth

The `--metadata-dump` and `--json` output of `ansible-doc` is the canonical schema feed for IDE tooling (Red Hat's vscode-ansible extension reads it). runsible's analogue should expose an equivalent JSON schema dump from day one.

---

## Appendix A - Glossary essentials

Terse definitions of terms that appear throughout the config surface:

- **action**: Specifies which module to run and its arguments.
- **ad hoc**: Single command via `ansible` rather than a playbook.
- **async**: Background-executed task; status polled later.
- **callback plugin**: Hook invoked at lifecycle events; owns stdout when `stdout_callback` matches.
- **check mode**: `--check` dry-run that previews changes without applying.
- **collection**: Distribution unit containing plugins, modules, roles, playbooks.
- **connection plugin**: Library that talks to the managed node (ssh, winrm, podman, ...).
- **control node**: Machine running Ansible.
- **fact**: Auto-discovered remote attribute, owned by `setup` module.
- **filter plugin**: Custom Jinja2 filter.
- **forks**: Maximum simultaneous host connections.
- **gather facts**: Whether to run `setup` at play start.
- **handler**: Task that runs only when notified by a `changed` task.
- **host**: Managed node entry in inventory.
- **inventory**: Source describing hosts and groups.
- **lookup plugin**: Pulls data from external sources during templating.
- **managed node**: Remote machine being configured.
- **module**: Code shipped to the managed node and executed there.
- **play**: Mapping of host pattern to task list.
- **playbook**: One or more plays in YAML.
- **role**: Reusable bundle of tasks/handlers/vars/files/templates.
- **strategy**: Plugin governing per-play execution order (`linear`, `free`, `host_pinned`, `debug`).
- **tags**: Selective execution markers.
- **task**: Single action with a name and metadata.
- **vars**: Templating values.
- **vault**: Encrypted YAML payload, decrypted on use.

---

## Appendix B - Release calendar (for runsible's compatibility window)

| ansible-core | GA | Critical phase | Security phase | EOL |
|---|---|---|---|---|
| 2.20 | 03 Nov 2025 | 18 May 2026 | 02 Nov 2026 | May 2027 |
| 2.19 | 21 Jul 2025 | 03 Nov 2025 | 18 May 2026 | Nov 2026 |
| 2.18 | 04 Nov 2024 | 19 May 2025 | 03 Nov 2025 | May 2026 |

Community Ansible package: 13.x current (depends on core 2.20); 12.x and 11.x EOL Dec 2025; older versions unmaintained.

Deprecation cycle: ansible-core deprecations remove after ~4 feature releases (~2 years). Collection deprecations: at least 1 year or until the next major Ansible community package.

---

## Appendix C - Implementation checklist for runsible-config

(Notes for the runsible-config crate; not part of the upstream Ansible reference but useful for the immediate task.)

- Treat the 250 INI keys above as the **input domain**. Map each to an explicit TOML path under one of: `defaults`, `inventory`, `become`, `ssh`, `paramiko`, `persistent`, `connection`, `colors`, `diff`, `selinux`, `galaxy`, `tags`, `callbacks.<name>`.
- Keep env-var precedence over file precedence as Ansible does, but also support layered config files (Cargo-style) - this is the single most-requested missing feature.
- Represent the 22-level variable precedence as a strict ordering enum so that the `vars` resolver can be unit-tested deterministically.
- Encode `interpreter_python_fallback` and `INTERPRETER_PYTHON_DISTRO_MAP` as TOML data so they are user-overridable.
- Provide an `ansible-config validate`-equivalent (`runsible config check`) from day one; do not allow unknown keys to silently survive.
- Expose `runsible config dump --json` whose schema matches `ansible-config dump --format json` so existing IDE tooling can dual-target.
- For each key, store: name, section, env_var, type, default, choices, deprecated_in, removed_in, description, since. This metadata feeds both validation and documentation.
- Match Ansible's `--only-changed` semantics in `runsible config dump` so users can audit deltas.
- Re-implement the world-writable check, but make it a hard error with a clear message rather than a silent skip.
- Accept Ansible-style env var names (`ANSIBLE_FORKS`) as aliases for `RUNSIBLE_FORKS` to ease migration.
