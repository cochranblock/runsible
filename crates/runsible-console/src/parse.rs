//! REPL line parser.
//!
//! M0 grammar:
//! - empty / whitespace-only line → `Empty`
//! - line beginning with `#` → `Comment`
//! - `quit` / `exit` (case-insensitive, surrounding whitespace ok) → `Quit`
//! - everything else → `Invoke { module, args }` where `module` is the first
//!   whitespace-separated token and `args` is the remaining `key=val` pairs
//!   parsed into a `toml::Value::Table`.

/// A parsed REPL line.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplCommand {
    /// Blank line — no-op.
    Empty,
    /// Line started with `#` — comment, ignored.
    Comment,
    /// User asked to leave the REPL.
    Quit,
    /// Module invocation.
    Invoke {
        module: String,
        args: toml::Value,
    },
    /// Reserved for future grammar errors. Unused at M0 (we treat any
    /// non-empty/non-comment/non-quit line as an `Invoke`), but kept on the
    /// public API so callers can already match on it.
    Unknown(String),
}

/// Parse a single REPL input line into a `ReplCommand`.
///
/// Never panics; malformed `key=value` arg tokens become an `Invoke` with the
/// offending pieces silently skipped — the engine will then complain about
/// the missing args, which surfaces a more useful error than a parse-time
/// rejection at this layer.
pub fn parse_line(s: &str) -> ReplCommand {
    let trimmed = s.trim();

    if trimmed.is_empty() {
        return ReplCommand::Empty;
    }

    if trimmed.starts_with('#') {
        return ReplCommand::Comment;
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower == "quit" || lower == "exit" {
        return ReplCommand::Quit;
    }

    // First whitespace-separated token is the module name; the rest are args.
    let mut iter = trimmed.split_whitespace();
    let module = match iter.next() {
        Some(m) => m.to_string(),
        None => return ReplCommand::Empty,
    };

    let mut table = toml::map::Map::new();
    for token in iter {
        if let Some((k, v)) = token.split_once('=') {
            table.insert(k.to_string(), toml::Value::String(v.to_string()));
        }
        // Tokens without '=' are ignored at M0; the engine will report any
        // resulting argument-validation problems.
    }

    ReplCommand::Invoke {
        module,
        args: toml::Value::Table(table),
    }
}
