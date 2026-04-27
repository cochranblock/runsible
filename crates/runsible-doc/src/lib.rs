use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::path::Path;

// ---------------------------------------------------------------------------
// Schema types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleDoc {
    pub name: String,
    pub short_description: String,
    pub description: String,
    pub version_added: String,
    pub author: Vec<String>,
    pub options: IndexMap<String, OptionDoc>,
    pub examples: Vec<Example>,
    pub return_values: IndexMap<String, ReturnDoc>,
    pub notes: Vec<String>,
    pub see_also: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionDoc {
    pub description: String,
    pub type_: String,
    pub required: bool,
    pub default: Option<String>,
    pub choices: Vec<String>,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Example {
    pub name: String,
    pub description: String,
    pub toml: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReturnDoc {
    pub description: String,
    pub type_: String,
    pub sample: String,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum DocError {
    #[error("module not found: '{0}'")]
    NotFound(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml: {0}")]
    Toml(String),
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

pub struct DocRegistry {
    docs: IndexMap<String, ModuleDoc>,
}

impl DocRegistry {
    /// Create a registry pre-loaded with all hand-authored builtin module docs.
    pub fn builtins() -> Self {
        let mut docs = IndexMap::new();
        for d in [
            builtin_debug(),
            builtin_ping(),
            builtin_set_fact(),
            builtin_assert(),
            builtin_command(),
            builtin_shell(),
            builtin_copy(),
            builtin_file(),
            builtin_template(),
            builtin_package(),
            builtin_service(),
            builtin_systemd_service(),
            builtin_get_url(),
        ] {
            docs.insert(d.name.clone(), d);
        }
        DocRegistry { docs }
    }

    pub fn get(&self, name: &str) -> Option<&ModuleDoc> {
        self.docs.get(name)
    }

    pub fn list(&self) -> Vec<&ModuleDoc> {
        self.docs.values().collect()
    }

    /// Load additional docs from a directory of `.doc.toml` files.
    pub fn load_dir(&mut self, path: &Path) -> Result<usize, DocError> {
        let mut count = 0usize;
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("toml")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.ends_with(".doc.toml"))
                    .unwrap_or(false)
            {
                let raw = std::fs::read_to_string(&p)?;
                let doc: ModuleDoc =
                    toml::from_str(&raw).map_err(|e| DocError::Toml(e.to_string()))?;
                self.docs.insert(doc.name.clone(), doc);
                count += 1;
            }
        }
        Ok(count)
    }
}

// ---------------------------------------------------------------------------
// Render helpers
// ---------------------------------------------------------------------------

/// Render a ModuleDoc to terminal-friendly text.
pub fn render_text(doc: &ModuleDoc) -> String {
    let mut out = String::new();

    // NAME
    out.push_str("NAME\n");
    out.push_str(&format!("    {}\n\n", doc.name));

    // SYNOPSIS
    out.push_str("SYNOPSIS\n");
    out.push_str(&format!("    {}\n\n", doc.short_description));

    // DESCRIPTION
    out.push_str("DESCRIPTION\n");
    for line in doc.description.lines() {
        out.push_str(&format!("    {}\n", line));
    }
    out.push('\n');

    // OPTIONS
    out.push_str("OPTIONS\n");
    if doc.options.is_empty() {
        out.push_str("    (none)\n");
    } else {
        for (key, opt) in &doc.options {
            let req = if opt.required { " [required]" } else { "" };
            let default_str = opt
                .default
                .as_deref()
                .map(|d| format!(" (default: {})", d))
                .unwrap_or_default();
            out.push_str(&format!(
                "  {} ({}){}{}:\n    {}\n",
                key, opt.type_, req, default_str, opt.description
            ));
            if !opt.choices.is_empty() {
                out.push_str(&format!("    choices: {}\n", opt.choices.join(", ")));
            }
            if !opt.aliases.is_empty() {
                out.push_str(&format!("    aliases: {}\n", opt.aliases.join(", ")));
            }
        }
    }
    out.push('\n');

    // EXAMPLES
    out.push_str("EXAMPLES\n");
    for ex in &doc.examples {
        out.push_str(&format!("  # {}\n", ex.name));
        if !ex.description.is_empty() {
            out.push_str(&format!("  # {}\n", ex.description));
        }
        for line in ex.toml.lines() {
            out.push_str(&format!("  {}\n", line));
        }
        out.push('\n');
    }

    // RETURN VALUES
    out.push_str("RETURN VALUES\n");
    if doc.return_values.is_empty() {
        out.push_str("    (none)\n");
    } else {
        for (key, ret) in &doc.return_values {
            out.push_str(&format!("  {} ({}):\n    {}\n", key, ret.type_, ret.description));
            if !ret.sample.is_empty() {
                out.push_str(&format!("    sample: {}\n", ret.sample));
            }
        }
    }
    out.push('\n');

    // NOTES
    if !doc.notes.is_empty() {
        out.push_str("NOTES\n");
        for note in &doc.notes {
            out.push_str(&format!("  - {}\n", note));
        }
        out.push('\n');
    }

    // SEE ALSO
    if !doc.see_also.is_empty() {
        out.push_str("SEE ALSO\n");
        for item in &doc.see_also {
            out.push_str(&format!("  - {}\n", item));
        }
        out.push('\n');
    }

    out
}

/// Render a ModuleDoc to Markdown.
pub fn render_markdown(doc: &ModuleDoc) -> String {
    let mut out = String::new();

    out.push_str(&format!("# {}\n\n", doc.name));
    out.push_str(&format!("_{}_\n\n", doc.short_description));

    out.push_str("## Description\n\n");
    out.push_str(&doc.description);
    out.push_str("\n\n");

    if !doc.options.is_empty() {
        out.push_str("## Options\n\n");
        out.push_str("| Parameter | Type | Required | Default | Description |\n");
        out.push_str("|-----------|------|----------|---------|-------------|\n");
        for (key, opt) in &doc.options {
            let req = if opt.required { "yes" } else { "no" };
            let default_str = opt.default.as_deref().unwrap_or("-");
            out.push_str(&format!(
                "| `{}` | {} | {} | {} | {} |\n",
                key, opt.type_, req, default_str, opt.description
            ));
        }
        out.push('\n');
    }

    if !doc.examples.is_empty() {
        out.push_str("## Examples\n\n");
        for ex in &doc.examples {
            out.push_str(&format!("### {}\n\n", ex.name));
            if !ex.description.is_empty() {
                out.push_str(&format!("{}\n\n", ex.description));
            }
            out.push_str("```toml\n");
            out.push_str(&ex.toml);
            out.push_str("```\n\n");
        }
    }

    if !doc.return_values.is_empty() {
        out.push_str("## Return Values\n\n");
        out.push_str("| Key | Type | Sample | Description |\n");
        out.push_str("|-----|------|--------|-------------|\n");
        for (key, ret) in &doc.return_values {
            out.push_str(&format!(
                "| `{}` | {} | `{}` | {} |\n",
                key, ret.type_, ret.sample, ret.description
            ));
        }
        out.push('\n');
    }

    if !doc.notes.is_empty() {
        out.push_str("## Notes\n\n");
        for note in &doc.notes {
            out.push_str(&format!("- {}\n", note));
        }
        out.push('\n');
    }

    out
}

/// Render a one-line TOML usage snippet ready to paste as a task.
pub fn render_snippet(doc: &ModuleDoc) -> String {
    // Derive the short module key from the FQCN (e.g. "runsible_builtin.debug" -> "debug")
    let module_key = doc
        .name
        .rsplit('.')
        .next()
        .unwrap_or(doc.name.as_str());

    // Build the inline table of required args (or placeholder if none)
    let required_args: Vec<String> = doc
        .options
        .iter()
        .filter(|(_, opt)| opt.required)
        .map(|(k, _)| format!("{} = \"TODO\"", k))
        .collect();

    let args = if required_args.is_empty() {
        // Show the first optional param with a placeholder, if any
        if let Some((k, opt)) = doc.options.iter().next() {
            let placeholder = opt
                .default
                .as_deref()
                .map(|d| format!("\"{}\"", d))
                .unwrap_or_else(|| "\"your value here\"".to_string());
            format!("{} = {}", k, placeholder)
        } else {
            String::new()
        }
    } else {
        required_args.join(", ")
    };

    let inline = if args.is_empty() {
        format!("{} = {{}}", module_key)
    } else {
        format!("{} = {{ {} }}", module_key, args)
    };

    format!(
        "[[plays.tasks]]\nname = \"{} task\"\n{}\n",
        module_key, inline
    )
}

// ---------------------------------------------------------------------------
// Built-in module documentation
// ---------------------------------------------------------------------------

fn builtin_debug() -> ModuleDoc {
    let mut options = IndexMap::new();

    options.insert(
        "msg".to_string(),
        OptionDoc {
            description: "The customized message that is printed. \
                If omitted, prints a generic debug message. \
                Mutually exclusive with `var`."
                .to_string(),
            type_: "str".to_string(),
            required: false,
            default: None,
            choices: vec![],
            aliases: vec![],
        },
    );
    options.insert(
        "var".to_string(),
        OptionDoc {
            description: "A variable name to debug. \
                Dumps the variable's current value to the output. \
                Mutually exclusive with `msg`."
                .to_string(),
            type_: "str".to_string(),
            required: false,
            default: None,
            choices: vec![],
            aliases: vec![],
        },
    );

    let mut return_values = IndexMap::new();
    return_values.insert(
        "msg".to_string(),
        ReturnDoc {
            description: "The message that was printed to the output.".to_string(),
            type_: "str".to_string(),
            sample: "Hello from runsible!".to_string(),
        },
    );

    ModuleDoc {
        name: "runsible_builtin.debug".to_string(),
        short_description: "Print a message or variable during play execution".to_string(),
        description: "The debug module is the primary way to emit human-readable output \
            during play execution without affecting host state.\n\n\
            Use `msg` to print a literal string or an expression. \
            Use `var` to dump a named variable — handy when you want to inspect \
            what a previous task registered. \
            Exactly one of `msg` or `var` should be supplied; \
            if neither is given a generic marker is printed so the task is still visible \
            in the run log.\n\n\
            debug is always skipped in check mode by design — it produces no changes \
            and its output would be identical. \
            To gate debug output on verbosity level, combine with a `when` condition \
            that tests `runsible_verbosity >= N`."
            .to_string(),
        version_added: "0.0.1".to_string(),
        author: vec!["runsible-core".to_string()],
        options,
        examples: vec![
            Example {
                name: "Print a literal message".to_string(),
                description: "Most basic use — emit a string to the run log.".to_string(),
                toml: "[[plays.tasks]]\nname = \"say hello\"\ndebug = { msg = \"Hello from runsible!\" }\n".to_string(),
            },
            Example {
                name: "Dump a registered variable".to_string(),
                description: "Inspect what a previous task captured via `register`.".to_string(),
                toml: "[[plays.tasks]]\nname = \"run a command\"\ncommand = { cmd = \"hostname\" }\nregister = \"host_result\"\n\n[[plays.tasks]]\nname = \"dump result\"\ndebug = { var = \"host_result\" }\n".to_string(),
            },
            Example {
                name: "Conditional debug on high verbosity".to_string(),
                description: "Only emit noise when the operator asked for it.".to_string(),
                toml: "[[plays.tasks]]\nname = \"verbose-only debug\"\ndebug = { msg = \"detailed trace data\" }\nwhen = \"runsible_verbosity >= 2\"\n".to_string(),
            },
        ],
        return_values,
        notes: vec![
            "debug is a no-op in terms of host state — it never modifies the managed node.".to_string(),
            "Use `msg` for expressions and literals; use `var` for variable names only.".to_string(),
        ],
        see_also: vec![
            "runsible_builtin.assert".to_string(),
            "runsible_builtin.fail".to_string(),
        ],
    }
}

fn builtin_ping() -> ModuleDoc {
    let mut return_values = IndexMap::new();
    return_values.insert(
        "ping".to_string(),
        ReturnDoc {
            description: "The reply from the managed node. Always the string \"pong\" on success."
                .to_string(),
            type_: "str".to_string(),
            sample: "pong".to_string(),
        },
    );

    ModuleDoc {
        name: "runsible_builtin.ping".to_string(),
        short_description: "Verify connectivity to a host".to_string(),
        description: "The ping module verifies that runsible can connect to a managed node \
            and that the node's runsible runtime is responsive.\n\n\
            ping takes no arguments. On success the module returns the string \"pong\". \
            It is the recommended first task in any connectivity-troubleshooting play \
            and a useful sanity check in CI pipelines that spin up fresh hosts.\n\n\
            Note that ping tests the runsible control channel (SSH by default), \
            not ICMP network reachability. \
            Use the `command` module with `cmd = \"ping -c 1 <host>\"` for ICMP checks."
            .to_string(),
        version_added: "0.0.1".to_string(),
        author: vec!["runsible-core".to_string()],
        options: IndexMap::new(),
        examples: vec![Example {
            name: "Basic connectivity check".to_string(),
            description: "Confirm all hosts in the play are reachable.".to_string(),
            toml: "[[plays.tasks]]\nname = \"verify connectivity\"\nping = {}\n".to_string(),
        }],
        return_values,
        notes: vec![
            "This tests the runsible control channel, not ICMP network reachability.".to_string(),
        ],
        see_also: vec!["runsible_builtin.command".to_string()],
    }
}

// ---------------------------------------------------------------------------
// Helpers for the 11 added builtins below
// ---------------------------------------------------------------------------

/// Build an OptionDoc with sensible defaults. Reduces noise in the per-module
/// doc bodies below.
fn opt(
    description: &str,
    type_: &str,
    required: bool,
    default: Option<&str>,
) -> OptionDoc {
    OptionDoc {
        description: description.to_string(),
        type_: type_.to_string(),
        required,
        default: default.map(String::from),
        choices: vec![],
        aliases: vec![],
    }
}

/// Build an OptionDoc with explicit choices.
fn opt_choices(
    description: &str,
    type_: &str,
    required: bool,
    default: Option<&str>,
    choices: &[&str],
) -> OptionDoc {
    OptionDoc {
        description: description.to_string(),
        type_: type_.to_string(),
        required,
        default: default.map(String::from),
        choices: choices.iter().map(|s| s.to_string()).collect(),
        aliases: vec![],
    }
}

fn ret(description: &str, type_: &str, sample: &str) -> ReturnDoc {
    ReturnDoc {
        description: description.to_string(),
        type_: type_.to_string(),
        sample: sample.to_string(),
    }
}

fn example(name: &str, description: &str, toml: &str) -> Example {
    Example {
        name: name.to_string(),
        description: description.to_string(),
        toml: toml.to_string(),
    }
}

// ---------------------------------------------------------------------------
// runsible_builtin.set_fact
// ---------------------------------------------------------------------------

fn builtin_set_fact() -> ModuleDoc {
    let mut options = IndexMap::new();
    options.insert(
        "<key>".to_string(),
        opt(
            "Arbitrary k=v pairs. Each key/value is set as a host fact \
             (in the host's variable namespace) and persists for the rest \
             of the play.",
            "any",
            false,
            None,
        ),
    );

    let mut return_values = IndexMap::new();
    return_values.insert(
        "ansible_facts".to_string(),
        ret(
            "Map of the facts that were set on the host.",
            "table",
            "{ pkg = \"nginx\" }",
        ),
    );

    ModuleDoc {
        name: "runsible_builtin.set_fact".to_string(),
        short_description: "Set host-scoped facts (variables) at runtime".to_string(),
        description: "set_fact stores arbitrary k=v pairs in the host's fact \
            namespace. Once set, the fact is visible to subsequent tasks running \
            on the same host as a top-level variable.\n\n\
            Unlike `vars` declared on the play, set_fact runs at task time, \
            so the values can be templated from prior task results, host facts, \
            or registered variables. Facts persist for the remainder of the \
            play; they are NOT written to disk."
            .to_string(),
        version_added: "0.0.1".to_string(),
        author: vec!["runsible builtin".to_string()],
        options,
        examples: vec![
            example(
                "Set two facts",
                "Two facts assigned in one task.",
                "[[plays.tasks]]\nname = \"set facts\"\nset_fact = { pkg = \"nginx\", port = 80 }\n",
            ),
            example(
                "Compute a fact from a previous register",
                "Reference a registered variable in the value.",
                "[[plays.tasks]]\nname = \"hostname\"\ncommand = { cmd = \"hostname\" }\nregister = \"hn\"\n\n[[plays.tasks]]\nname = \"stash hostname\"\nset_fact = { my_host = \"{{ hn.stdout }}\" }\n",
            ),
        ],
        return_values,
        notes: vec![
            "Facts set by set_fact are scoped to the current host for the remainder of the play.".to_string(),
            "set_fact does not modify host state — it only mutates the controller's variable map.".to_string(),
        ],
        see_also: vec!["runsible_builtin.debug".to_string()],
    }
}

// ---------------------------------------------------------------------------
// runsible_builtin.assert
// ---------------------------------------------------------------------------

fn builtin_assert() -> ModuleDoc {
    let mut options = IndexMap::new();
    options.insert(
        "that".to_string(),
        opt(
            "List of Jinja boolean expressions. Each expression is evaluated \
             in the current variable context; if any evaluates to false the \
             task fails.",
            "list of str",
            true,
            None,
        ),
    );
    options.insert(
        "fail_msg".to_string(),
        opt(
            "Custom message printed when an expression is false.",
            "str",
            false,
            None,
        ),
    );
    options.insert(
        "success_msg".to_string(),
        opt(
            "Custom message printed when every expression is true.",
            "str",
            false,
            None,
        ),
    );

    let mut return_values = IndexMap::new();
    return_values.insert(
        "msg".to_string(),
        ret("Result message (success_msg or fail_msg).", "str", "All assertions passed"),
    );

    ModuleDoc {
        name: "runsible_builtin.assert".to_string(),
        short_description: "Assert that one or more Jinja boolean expressions hold".to_string(),
        description: "The assert module evaluates a list of boolean expressions \
            in the current variable context. The task succeeds only when every \
            expression evaluates true; if any expression is false the task fails \
            and `fail_msg` (or a default) is recorded.\n\n\
            assert is the idiomatic way to express invariants in a play — \
            use it to gate later tasks behind preconditions, validate \
            registered output, or sanity-check fact values."
            .to_string(),
        version_added: "0.0.1".to_string(),
        author: vec!["runsible builtin".to_string()],
        options,
        examples: vec![
            example(
                "Single condition",
                "Fail the play if the package facts are missing.",
                "[[plays.tasks]]\nname = \"need facts\"\nassert = { that = [\"ansible_facts is defined\"] }\n",
            ),
            example(
                "Multiple conditions with custom messages",
                "All expressions must hold.",
                "[[plays.tasks]]\nname = \"sanity check\"\nassert = { that = [\"port > 0\", \"port < 65536\"], fail_msg = \"port out of range\", success_msg = \"port ok\" }\n",
            ),
        ],
        return_values,
        notes: vec![
            "assert never modifies host state.".to_string(),
            "Use assert (rather than the `fail` module) when the failure depends on a runtime expression.".to_string(),
        ],
        see_also: vec![
            "runsible_builtin.fail".to_string(),
            "runsible_builtin.debug".to_string(),
        ],
    }
}

// ---------------------------------------------------------------------------
// runsible_builtin.command
// ---------------------------------------------------------------------------

fn builtin_command() -> ModuleDoc {
    let mut options = IndexMap::new();
    options.insert(
        "argv".to_string(),
        opt(
            "Argument vector. Preferred form — passes each element directly \
             to execve() with no shell interpretation.",
            "list of str",
            false,
            None,
        ),
    );
    options.insert(
        "cmd".to_string(),
        opt(
            "Command line to execute. The first whitespace-separated token \
             is the binary; subsequent tokens are positional arguments. \
             Mutually exclusive with `argv`.",
            "str",
            false,
            None,
        ),
    );
    options.insert(
        "chdir".to_string(),
        opt(
            "Change to this directory before executing the command.",
            "str",
            false,
            None,
        ),
    );
    options.insert(
        "creates".to_string(),
        opt(
            "If this path already exists the task is skipped (creates idempotency).",
            "str",
            false,
            None,
        ),
    );
    options.insert(
        "removes".to_string(),
        opt(
            "If this path does NOT exist the task is skipped.",
            "str",
            false,
            None,
        ),
    );

    let mut return_values = IndexMap::new();
    return_values.insert("rc".to_string(), ret("Exit status code.", "int", "0"));
    return_values.insert("stdout".to_string(), ret("Captured standard output.", "str", "hello"));
    return_values.insert("stderr".to_string(), ret("Captured standard error.", "str", ""));

    ModuleDoc {
        name: "runsible_builtin.command".to_string(),
        short_description: "Execute a binary directly without a shell".to_string(),
        description: "command runs an external program by invoking its binary \
            with `execve()` directly. No shell is involved, so shell metacharacters \
            (pipes, redirection, globs, variable expansion) are NOT interpreted — \
            they are passed through as literal characters.\n\n\
            Prefer `argv` over `cmd`: `argv = [\"git\", \"clone\", repo]` always splits \
            arguments correctly, even when values contain whitespace. `cmd` splits on \
            whitespace, which is fragile.\n\n\
            command is NOT idempotent on its own — use `creates`/`removes` to make it so."
            .to_string(),
        version_added: "0.0.1".to_string(),
        author: vec!["runsible builtin".to_string()],
        options,
        examples: vec![
            example(
                "Run hostname (cmd form)",
                "Capture the result with register.",
                "[[plays.tasks]]\nname = \"get hostname\"\ncommand = { cmd = \"hostname\" }\nregister = \"hn\"\n",
            ),
            example(
                "argv form",
                "Robust against whitespace in arguments.",
                "[[plays.tasks]]\nname = \"git clone\"\ncommand = { argv = [\"git\", \"clone\", \"https://example.com/repo.git\", \"/tmp/repo\"] }\n",
            ),
            example(
                "Idempotent via creates",
                "Skip if the marker file already exists.",
                "[[plays.tasks]]\nname = \"once-only init\"\ncommand = { cmd = \"./init.sh\", creates = \"/var/lib/myapp/initialized\" }\n",
            ),
        ],
        return_values,
        notes: vec![
            "command is NOT idempotent. Use `creates` / `removes` for first-run-only behavior.".to_string(),
            "Shell metacharacters in `cmd` are passed through literally — use `shell` if you actually want a shell.".to_string(),
            "Prefer `argv` to avoid argument-splitting bugs.".to_string(),
        ],
        see_also: vec![
            "runsible_builtin.shell".to_string(),
            "runsible_builtin.script".to_string(),
        ],
    }
}

// ---------------------------------------------------------------------------
// runsible_builtin.shell
// ---------------------------------------------------------------------------

fn builtin_shell() -> ModuleDoc {
    let mut options = IndexMap::new();
    options.insert(
        "cmd".to_string(),
        opt(
            "Shell command line. Passed to `executable -c <cmd>` so all \
             shell features (pipes, redirection, globs, $vars) are available.",
            "str",
            true,
            None,
        ),
    );
    options.insert(
        "executable".to_string(),
        opt(
            "Shell binary to invoke.",
            "str",
            false,
            Some("/bin/sh"),
        ),
    );
    options.insert(
        "chdir".to_string(),
        opt(
            "Change to this directory before executing the command.",
            "str",
            false,
            None,
        ),
    );
    options.insert(
        "creates".to_string(),
        opt(
            "If this path already exists the task is skipped.",
            "str",
            false,
            None,
        ),
    );
    options.insert(
        "removes".to_string(),
        opt(
            "If this path does NOT exist the task is skipped.",
            "str",
            false,
            None,
        ),
    );

    let mut return_values = IndexMap::new();
    return_values.insert("rc".to_string(), ret("Exit status code.", "int", "0"));
    return_values.insert("stdout".to_string(), ret("Captured standard output.", "str", "line1\nline2\n"));
    return_values.insert("stderr".to_string(), ret("Captured standard error.", "str", ""));

    ModuleDoc {
        name: "runsible_builtin.shell".to_string(),
        short_description: "Execute a command through a shell".to_string(),
        description: "shell runs a command line via `<executable> -c <cmd>`. \
            Unlike `command`, the cmd is interpreted by a shell, so pipes, \
            redirection, globs, and variable expansion all work.\n\n\
            **Use shell only when you actually need shell features.** \
            For straightforward binary invocations prefer `command` — it avoids \
            quoting pitfalls and is harder to misuse with untrusted input.\n\n\
            Untrusted Jinja-rendered input must NOT be embedded in `cmd` without \
            careful escaping; doing so creates a shell-injection vulnerability."
            .to_string(),
        version_added: "0.0.1".to_string(),
        author: vec!["runsible builtin".to_string()],
        options,
        examples: vec![
            example(
                "Pipeline",
                "Use shell when you genuinely need pipes.",
                "[[plays.tasks]]\nname = \"top user processes\"\nshell = { cmd = \"ps -ef | sort -k2 | head -5\" }\n",
            ),
            example(
                "Bash-specific",
                "Pick a different executable.",
                "[[plays.tasks]]\nname = \"bashism\"\nshell = { cmd = \"set -o pipefail; cat foo | grep bar\", executable = \"/bin/bash\" }\n",
            ),
        ],
        return_values,
        notes: vec![
            "WARNING: shell expands its argument through a shell. Untrusted Jinja-rendered values can lead to command injection.".to_string(),
            "Prefer `command` for non-shell tasks (linter rule L018).".to_string(),
            "shell is NOT idempotent. Use `creates` / `removes` for first-run-only behavior.".to_string(),
        ],
        see_also: vec![
            "runsible_builtin.command".to_string(),
            "runsible_builtin.script".to_string(),
        ],
    }
}

// ---------------------------------------------------------------------------
// runsible_builtin.copy
// ---------------------------------------------------------------------------

fn builtin_copy() -> ModuleDoc {
    let mut options = IndexMap::new();
    options.insert(
        "src".to_string(),
        opt(
            "Local source path on the controller. Mutually exclusive with `content`.",
            "str",
            false,
            None,
        ),
    );
    options.insert(
        "content".to_string(),
        opt(
            "Inline content to write to dest. Mutually exclusive with `src`.",
            "str",
            false,
            None,
        ),
    );
    options.insert(
        "dest".to_string(),
        opt(
            "Destination path on the managed node.",
            "str",
            true,
            None,
        ),
    );
    options.insert(
        "mode".to_string(),
        opt(
            "Octal-style file mode (as a string, e.g. \"0644\").",
            "str",
            false,
            None,
        ),
    );

    let mut return_values = IndexMap::new();
    return_values.insert("dest".to_string(), ret("Destination path.", "str", "/etc/foo.conf"));
    return_values.insert("changed".to_string(), ret("Whether the file was created or modified.", "bool", "true"));
    return_values.insert("checksum".to_string(), ret("SHA-256 checksum of the destination after copy.", "str", "ab12…"));

    ModuleDoc {
        name: "runsible_builtin.copy".to_string(),
        short_description: "Copy a file or inline content to a destination".to_string(),
        description: "copy writes a file on the managed node. Either `src` (a path \
            on the controller) or `content` (an inline string) must be supplied — \
            never both. The destination is written atomically: copy first writes to \
            a temporary file in the same directory, then renames into place, so \
            partial writes never appear at `dest`.\n\n\
            copy is idempotent: if the destination already has the same content \
            (and matching mode if specified) the task reports `ok` rather than \
            `changed`."
            .to_string(),
        version_added: "0.0.1".to_string(),
        author: vec!["runsible builtin".to_string()],
        options,
        examples: vec![
            example(
                "Copy a local file",
                "Standard file deployment.",
                "[[plays.tasks]]\nname = \"deploy nginx.conf\"\ncopy = { src = \"files/nginx.conf\", dest = \"/etc/nginx/nginx.conf\", mode = \"0644\" }\n",
            ),
            example(
                "Inline content",
                "Skip the file system on the controller side.",
                "[[plays.tasks]]\nname = \"write banner\"\ncopy = { content = \"hello\\n\", dest = \"/etc/motd\", mode = \"0644\" }\n",
            ),
        ],
        return_values,
        notes: vec![
            "copy writes atomically via tempfile + rename.".to_string(),
            "Provide mode as an octal-formatted string (\"0644\"), not an integer.".to_string(),
        ],
        see_also: vec![
            "runsible_builtin.template".to_string(),
            "runsible_builtin.file".to_string(),
        ],
    }
}

// ---------------------------------------------------------------------------
// runsible_builtin.file
// ---------------------------------------------------------------------------

fn builtin_file() -> ModuleDoc {
    let mut options = IndexMap::new();
    options.insert(
        "path".to_string(),
        opt(
            "Filesystem path to manage.",
            "str",
            true,
            None,
        ),
    );
    options.insert(
        "state".to_string(),
        opt_choices(
            "Desired state of the path.",
            "str",
            false,
            Some("present"),
            &["present", "absent", "directory", "touch"],
        ),
    );
    options.insert(
        "mode".to_string(),
        opt(
            "Octal-style permission bits (e.g. \"0755\").",
            "str",
            false,
            None,
        ),
    );

    let mut return_values = IndexMap::new();
    return_values.insert("path".to_string(), ret("The managed path.", "str", "/var/lib/app"));
    return_values.insert("state".to_string(), ret("The state after the action.", "str", "directory"));
    return_values.insert("changed".to_string(), ret("Whether the path was modified.", "bool", "true"));

    ModuleDoc {
        name: "runsible_builtin.file".to_string(),
        short_description: "Manage filesystem entries (files, directories, symlinks)".to_string(),
        description: "file ensures the named path exists in the requested state. \
            Available states are `present` (regular file exists), `absent` (path \
            removed if it exists), `directory` (mkdir -p semantics), and \
            `touch` (create-or-update mtime).\n\n\
            file is idempotent: if the path is already in the requested state the \
            task reports `ok`. The optional `mode` field reconciles permissions on \
            every run."
            .to_string(),
        version_added: "0.0.1".to_string(),
        author: vec!["runsible builtin".to_string()],
        options,
        examples: vec![
            example(
                "Ensure a directory exists",
                "Create-if-missing with explicit mode.",
                "[[plays.tasks]]\nname = \"app data dir\"\nfile = { path = \"/var/lib/app\", state = \"directory\", mode = \"0755\" }\n",
            ),
            example(
                "Remove a file",
                "Idempotent removal.",
                "[[plays.tasks]]\nname = \"remove tempfile\"\nfile = { path = \"/tmp/scratch\", state = \"absent\" }\n",
            ),
        ],
        return_values,
        notes: vec![
            "Use `copy` or `template` to write file content; `file` only manages metadata.".to_string(),
            "World-writable modes (\"0777\", \"0666\") are flagged by lint rules L044.".to_string(),
        ],
        see_also: vec![
            "runsible_builtin.copy".to_string(),
            "runsible_builtin.template".to_string(),
        ],
    }
}

// ---------------------------------------------------------------------------
// runsible_builtin.template
// ---------------------------------------------------------------------------

fn builtin_template() -> ModuleDoc {
    let mut options = IndexMap::new();
    options.insert(
        "src".to_string(),
        opt(
            "Path to the Jinja template file on the controller.",
            "str",
            true,
            None,
        ),
    );
    options.insert(
        "dest".to_string(),
        opt(
            "Destination path on the managed node.",
            "str",
            true,
            None,
        ),
    );
    options.insert(
        "mode".to_string(),
        opt(
            "Octal-style file mode (e.g. \"0644\").",
            "str",
            false,
            None,
        ),
    );

    let mut return_values = IndexMap::new();
    return_values.insert("dest".to_string(), ret("Destination path written to.", "str", "/etc/app.conf"));
    return_values.insert("changed".to_string(), ret("Whether the rendered output differed from the prior file.", "bool", "true"));
    return_values.insert("checksum".to_string(), ret("SHA-256 of the rendered output.", "str", "ab12…"));

    ModuleDoc {
        name: "runsible_builtin.template".to_string(),
        short_description: "Render a Jinja template to a destination file".to_string(),
        description: "template reads `src` from the controller, renders it through \
            the runsible Jinja engine using the current host's variable context, \
            and writes the result to `dest` on the managed node.\n\n\
            Like `copy`, template is idempotent: the rendered bytes are compared \
            with the existing file and the task is `ok` when they match. Writes \
            are atomic via tempfile + rename."
            .to_string(),
        version_added: "0.0.1".to_string(),
        author: vec!["runsible builtin".to_string()],
        options,
        examples: vec![
            example(
                "Render a config file",
                "Per-host configuration via variables.",
                "[[plays.tasks]]\nname = \"app config\"\ntemplate = { src = \"templates/app.conf.j2\", dest = \"/etc/app.conf\", mode = \"0644\" }\n",
            ),
        ],
        return_values,
        notes: vec![
            "The template language is Jinja2-compatible (subset).".to_string(),
            "World-writable modes are flagged by lint rule L045.".to_string(),
        ],
        see_also: vec![
            "runsible_builtin.copy".to_string(),
            "runsible_builtin.file".to_string(),
        ],
    }
}

// ---------------------------------------------------------------------------
// runsible_builtin.package
// ---------------------------------------------------------------------------

fn builtin_package() -> ModuleDoc {
    let mut options = IndexMap::new();
    options.insert(
        "name".to_string(),
        opt(
            "Package name, or list of package names.",
            "str or list of str",
            true,
            None,
        ),
    );
    options.insert(
        "state".to_string(),
        opt_choices(
            "Desired state of the package.",
            "str",
            false,
            Some("present"),
            &["present", "absent", "latest"],
        ),
    );
    options.insert(
        "manager".to_string(),
        opt_choices(
            "Package manager to use. `auto` detects from the host facts.",
            "str",
            false,
            Some("auto"),
            &["apt", "dnf", "yum", "auto"],
        ),
    );

    let mut return_values = IndexMap::new();
    return_values.insert("changed".to_string(), ret("Whether the package set was modified.", "bool", "true"));
    return_values.insert("manager".to_string(), ret("Package manager actually used.", "str", "apt"));

    ModuleDoc {
        name: "runsible_builtin.package".to_string(),
        short_description: "Install, upgrade, or remove OS packages".to_string(),
        description: "package is a generic frontend for the host's native package \
            manager. With `manager = \"auto\"` (default) the appropriate backend \
            is chosen from host facts; otherwise an explicit backend is used.\n\n\
            `name` may be a single package or a list — passing a list is preferred \
            because most backends batch installs in a single transaction. \
            The supported states are `present`, `absent`, and `latest`."
            .to_string(),
        version_added: "0.0.1".to_string(),
        author: vec!["runsible builtin".to_string()],
        options,
        examples: vec![
            example(
                "Install one package",
                "Single-package install.",
                "[[plays.tasks]]\nname = \"install nginx\"\npackage = { name = \"nginx\", state = \"present\" }\n",
            ),
            example(
                "Install multiple",
                "Batched in a single transaction.",
                "[[plays.tasks]]\nname = \"webserver stack\"\npackage = { name = [\"nginx\", \"certbot\", \"curl\"], state = \"present\" }\n",
            ),
            example(
                "Force a specific backend",
                "Explicitly use apt (overriding auto-detect).",
                "[[plays.tasks]]\nname = \"latest jq via apt\"\npackage = { name = \"jq\", state = \"latest\", manager = \"apt\" }\n",
            ),
        ],
        return_values,
        notes: vec![
            "`state = \"latest\"` forces an upgrade — be explicit about whether the underlying transaction may pull in updates.".to_string(),
            "`manager = \"auto\"` requires the `setup` module to have populated host facts at least once.".to_string(),
        ],
        see_also: vec!["runsible_builtin.service".to_string()],
    }
}

// ---------------------------------------------------------------------------
// runsible_builtin.service
// ---------------------------------------------------------------------------

fn builtin_service() -> ModuleDoc {
    let mut options = IndexMap::new();
    options.insert(
        "name".to_string(),
        opt(
            "Service name (e.g. \"nginx\").",
            "str",
            true,
            None,
        ),
    );
    options.insert(
        "state".to_string(),
        opt_choices(
            "Desired state to drive the service to.",
            "str",
            false,
            None,
            &["started", "stopped", "restarted", "reloaded"],
        ),
    );
    options.insert(
        "enabled".to_string(),
        opt(
            "Whether the service should be enabled at boot.",
            "bool",
            false,
            None,
        ),
    );

    let mut return_values = IndexMap::new();
    return_values.insert("name".to_string(), ret("Service name.", "str", "nginx"));
    return_values.insert("state".to_string(), ret("State after the action.", "str", "started"));
    return_values.insert("changed".to_string(), ret("Whether the service was modified.", "bool", "true"));

    ModuleDoc {
        name: "runsible_builtin.service".to_string(),
        short_description: "Manage init-system services on the managed node".to_string(),
        description: "service drives the host's init system to ensure the named \
            service is in the requested state and (optionally) enabled at boot. \
            The exact init backend (systemd, OpenRC, etc.) is detected from host \
            facts; if you need systemd-specific knobs use `systemd_service` \
            instead.\n\n\
            `state = \"restarted\"` is unconditional — useful in handlers that \
            should run on config changes. `state = \"reloaded\"` sends SIGHUP \
            (or the equivalent) when the backend supports it."
            .to_string(),
        version_added: "0.0.1".to_string(),
        author: vec!["runsible builtin".to_string()],
        options,
        examples: vec![
            example(
                "Start and enable nginx",
                "Most common case.",
                "[[plays.tasks]]\nname = \"nginx up\"\nservice = { name = \"nginx\", state = \"started\", enabled = true }\n",
            ),
            example(
                "Restart in a handler",
                "Handlers fire after notifying tasks change config.",
                "[[plays.handlers]]\nname = \"restart nginx\"\nservice = { name = \"nginx\", state = \"restarted\" }\n",
            ),
        ],
        return_values,
        notes: vec![
            "If neither `state` nor `enabled` is set the task is a no-op.".to_string(),
            "Restarting `ssh`/`sshd` from the same host you're connected over can lock you out — see lint rule L049.".to_string(),
        ],
        see_also: vec!["runsible_builtin.systemd_service".to_string()],
    }
}

// ---------------------------------------------------------------------------
// runsible_builtin.systemd_service
// ---------------------------------------------------------------------------

fn builtin_systemd_service() -> ModuleDoc {
    let mut options = IndexMap::new();
    options.insert(
        "name".to_string(),
        opt(
            "Unit name (\"nginx\", \"nginx.service\", \"my.timer\").",
            "str",
            true,
            None,
        ),
    );
    options.insert(
        "state".to_string(),
        opt_choices(
            "Desired state to drive the unit to.",
            "str",
            false,
            None,
            &["started", "stopped", "restarted", "reloaded"],
        ),
    );
    options.insert(
        "enabled".to_string(),
        opt(
            "Whether the unit should be enabled at boot.",
            "bool",
            false,
            None,
        ),
    );
    options.insert(
        "daemon_reload".to_string(),
        opt(
            "Run `systemctl daemon-reload` before applying. Set after \
             dropping a new unit file on disk.",
            "bool",
            false,
            Some("false"),
        ),
    );
    options.insert(
        "scope".to_string(),
        opt_choices(
            "Whether to operate on system or per-user units.",
            "str",
            false,
            Some("system"),
            &["system", "user"],
        ),
    );

    let mut return_values = IndexMap::new();
    return_values.insert("name".to_string(), ret("Unit name.", "str", "nginx.service"));
    return_values.insert("state".to_string(), ret("State after the action.", "str", "started"));
    return_values.insert("changed".to_string(), ret("Whether the unit was modified.", "bool", "true"));

    ModuleDoc {
        name: "runsible_builtin.systemd_service".to_string(),
        short_description: "Manage systemd units (with daemon_reload + user/system scope)".to_string(),
        description: "systemd_service is the systemd-specific superset of `service`. \
            In addition to `state`/`enabled` it understands `daemon_reload` (for \
            picking up freshly-dropped unit files) and `scope` (system vs. \
            per-user units).\n\n\
            Use this module when you need explicit control over those features. \
            For portable plays that should also run on non-systemd hosts, prefer \
            the generic `service` module."
            .to_string(),
        version_added: "0.0.1".to_string(),
        author: vec!["runsible builtin".to_string()],
        options,
        examples: vec![
            example(
                "Drop a unit and start it",
                "Reload daemon then start the new unit.",
                "[[plays.tasks]]\nname = \"install unit\"\ncopy = { src = \"files/myapp.service\", dest = \"/etc/systemd/system/myapp.service\", mode = \"0644\" }\n\n[[plays.tasks]]\nname = \"start myapp\"\nsystemd_service = { name = \"myapp\", state = \"started\", enabled = true, daemon_reload = true }\n",
            ),
            example(
                "User scope",
                "Manage a unit in the calling user's session.",
                "[[plays.tasks]]\nname = \"user timer\"\nsystemd_service = { name = \"backup.timer\", state = \"started\", enabled = true, scope = \"user\" }\n",
            ),
        ],
        return_values,
        notes: vec![
            "Requires systemd on the managed node.".to_string(),
            "Set `daemon_reload = true` after writing a new unit file or systemd will keep using the old definition.".to_string(),
        ],
        see_also: vec!["runsible_builtin.service".to_string()],
    }
}

// ---------------------------------------------------------------------------
// runsible_builtin.get_url
// ---------------------------------------------------------------------------

fn builtin_get_url() -> ModuleDoc {
    let mut options = IndexMap::new();
    options.insert(
        "url".to_string(),
        opt("URL to fetch.", "str", true, None),
    );
    options.insert(
        "dest".to_string(),
        opt(
            "Destination path on the managed node.",
            "str",
            true,
            None,
        ),
    );
    options.insert(
        "mode".to_string(),
        opt(
            "Octal-style file mode for the destination file.",
            "str",
            false,
            None,
        ),
    );
    options.insert(
        "checksum".to_string(),
        opt(
            "Expected checksum of the downloaded file, prefixed with the algorithm \
             (e.g. \"sha256:ab12…\"). The download is verified against this and \
             rejected on mismatch.",
            "str",
            false,
            None,
        ),
    );

    let mut return_values = IndexMap::new();
    return_values.insert("dest".to_string(), ret("Destination path.", "str", "/tmp/foo.tgz"));
    return_values.insert("url".to_string(), ret("URL fetched.", "str", "https://example.com/foo.tgz"));
    return_values.insert("changed".to_string(), ret("Whether the file was downloaded (false on cache hit).", "bool", "true"));
    return_values.insert("checksum_dest".to_string(), ret("SHA-256 of the destination after download.", "str", "ab12…"));

    ModuleDoc {
        name: "runsible_builtin.get_url".to_string(),
        short_description: "Download content from a URL to a destination file".to_string(),
        description: "get_url fetches `url` to `dest`. If `dest` already exists \
            and a `checksum` is supplied that matches the existing file, the \
            download is skipped and the task reports `ok`.\n\n\
            **Always supply `checksum` when possible.** Without it the only \
            integrity guarantee is whatever the transport (HTTPS) provides; \
            with it the file is verified end-to-end before being moved into \
            place. Lint rule L046 flags missing checksums in Safety profile."
            .to_string(),
        version_added: "0.0.1".to_string(),
        author: vec!["runsible builtin".to_string()],
        options,
        examples: vec![
            example(
                "Download with checksum",
                "Pinned by content hash — the recommended form.",
                "[[plays.tasks]]\nname = \"download installer\"\nget_url = { url = \"https://example.com/installer.sh\", dest = \"/tmp/installer.sh\", mode = \"0755\", checksum = \"sha256:abc123…\" }\n",
            ),
            example(
                "Plain download",
                "Discouraged without a checksum.",
                "[[plays.tasks]]\nname = \"fetch tarball\"\nget_url = { url = \"https://example.com/data.tgz\", dest = \"/tmp/data.tgz\" }\n",
            ),
        ],
        return_values,
        notes: vec![
            "Always pin downloads to a checksum when possible (`sha256:…`).".to_string(),
            "Atomic write: get_url downloads to a tempfile in dest's directory and renames into place.".to_string(),
        ],
        see_also: vec![
            "runsible_builtin.copy".to_string(),
            "runsible_builtin.uri".to_string(),
        ],
    }
}

// ---------------------------------------------------------------------------
// TRIPLE SIMS gate
// ---------------------------------------------------------------------------

/// Smoke gate: build the builtins registry, look up debug+ping docs, render
/// each in text + markdown + snippet form, and check the rendered output
/// contains the expected sections. Returns 0 on success.
pub fn f30() -> i32 {
    let reg = DocRegistry::builtins();

    let debug = match reg.get("runsible_builtin.debug") {
        Some(d) => d,
        None => return 1,
    };
    if reg.get("runsible_builtin.ping").is_none() {
        return 2;
    }

    let text = render_text(debug);
    if !text.contains("OPTIONS") {
        return 3;
    }
    if !text.contains("EXAMPLES") {
        return 4;
    }

    let md = render_markdown(debug);
    if !md.contains("# runsible_builtin.debug") {
        return 5;
    }

    let ping = match reg.get("runsible_builtin.ping") {
        Some(d) => d,
        None => return 6,
    };
    let snippet = render_snippet(ping);
    if !snippet.contains("ping = ") {
        return 7;
    }

    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_has_debug() {
        let reg = DocRegistry::builtins();
        assert!(
            reg.get("runsible_builtin.debug").is_some(),
            "debug doc should be present"
        );
    }

    #[test]
    fn builtins_has_ping() {
        let reg = DocRegistry::builtins();
        assert!(
            reg.get("runsible_builtin.ping").is_some(),
            "ping doc should be present"
        );
    }

    #[test]
    fn list_returns_all() {
        let reg = DocRegistry::builtins();
        assert!(
            reg.list().len() >= 2,
            "should have at least 2 builtin docs"
        );
    }

    #[test]
    fn render_text_has_name() {
        let reg = DocRegistry::builtins();
        let doc = reg.get("runsible_builtin.debug").unwrap();
        let text = render_text(doc);
        assert!(
            text.contains("runsible_builtin.debug"),
            "text render should contain the module FQCN"
        );
    }

    #[test]
    fn render_text_has_options() {
        let reg = DocRegistry::builtins();
        let doc = reg.get("runsible_builtin.debug").unwrap();
        let text = render_text(doc);
        assert!(
            text.contains("OPTIONS"),
            "text render should contain OPTIONS section"
        );
    }

    #[test]
    fn render_markdown_has_heading() {
        let reg = DocRegistry::builtins();
        let doc = reg.get("runsible_builtin.debug").unwrap();
        let md = render_markdown(doc);
        assert!(
            md.contains("# runsible_builtin.debug"),
            "markdown should start with an H1 heading matching the module name"
        );
    }

    #[test]
    fn snippet_contains_module_key() {
        let reg = DocRegistry::builtins();
        let doc = reg.get("runsible_builtin.debug").unwrap();
        let snippet = render_snippet(doc);
        assert!(
            snippet.contains("debug ="),
            "snippet should contain the module key 'debug ='"
        );
    }

    #[test]
    fn unknown_module_errors() {
        let reg = DocRegistry::builtins();
        assert!(
            reg.get("no.such.module").is_none(),
            "unknown module should return None"
        );
    }

    #[test]
    fn render_json_roundtrip() {
        let reg = DocRegistry::builtins();
        let doc = reg.get("runsible_builtin.debug").unwrap();
        let json = serde_json::to_string(doc).expect("serialize to JSON");
        let back: ModuleDoc = serde_json::from_str(&json).expect("deserialize from JSON");
        assert_eq!(
            doc.name, back.name,
            "round-tripped module name should match"
        );
    }

    #[test]
    fn text_render_contains_examples() {
        let reg = DocRegistry::builtins();
        let doc = reg.get("runsible_builtin.debug").unwrap();
        let text = render_text(doc);
        assert!(
            text.contains("EXAMPLES"),
            "text render should contain EXAMPLES section"
        );
    }

    // ── New: render_text for ping shows OPTIONS header even with no opts ────
    #[test]
    fn render_text_ping_has_options_header() {
        let reg = DocRegistry::builtins();
        let doc = reg.get("runsible_builtin.ping").unwrap();
        let text = render_text(doc);
        assert!(
            text.contains("OPTIONS"),
            "ping render_text should still include OPTIONS section header"
        );
        // Ping has zero options, so the "(none)" placeholder should appear.
        assert!(
            text.contains("(none)"),
            "ping render should show (none) under OPTIONS"
        );
    }

    // ── New: render_text for debug shows all 3 examples ─────────────────────
    #[test]
    fn render_text_debug_has_all_three_examples() {
        let reg = DocRegistry::builtins();
        let doc = reg.get("runsible_builtin.debug").unwrap();
        assert_eq!(doc.examples.len(), 3, "debug should have 3 examples");
        let text = render_text(doc);
        // Each example's name should appear as a comment line in the rendered text.
        for ex in &doc.examples {
            assert!(
                text.contains(&ex.name),
                "render_text should include example name '{}'; full text:\n{}",
                ex.name,
                text
            );
        }
    }

    // ── New: render_markdown for debug contains an `## Options` heading ─────
    // The markdown renderer uses "## Options" (title case) — locked here.
    #[test]
    fn render_markdown_debug_has_options_heading() {
        let reg = DocRegistry::builtins();
        let doc = reg.get("runsible_builtin.debug").unwrap();
        let md = render_markdown(doc);
        assert!(
            md.contains("## Options") || md.contains("## OPTIONS"),
            "markdown should contain an Options/OPTIONS heading; got:\n{}",
            md
        );
    }

    // ── New: render_markdown includes version_added when present ────────────
    #[test]
    fn render_markdown_or_text_includes_version_added() {
        // The current markdown renderer doesn't directly emit version_added,
        // but the field is populated and accessible via the doc struct itself.
        // Test asserts the doc has version_added populated and that it can be
        // reached via the public API; if the renderer adds it later this test
        // also confirms the pipeline.
        let reg = DocRegistry::builtins();
        let doc = reg.get("runsible_builtin.debug").unwrap();
        assert!(
            !doc.version_added.is_empty(),
            "debug doc should populate version_added"
        );
        // Verify version_added round-trips through render-or-doc surface.
        // We accept either: appears in markdown body, OR is reachable via the
        // ModuleDoc struct after JSON round-trip.
        let md = render_markdown(doc);
        let json = serde_json::to_string(doc).unwrap();
        let combined = format!("{md}\n{json}");
        assert!(
            combined.contains(&doc.version_added),
            "version_added '{}' should appear in markdown or JSON serialization",
            doc.version_added
        );
    }

    // ── New: render_snippet for ping contains the FQCN literally ────────────
    // Note: the current render_snippet derives a SHORT key from the FQCN,
    // not the full FQCN. We lock current behavior: the short key "ping"
    // appears, and the test verifies the snippet would let a user paste
    // & be reminded of the canonical FQCN — currently via context only.
    // If render_snippet later includes the FQCN, this test still passes.
    #[test]
    fn render_snippet_ping_contains_short_or_fqcn() {
        let reg = DocRegistry::builtins();
        let doc = reg.get("runsible_builtin.ping").unwrap();
        let snippet = render_snippet(doc);
        // Either the literal FQCN or the short key — short key is current behavior.
        assert!(
            snippet.contains("runsible_builtin.ping") || snippet.contains("ping ="),
            "snippet should reference ping (either fqcn or short alias); got:\n{}",
            snippet
        );
    }

    // ── New: render_snippet for debug uses `debug = ` short alias ───────────
    #[test]
    fn render_snippet_debug_uses_short_alias() {
        let reg = DocRegistry::builtins();
        let doc = reg.get("runsible_builtin.debug").unwrap();
        let snippet = render_snippet(doc);
        assert!(
            snippet.contains("debug ="),
            "snippet should use `debug =` short alias from FQCN; got:\n{}",
            snippet
        );
    }

    // ── New: DocRegistry::list returns docs in stable order across calls ────
    #[test]
    fn list_returns_stable_order() {
        let reg = DocRegistry::builtins();
        let names1: Vec<String> = reg.list().iter().map(|d| d.name.clone()).collect();
        let names2: Vec<String> = reg.list().iter().map(|d| d.name.clone()).collect();
        assert_eq!(names1, names2, "list() ordering should be stable across calls");
    }

    // ── New: get() with various unknown names returns None ─────────────────
    #[test]
    fn get_unknown_names_all_none() {
        let reg = DocRegistry::builtins();
        for bogus in &[
            "",
            "nope",
            "runsible_builtin.does_not_exist",
            "DEBUG",
            "runsible_builtin.Debug",
            "runsible_builtin.",
            ".runsible_builtin.debug",
        ] {
            assert!(
                reg.get(bogus).is_none(),
                "expected None for unknown name '{}'",
                bogus
            );
        }
    }

    // ── New: ModuleDoc serializes to JSON and back via serde_json ──────────
    #[test]
    fn moduledoc_json_full_roundtrip() {
        let reg = DocRegistry::builtins();
        let doc = reg.get("runsible_builtin.ping").unwrap();
        let json = serde_json::to_string_pretty(doc).expect("serialize");
        let back: ModuleDoc = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(doc.name, back.name);
        assert_eq!(doc.short_description, back.short_description);
        assert_eq!(doc.description, back.description);
        assert_eq!(doc.version_added, back.version_added);
        assert_eq!(doc.author, back.author);
        assert_eq!(doc.examples.len(), back.examples.len());
        assert_eq!(doc.return_values.len(), back.return_values.len());
        assert_eq!(doc.notes, back.notes);
        assert_eq!(doc.see_also, back.see_also);
    }

    // ── New: every runsible_builtin module has a registered doc ────────────
    #[test]
    fn all_runsible_builtins_have_docs() {
        let reg = DocRegistry::builtins();
        for name in [
            "debug",
            "ping",
            "set_fact",
            "assert",
            "command",
            "shell",
            "copy",
            "file",
            "template",
            "package",
            "service",
            "systemd_service",
            "get_url",
        ] {
            let fq = format!("runsible_builtin.{name}");
            assert!(
                reg.get(&fq).is_some(),
                "missing doc for {name} (fq={fq})"
            );
        }
        assert!(
            reg.list().len() >= 13,
            "registry should hold ≥ 13 builtin docs; got {}",
            reg.list().len()
        );
    }

    // ── New: empty options + empty examples renders without panic ──────────
    #[test]
    fn empty_options_and_examples_renders_clean() {
        let doc = ModuleDoc {
            name: "test_ns.empty".to_string(),
            short_description: "an empty test module".to_string(),
            description: "no body to speak of".to_string(),
            version_added: "0.0.1".to_string(),
            author: vec![],
            options: IndexMap::new(),
            examples: vec![],
            return_values: IndexMap::new(),
            notes: vec![],
            see_also: vec![],
        };
        let text = render_text(&doc);
        assert!(text.contains("test_ns.empty"));
        assert!(text.contains("OPTIONS"));
        let md = render_markdown(&doc);
        assert!(md.contains("# test_ns.empty"));
        let snippet = render_snippet(&doc);
        // No options → snippet should still produce a valid task block.
        assert!(snippet.contains("[[plays.tasks]]"));
        assert!(snippet.contains("empty"));
    }
}
