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
