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

    fn test_ctx<'a>(host: &'a runsible_core::types::Host, vars: &'a runsible_core::types::Vars, conn: &'a runsible_connection::LocalSync) -> runsible_core::traits::ExecutionContext<'a> {
        runsible_core::traits::ExecutionContext { host, vars, connection: conn, check_mode: false }
    }

    #[test]
    fn debug_module_plan_and_apply() {
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "localhost".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = test_ctx(&host, &vars, &conn);
        let module = modules::debug::DebugModule;
        let args = toml::from_str::<toml::Value>(r#"msg = "hi""#).unwrap();
        let plan = catalog::DynModule::plan(&module, &args, &ctx).unwrap();
        assert!(!plan.will_change);
        let outcome = catalog::DynModule::apply(&module, &plan, &ctx).unwrap();
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
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = test_ctx(&host, &vars, &conn);
        let m = modules::set_fact::SetFactModule;
        let args = toml::from_str::<toml::Value>(r#"build_id = "abc"
env = "prod""#).unwrap();
        let plan = catalog::DynModule::plan(&m, &args, &ctx).unwrap();
        assert!(plan.will_change);
        assert_eq!(plan.diff["build_id"], "abc");
        assert_eq!(plan.diff["env"], "prod");
    }

    #[test]
    fn assert_plan_does_not_change() {
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = test_ctx(&host, &vars, &conn);
        let m = modules::assert_mod::AssertModule;
        let args = toml::from_str::<toml::Value>(r#"that = ["x == 1"]"#).unwrap();
        let plan = catalog::DynModule::plan(&m, &args, &ctx).unwrap();
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
    fn command_module_runs_echo() {
        let src = r#"
[imports]
command = "runsible_builtin.command"
[[plays]]
name = "Cmd"
hosts = "localhost"
[[plays.tasks]]
name = "echo"
command = { argv = ["echo", "hello"] }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.changed, 1, "command always reports Changed on rc=0");
    }

    #[test]
    fn shell_module_pipes() {
        let src = r#"
[imports]
shell = "runsible_builtin.shell"
[[plays]]
name = "Sh"
hosts = "localhost"
[[plays.tasks]]
name = "pipe"
shell = { cmd = "echo hi | tr a-z A-Z" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.changed, 1);
    }

    #[test]
    fn copy_module_creates_then_idempotent() {
        let dest = std::env::temp_dir().join(format!("rsl-copy-test-{}.txt", std::process::id()));
        let dest_str = dest.to_string_lossy().replace('\\', "\\\\");
        let _ = std::fs::remove_file(&dest);
        let src = format!(r#"
[imports]
copy = "runsible_builtin.copy"
[[plays]]
name = "Copy"
hosts = "localhost"
[[plays.tasks]]
name = "copy literal"
copy = {{ content = "hello world", dest = "{dest_str}" }}
"#);
        let r = run(&src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.changed, 1);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "hello world");
        // Second run: same content → no change.
        let r2 = run(&src, "localhost,", "test").unwrap();
        assert_eq!(r2.changed, 0);
        assert_eq!(r2.ok, 1);
        let _ = std::fs::remove_file(&dest);
    }

    #[test]
    fn file_module_creates_directory() {
        let dir = std::env::temp_dir().join(format!("rsl-file-test-{}", std::process::id()));
        let dir_str = dir.to_string_lossy().replace('\\', "\\\\");
        let _ = std::fs::remove_dir_all(&dir);
        let src = format!(r#"
[imports]
file = "runsible_builtin.file"
[[plays]]
name = "Mkdir"
hosts = "localhost"
[[plays.tasks]]
name = "ensure dir"
file = {{ path = "{dir_str}", state = "directory" }}
"#);
        let r = run(&src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.changed, 1);
        assert!(dir.exists() && dir.is_dir());
        // Second run: dir exists → no change.
        let r2 = run(&src, "localhost,", "test").unwrap();
        assert_eq!(r2.changed, 0);
        assert_eq!(r2.ok, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn template_module_renders_and_writes() {
        let src_path = std::env::temp_dir().join(format!("rsl-tpl-src-{}.j2", std::process::id()));
        let dest_path = std::env::temp_dir().join(format!("rsl-tpl-dst-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&src_path);
        let _ = std::fs::remove_file(&dest_path);
        std::fs::write(&src_path, "Hello, {{ name }}! You are {{ age }}.\n").unwrap();

        let src_str = src_path.to_string_lossy().replace('\\', "\\\\");
        let dest_str = dest_path.to_string_lossy().replace('\\', "\\\\");
        let pb = format!(r#"
[imports]
template = "runsible_builtin.template"
[[plays]]
name = "Tpl"
hosts = "localhost"
[plays.vars]
name = "Alice"
age = 30
[[plays.tasks]]
name = "render"
template = {{ src = "{src_str}", dest = "{dest_str}" }}
"#);
        let r = run(&pb, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.changed, 1);
        let written = std::fs::read_to_string(&dest_path).unwrap();
        assert_eq!(written, "Hello, Alice! You are 30.\n");

        // Second run: same content → no change.
        let r2 = run(&pb, "localhost,", "test").unwrap();
        assert_eq!(r2.changed, 0);
        assert_eq!(r2.ok, 1);

        let _ = std::fs::remove_file(&src_path);
        let _ = std::fs::remove_file(&dest_path);
    }

    #[test]
    fn catalog_has_new_m1_modules() {
        let cat = catalog::ModuleCatalog::with_builtins();
        assert!(cat.get("runsible_builtin.package").is_some());
        assert!(cat.get("runsible_builtin.service").is_some());
        assert!(cat.get("runsible_builtin.systemd_service").is_some());
        assert!(cat.get("runsible_builtin.get_url").is_some());
    }

    #[test]
    fn package_plan_requires_name() {
        use runsible_core::types::{Host, Vars};
        use runsible_core::traits::ExecutionContext;
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = ExecutionContext { host: &host, vars: &vars, connection: &conn, check_mode: false };
        let m = modules::package::PackageModule;
        let args = toml::from_str::<toml::Value>(r#"state = "present""#).unwrap();
        let r = catalog::DynModule::plan(&m, &args, &ctx);
        assert!(r.is_err(), "package without name should error");
    }

    #[test]
    fn service_plan_requires_name() {
        use runsible_core::types::{Host, Vars};
        use runsible_core::traits::ExecutionContext;
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = ExecutionContext { host: &host, vars: &vars, connection: &conn, check_mode: false };
        let m = modules::service::ServiceModule;
        let args = toml::from_str::<toml::Value>(r#"state = "started""#).unwrap();
        let r = catalog::DynModule::plan(&m, &args, &ctx);
        assert!(r.is_err(), "service without name should error");
    }

    #[test]
    fn get_url_plan_requires_url_and_dest() {
        use runsible_core::types::{Host, Vars};
        use runsible_core::traits::ExecutionContext;
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = ExecutionContext { host: &host, vars: &vars, connection: &conn, check_mode: false };
        let m = modules::get_url::GetUrlModule;
        let args = toml::from_str::<toml::Value>(r#"url = "https://example.com""#).unwrap();
        let r = catalog::DynModule::plan(&m, &args, &ctx);
        assert!(r.is_err(), "get_url without dest should error");
    }

    #[test]
    fn loop_runs_task_per_item() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Loop"
hosts = "localhost"
[[plays.tasks]]
name = "iterate"
loop = ["a", "b", "c"]
debug = { msg = "item is {{ item }}" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        // Loop adds one task outcome per item.
        assert_eq!(r.ok, 3);
    }

    #[test]
    fn loop_with_loop_var_renames_binding() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "LoopVar"
hosts = "localhost"
[[plays.tasks]]
name = "iterate"
loop = ["x", "y"]
loop_control = { loop_var = "thing" }
debug = { msg = "thing is {{ thing }}" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 2);
    }

    #[test]
    fn until_succeeds_first_attempt() {
        let src = r#"
[imports]
set_fact = "runsible_builtin.set_fact"
[[plays]]
name = "Until"
hosts = "localhost"
[[plays.tasks]]
name = "trivially-true"
register = "r"
until = { expr = "r.status == 'ok'" }
retries = 3
delay_seconds = 0
set_fact = { dummy = "x" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        // set_fact reports Ok; until is true on first attempt.
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn block_runs_all_children_when_clean() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Block"
hosts = "localhost"
[[plays.tasks]]
name = "Wrap"
[[plays.tasks.block]]
name = "first"
debug = { msg = "1" }
[[plays.tasks.block]]
name = "second"
debug = { msg = "2" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        // Two child debugs run; both Ok.
        assert_eq!(r.ok, 2);
    }

    #[test]
    fn rescue_runs_only_when_block_fails() {
        let src = r#"
[imports]
assert = "runsible_builtin.assert"
debug = "runsible_builtin.debug"
[[plays]]
name = "Rescue"
hosts = "localhost"
[[plays.tasks]]
name = "Wrap"
[[plays.tasks.block]]
name = "fails"
assert = { that = ["1 == 2"] }
[[plays.tasks.rescue]]
name = "rescue runs"
debug = { msg = "recovered" }
[[plays.tasks.always]]
name = "always runs"
debug = { msg = "cleanup" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        // 1 Failed (the assert) + 1 Ok (rescue debug) + 1 Ok (always debug)
        assert_eq!(r.failed, 1);
        assert_eq!(r.ok, 2);
    }

    #[test]
    fn always_runs_even_on_clean_block() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Always"
hosts = "localhost"
[[plays.tasks]]
name = "Wrap"
[[plays.tasks.block]]
name = "ok"
debug = { msg = "ok" }
[[plays.tasks.always]]
name = "always"
debug = { msg = "always" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 2);
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
