# runsible

Rust reimplementation of Ansible's CLI surface. TOML playbooks, zero Python dependency, single binary.

## What It Is

runsible is a 1-for-1 reimagining of Ansible's toolchain in Rust and TOML. It ships every Ansible subcommand — `runsible-playbook`, `runsible-inventory`, `runsible-vault`, `runsible-lint`, `runsible-test`, and more — as a single compiled binary with no runtime dependencies.

Where Ansible requires Python, a virtualenv, and `pip install ansible`, runsible requires nothing but the binary.

## Workspace Layout

| Crate | Purpose |
|-------|---------|
| `runsible` | Main CLI entrypoint |
| `runsible-core` | Shared types, task engine, connection |
| `runsible-playbook` | Playbook execution |
| `runsible-inventory` | Host/group inventory |
| `runsible-vault` | Secrets encryption |
| `runsible-config` | Configuration management |
| `runsible-lint` | Playbook linting |
| `runsible-test` | Test harness |
| `runsible-galaxy` | Collections/roles |
| `runsible-doc` | Documentation generation |
| `runsible-console` | Interactive console |
| `runsible-pull` | Pull mode |
| `yaml2toml` | YAML → TOML playbook converter |
<!-- COCHRANBLOCK-BRAND-FOOTER:START -->

---

<sub>&#9656; **THE COCHRAN BLOCK, LLC** &#183; CAGE `1CQ66` &#183; UEI `W7X3HAQL9CF9` &#183; UNLICENSE &#183; [cochranblock.org](https://cochranblock.org)</sub>
<!-- COCHRANBLOCK-BRAND-FOOTER:END -->
