//! runsible-console — interactive REPL frontend for the runsible engine.
//!
//! M0 surface: a `rustyline`-backed loop that parses `<module> [k=v ...]`
//! lines and runs each as a one-task synthetic playbook against a single
//! startup-supplied target.

pub mod errors;
pub mod parse;
pub mod repl;

pub use errors::{ConsoleError, Result};
pub use parse::{parse_line, ReplCommand};
pub use repl::run_repl;

// ---------------------------------------------------------------------------
// TRIPLE SIMS gate
// ---------------------------------------------------------------------------

/// Smoke gate: parse every grammar form (Quit, Empty, Comment, Invoke with
/// args, case-insensitive Quit) through `parse_line`, verifying each variant
/// matches the expected `ReplCommand` and the args round-trip correctly.
/// Returns 0 on success.
pub fn f30() -> i32 {
    // ── Stage 1: "quit" → Quit ──────────────────────────────────────────────
    if !matches!(parse_line("quit"), ReplCommand::Quit) {
        return 1;
    }

    // ── Stage 2: "" → Empty ─────────────────────────────────────────────────
    if !matches!(parse_line(""), ReplCommand::Empty) {
        return 2;
    }

    // ── Stage 3: "# comment" → Comment ──────────────────────────────────────
    if !matches!(parse_line("# comment"), ReplCommand::Comment) {
        return 3;
    }

    // ── Stage 4: "debug msg=hello" → Invoke { module: "debug", args.msg=="hello" }
    match parse_line("debug msg=hello") {
        ReplCommand::Invoke { module, args } => {
            if module != "debug" {
                return 4;
            }
            match args.get("msg").and_then(|v| v.as_str()) {
                Some("hello") => {}
                _ => return 5,
            }
        }
        _ => return 6,
    }

    // ── Stage 5: "EXIT" (case-insensitive) → Quit ──────────────────────────
    if !matches!(parse_line("EXIT"), ReplCommand::Quit) {
        return 7;
    }

    // ── Stage 6: ConsoleCompleter pulls module names from the catalog and
    // completes a partial prefix. ──────────────────────────────────────────
    let completer = repl::ConsoleCompleter::from_builtins();
    let (start, candidates) = completer.complete_word("deb", 3);
    if start != 0 {
        return 8;
    }
    if !candidates.iter().any(|c| c == "debug") {
        return 9;
    }

    // Empty prefix returns at least the catalog's known short alias for ping.
    let (_, all) = completer.complete_word("", 0);
    if !all.iter().any(|c| c == "ping" || c == "runsible_builtin.ping") {
        return 10;
    }

    // Unknown prefix returns no candidates.
    let (_, none) = completer.complete_word("xyz_no_such_prefix_zzz", 22);
    if !none.is_empty() {
        return 11;
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quit() {
        assert!(matches!(parse_line("quit"), ReplCommand::Quit));
        assert!(matches!(parse_line("exit"), ReplCommand::Quit));
        assert!(matches!(parse_line("EXIT"), ReplCommand::Quit));
        assert!(matches!(parse_line(" quit "), ReplCommand::Quit));
        assert!(matches!(parse_line("Quit"), ReplCommand::Quit));
    }

    #[test]
    fn parse_empty() {
        assert!(matches!(parse_line(""), ReplCommand::Empty));
        assert!(matches!(parse_line("   "), ReplCommand::Empty));
        assert!(matches!(parse_line("\t\t"), ReplCommand::Empty));
    }

    #[test]
    fn parse_comment() {
        assert!(matches!(parse_line("# hello"), ReplCommand::Comment));
        assert!(matches!(parse_line("#no space"), ReplCommand::Comment));
        assert!(matches!(parse_line("   # leading whitespace"), ReplCommand::Comment));
    }

    #[test]
    fn parse_invoke_no_args() {
        match parse_line("runsible_builtin.ping") {
            ReplCommand::Invoke { module, args } => {
                assert_eq!(module, "runsible_builtin.ping");
                let table = args.as_table().expect("args must be a table");
                assert!(table.is_empty(), "expected empty args, got {table:?}");
            }
            other => panic!("expected Invoke, got {other:?}"),
        }
    }

    #[test]
    fn parse_invoke_kv_args() {
        match parse_line("runsible_builtin.debug msg=hello") {
            ReplCommand::Invoke { module, args } => {
                assert_eq!(module, "runsible_builtin.debug");
                assert_eq!(
                    args.get("msg").and_then(|v| v.as_str()),
                    Some("hello")
                );
            }
            other => panic!("expected Invoke, got {other:?}"),
        }
    }

    #[test]
    fn parse_invoke_multi_kv() {
        match parse_line("debug msg=hi var=x") {
            ReplCommand::Invoke { module, args } => {
                assert_eq!(module, "debug");
                assert_eq!(args.get("msg").and_then(|v| v.as_str()), Some("hi"));
                assert_eq!(args.get("var").and_then(|v| v.as_str()), Some("x"));
            }
            other => panic!("expected Invoke, got {other:?}"),
        }
    }

    #[test]
    fn parse_unknown() {
        // "Unknown" inputs that aren't quit/exit/empty/comment fall through
        // as Invoke calls with the first token as the module name; positional
        // tokens without '=' are silently dropped.
        match parse_line("garbage with spaces") {
            ReplCommand::Invoke { module, args } => {
                assert_eq!(module, "garbage");
                let table = args.as_table().expect("args must be a table");
                assert!(table.is_empty(), "non-kv tokens should be skipped");
            }
            other => panic!("expected Invoke for non-quit input, got {other:?}"),
        }
    }

    /// `:quit` is NOT recognized — colon-prefixed REPL commands are a future
    /// feature and currently fall through to `Invoke { module: ":quit" }`.
    /// Lock that behavior in so the day we add real REPL meta-commands we
    /// notice this test breaking.
    #[test]
    fn parse_colon_quit_is_unrecognized_invoke() {
        match parse_line(":quit") {
            ReplCommand::Invoke { module, args } => {
                assert_eq!(module, ":quit");
                let table = args.as_table().expect("args must be a table");
                assert!(table.is_empty());
            }
            other => panic!("expected Invoke (current M0 behavior), got {other:?}"),
        }
    }

    /// Bare module name with no args produces an Invoke whose args table is
    /// empty.
    #[test]
    fn parse_alias_only_invoke_has_empty_args() {
        match parse_line("debug") {
            ReplCommand::Invoke { module, args } => {
                assert_eq!(module, "debug");
                let table = args.as_table().expect("args must be a table");
                assert!(table.is_empty(), "expected no args; got {table:?}");
            }
            other => panic!("expected Invoke, got {other:?}"),
        }
    }

    /// Quoted values containing whitespace are NOT supported by the M0
    /// tokenizer (it splits on whitespace before key=value parsing). Lock
    /// that limitation in so callers know what to expect.
    #[test]
    fn arg_with_spaces_not_supported_yet() {
        match parse_line("debug msg=\"hello world\"") {
            ReplCommand::Invoke { module, args } => {
                assert_eq!(module, "debug");
                // Whitespace splits the input — `msg` gets `"hello` as its
                // value (with a stray opening quote) and the trailing
                // `world"` token is dropped because it has no `=`.
                let v = args.get("msg").and_then(|v| v.as_str()).unwrap_or("");
                assert!(
                    v.contains("hello") && v.contains('"'),
                    "expected the partial-quote value to land in `msg`; got: {v:?}"
                );
                assert!(
                    !v.contains("world"),
                    "M0 tokenizer must NOT join whitespace-separated quoted args; got: {v:?}"
                );
            }
            other => panic!("expected Invoke, got {other:?}"),
        }
    }

    /// Repeated keys: last value wins (the toml::Map insert overwrites).
    #[test]
    fn parse_duplicate_keys_last_wins() {
        match parse_line("debug msg=hello msg=overwritten") {
            ReplCommand::Invoke { module, args } => {
                assert_eq!(module, "debug");
                assert_eq!(
                    args.get("msg").and_then(|v| v.as_str()),
                    Some("overwritten"),
                    "duplicate keys: last value must win"
                );
            }
            other => panic!("expected Invoke, got {other:?}"),
        }
    }

    /// Malformed `=value` token (empty key) — the parser inserts an empty
    /// key into the args table. Lock that behavior in.
    #[test]
    fn parse_malformed_token_empty_key_inserted() {
        match parse_line("debug =bare") {
            ReplCommand::Invoke { module, args } => {
                assert_eq!(module, "debug");
                let table = args.as_table().expect("args must be a table");
                // M0 split_once('=') on "=bare" returns ("", "bare"), so we
                // get an empty-string key. This is a known quirk; lock it in.
                assert_eq!(
                    table.get("").and_then(|v| v.as_str()),
                    Some("bare"),
                    "M0 inserts empty-key tokens; got: {table:?}"
                );
            }
            other => panic!("expected Invoke, got {other:?}"),
        }
    }

    /// Comment lines are detected even when they mention reserved words.
    #[test]
    fn parse_comment_with_reserved_word_in_body() {
        assert!(matches!(
            parse_line("# this is a comment with debug in it"),
            ReplCommand::Comment
        ));
        // Trailing whitespace before `#` is also a comment.
        assert!(matches!(
            parse_line("   # padded"),
            ReplCommand::Comment
        ));
    }

    /// Mixed tabs + spaces: still Empty.
    #[test]
    fn parse_tabs_and_spaces_only_is_empty() {
        assert!(matches!(parse_line("\t  "), ReplCommand::Empty));
        assert!(matches!(parse_line(" \t \t "), ReplCommand::Empty));
    }

    /// Uppercase EXIT must be recognized — quit/exit are case-insensitive.
    #[test]
    fn parse_uppercase_exit_is_quit() {
        assert!(matches!(parse_line("EXIT"), ReplCommand::Quit));
        assert!(matches!(parse_line("ExIt"), ReplCommand::Quit));
        assert!(matches!(parse_line("QUIT"), ReplCommand::Quit));
    }
}
