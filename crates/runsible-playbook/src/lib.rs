pub mod ast;
pub mod catalog;
pub mod engine;
pub mod errors;
pub mod modules;
pub mod output;
pub mod parse;

pub use engine::{run, RunResult};
pub use errors::{PlaybookError, Result};

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO_PLAYBOOK: &str = r#"
schema = "runsible.playbook.v1"

[imports]
debug = "runsible_builtin.debug"

[[plays]]
name = "Hello World"
hosts = "localhost"

[[plays.tasks]]
name = "Say hello"
debug = { msg = "Hello, world!" }
"#;

    #[test]
    fn parse_minimal_playbook() {
        let pb = parse::parse_playbook(HELLO_PLAYBOOK).unwrap();
        assert_eq!(pb.plays.len(), 1);
        assert_eq!(pb.plays[0].name, "Hello World");
        assert_eq!(pb.plays[0].tasks.len(), 1);
    }

    #[test]
    fn resolve_task_extracts_module() {
        let pb = parse::parse_playbook(HELLO_PLAYBOOK).unwrap();
        let raw = &pb.plays[0].tasks[0];
        let task = parse::resolve_task(raw, &pb.imports).unwrap();
        assert_eq!(task.module_name, "runsible_builtin.debug");
        assert_eq!(task.name.as_deref(), Some("Say hello"));
    }

    #[test]
    fn debug_module_plan_and_apply() {
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "localhost".into(), vars: Vars::new() };
        let module = modules::debug::DebugModule;
        let args = toml::from_str::<toml::Value>(r#"msg = "hi""#).unwrap();
        let plan = catalog::DynModule::plan(&module, &args, &host).unwrap();
        assert!(!plan.will_change);
        let outcome = catalog::DynModule::apply(&module, &plan, &host).unwrap();
        assert_eq!(outcome.returns["msg"], "hi");
    }

    #[test]
    fn run_hello_playbook() {
        let result = run(HELLO_PLAYBOOK, "localhost,", "test").unwrap();
        assert_eq!(result.ok, 1);
        assert_eq!(result.failed, 0);
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn run_multi_host() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Multi"
hosts = "all"
[[plays.tasks]]
name = "hi"
debug = { msg = "hi" }
"#;
        let result = run(src, "host1,host2,host3,", "test").unwrap();
        // 1 play × 3 hosts × 1 task = 3 ok
        assert_eq!(result.ok, 3);
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn unknown_module_errors() {
        let src = r#"
[[plays]]
name = "Bad"
hosts = "localhost"
[[plays.tasks]]
name = "boom"
no_such_module = { msg = "x" }
"#;
        let err = run(src, "localhost,", "test").unwrap_err();
        assert!(
            matches!(err, PlaybookError::ModuleNotFound(_)),
            "expected ModuleNotFound, got {err:?}"
        );
    }
}
