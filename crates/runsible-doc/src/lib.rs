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
        let debug_doc = builtin_debug();
        let ping_doc = builtin_ping();
        docs.insert(debug_doc.name.clone(), debug_doc);
        docs.insert(ping_doc.name.clone(), ping_doc);
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
}
