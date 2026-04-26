# runsible

A 1-for-1 Rust reimagining of Ansible. TOML-native, Unlicense, with an automatic YAML to TOML converter so existing Ansible playbooks port cleanly.

## Status

Pre-alpha. Scaffolding only. Not usable yet.

## Binary parity goal

Each Ansible CLI maps to a runsible counterpart:

| Ansible              | runsible              |
|----------------------|-----------------------|
| `ansible`            | `runsible`            |
| `ansible-playbook`   | `runsible-playbook`   |
| `ansible-galaxy`     | `runsible-galaxy`     |
| `ansible-vault`      | `runsible-vault`      |
| `ansible-inventory`  | `runsible-inventory`  |
| `ansible-doc`        | `runsible-doc`        |
| `ansible-config`     | `runsible-config`     |
| `ansible-console`    | `runsible-console`    |
| `ansible-pull`       | `runsible-pull`       |
| `ansible-lint`       | `runsible-lint`       |
| `ansible-test`       | `runsible-test`       |
| `ansible-connection` | `runsible-connection` |

Plus `yaml2toml` for converting existing Ansible YAML to runsible TOML.

## Why TOML

- One canonical syntax. No tabs/spaces ambiguity, no anchor/alias footguns, no implicit type coercion that turns `no` into `false` or `01:30` into seconds.
- Native to the Rust ecosystem (Cargo, rustfmt, every config in sight).
- Round-trips cleanly through serde without losing comments via `toml_edit`.
- Diffs are stable.

YAML stays supported for migration via `yaml2toml`. Once converted, TOML is the source of truth.

## Prior art

This project is not the first to consider a Rust take on Ansible, nor the first to claim the name. Acknowledged prior and parallel work:

- **Ansible** (Red Hat / IBM, 2012). The original. runsible is a reimagining, not a fork; no code is taken from upstream Ansible. The CLI surface is intentionally mirrored for muscle-memory parity.
- **kernelfirma/rustible** (MIT, 2025). Active Rust config-management engine that keeps Ansible's YAML playbook syntax. Different goals: rustible targets YAML compatibility; runsible targets TOML-native with conversion.
- **SickMcNugget/rustible** (2025, inactive). Early Rust automation experiment.
- **bcoca/rustible_utilities** (2025, inactive). Utility/shared-code experiments by an Ansible core maintainer.
- **rickhull/runsible** (Ruby, 2015, abandoned). SSH + YAML command runner. Unrelated codebase.
- **KONOVALOVda/RUnsible** (2025, inactive). Russian "Ansible analog from scratch."
- **juliadin/runsible** (2024, inactive, empty).

If your project is missing from this list and should be credited, open an issue.

## Why runsible is different

- **TOML, not YAML.** First-class. The data model is TOML; YAML is an import format.
- **Bundled converter.** `yaml2toml` ships in-tree and round-trips real Ansible playbooks, roles, inventories, and group_vars/host_vars.
- **Unlicense.** Public domain dedication. No attribution clause, no copyleft, no patent grant gymnastics. Take it.
- **One binary per Ansible tool.** Drop-in CLI parity, not a single mega-binary.
- **Rust.** Compiled, statically linked, no Python runtime on the controller.
- **No Jinja runtime.** Templating is a Rust crate (Tera or MiniJinja) chosen at compile time, not embedded Python.

## License

Unlicense. See `UNLICENSE`.
