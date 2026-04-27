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
}
