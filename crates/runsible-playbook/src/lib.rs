pub mod ast;
pub mod catalog;
pub mod engine;
pub mod errors;
pub mod modules;
pub mod output;
pub mod parse;
pub mod templating;

pub use engine::{run, RunResult};
pub use errors::{PlaybookError, Result};
pub use templating::Templater;

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
    fn set_fact_plan_carries_args() {
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let m = modules::set_fact::SetFactModule;
        let args = toml::from_str::<toml::Value>(r#"build_id = "abc"
env = "prod""#).unwrap();
        let plan = catalog::DynModule::plan(&m, &args, &host).unwrap();
        assert!(plan.will_change);
        assert_eq!(plan.diff["build_id"], "abc");
        assert_eq!(plan.diff["env"], "prod");
    }

    #[test]
    fn assert_plan_does_not_change() {
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let m = modules::assert_mod::AssertModule;
        let args = toml::from_str::<toml::Value>(r#"that = ["x == 1"]"#).unwrap();
        let plan = catalog::DynModule::plan(&m, &args, &host).unwrap();
        assert!(!plan.will_change);
        assert_eq!(plan.diff["that"][0], "x == 1");
    }

    #[test]
    fn catalog_has_set_fact_and_assert() {
        let cat = catalog::ModuleCatalog::with_builtins();
        assert!(cat.get("runsible_builtin.set_fact").is_some());
        assert!(cat.get("runsible_builtin.assert").is_some());
    }

    #[test]
    fn templating_renders_args_from_play_vars() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Render"
hosts = "localhost"
[plays.vars]
who = "world"
[[plays.tasks]]
name = "Greet"
debug = { msg = "Hello, {{ who }}!" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.ok, 1);
        assert_eq!(r.failed, 0);
    }

    #[test]
    fn when_false_skips_task() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Conditional"
hosts = "localhost"
[plays.vars]
gate = false
[[plays.tasks]]
name = "Should skip"
when = { expr = "gate" }
debug = { msg = "should not run" }
[[plays.tasks]]
name = "Should run"
debug = { msg = "ran" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.ok, 1);
        assert_eq!(r.skipped, 1);
        assert_eq!(r.failed, 0);
    }

    #[test]
    fn register_captures_outcome() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
assert = "runsible_builtin.assert"
[[plays]]
name = "Register"
hosts = "localhost"
[[plays.tasks]]
name = "First"
register = "first_result"
debug = { msg = "captured" }
[[plays.tasks]]
name = "Check it"
assert = { that = ["first_result.status == 'ok'"] }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0, "assert should pass when first_result.status==ok");
        assert_eq!(r.ok, 2);
    }

    #[test]
    fn set_fact_then_use_in_template() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
set_fact = "runsible_builtin.set_fact"
[[plays]]
name = "Set fact"
hosts = "localhost"
[[plays.tasks]]
name = "Set build_id"
set_fact = { build_id = "abc123" }
[[plays.tasks]]
name = "Use it"
debug = { msg = "build is {{ build_id }}" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 2);
    }

    #[test]
    fn assert_failure_is_reported() {
        let src = r#"
[imports]
assert = "runsible_builtin.assert"
[[plays]]
name = "Assert"
hosts = "localhost"
[plays.vars]
x = 5
[[plays.tasks]]
name = "Bad assertion"
assert = { that = ["x == 1"] }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 1);
        assert_eq!(r.exit_code(), 2);
    }

    #[test]
    fn tags_filter_only_runs_matching() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Tagged"
hosts = "localhost"
[[plays.tasks]]
name = "Web only"
tags = ["web"]
debug = { msg = "web" }
[[plays.tasks]]
name = "DB only"
tags = ["db"]
debug = { msg = "db" }
"#;
        let opts = engine::RunOptions {
            tags: vec!["web".into()],
            ..Default::default()
        };
        let r = engine::run_with(src, "localhost,", "test", opts).unwrap();
        assert_eq!(r.ok, 1, "only the web-tagged task should run");
    }

    #[test]
    fn handler_fires_only_when_changed() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
set_fact = "runsible_builtin.set_fact"
[[plays]]
name = "Handlers"
hosts = "localhost"
[[plays.tasks]]
name = "Notify-but-not-changed"
notify = ["restart_app"]
debug = { msg = "ok status, won't notify" }
[plays.handlers.restart_app]
debug = { msg = "handler fired" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        // debug returns Ok (not Changed), so the handler should NOT fire.
        // Result: 1 task (the debug), 0 handlers run.
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn unknown_handler_id_errors_at_parse() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Bad notify"
hosts = "localhost"
[[plays.tasks]]
name = "Notifies missing"
notify = ["does_not_exist"]
debug = { msg = "x" }
"#;
        let err = run(src, "localhost,", "test").unwrap_err();
        assert!(matches!(err, PlaybookError::TypeCheck(_)));
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
