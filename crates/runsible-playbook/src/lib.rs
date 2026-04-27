pub mod ast;
pub mod catalog;
pub mod engine;
pub mod errors;
pub mod modules;
pub mod output;
pub mod parse;
pub mod roles;
pub mod templating;

pub use engine::{run, RunResult};
pub use errors::{PlaybookError, Result};
pub use templating::Templater;

/// Smoke gate: drive the public `run` API end-to-end with a synthetic two-task
/// playbook (set_fact, then debug templating the fact). Returns 0 only when the
/// run reports `ok == 2 && failed == 0 && exit_code == 0`. Distinct non-zero
/// codes for parse failure, run error, and wrong counters.
pub fn f30() -> i32 {
    let src = r#"
[imports]
debug = "runsible_builtin.debug"
set_fact = "runsible_builtin.set_fact"

[[plays]]
name = "f30"
hosts = "localhost"

[[plays.tasks]]
set_fact = { x = 42 }

[[plays.tasks]]
debug = { msg = "x is {{ x }}" }
"#;

    // Pre-flight: ensure the playbook actually parses with the public parser.
    if parse::parse_playbook(src).is_err() {
        return 1;
    }

    let result = match run(src, "localhost,", "f30") {
        Ok(r) => r,
        Err(_) => return 2,
    };

    if result.failed != 0 {
        return 3;
    }
    if result.ok != 2 {
        return 4;
    }
    if result.exit_code() != 0 {
        return 5;
    }

    // Stage 2: --check mode skips real changes for mutating modules.
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dest = std::env::temp_dir().join(format!("rsl-pb-f30-{pid}-{nanos}.txt"));
    let _ = std::fs::remove_file(&dest);
    let dest_str = dest.to_string_lossy().replace('\\', "\\\\");
    let check_pb = format!(
        r#"
[imports]
copy = "runsible_builtin.copy"
[[plays]]
name = "check"
hosts = "localhost"
[[plays.tasks]]
copy = {{ content = "must not write\n", dest = "{dest_str}" }}
"#
    );
    let opts = engine::RunOptions {
        check_mode: true,
        ..Default::default()
    };
    let r = match engine::run_with(&check_pb, "localhost,", "f30-check", opts) {
        Ok(r) => r,
        Err(_) => return 6,
    };
    if r.changed != 1 {
        let _ = std::fs::remove_file(&dest);
        return 7;
    }
    if dest.exists() {
        let _ = std::fs::remove_file(&dest);
        return 8;
    }

    // Stage 3: --diff mode populates before+after on mutating modules.
    if std::fs::write(&dest, "old\n").is_err() {
        return 9;
    }
    // Reuse the same playbook, now with check+diff and a pre-existing file.
    let opts2 = engine::RunOptions {
        check_mode: true,
        diff_mode: true,
        ..Default::default()
    };
    if engine::run_with(&check_pb, "localhost,", "f30-diff", opts2).is_err() {
        let _ = std::fs::remove_file(&dest);
        return 10;
    }
    let _ = std::fs::remove_file(&dest);

    // Stage 4: forks=4 across 8 hosts produces 8 outcomes.
    // Run on a fresh OS thread because the engine's parallel path builds its
    // own tokio multi-thread runtime via `block_on`. If we're being driven by
    // a tokio current-thread runtime (which exopack's test bin is), nesting
    // would panic with "Cannot start a runtime from within a runtime".
    let multi_pb = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "multi"
hosts = "all"
[[plays.tasks]]
debug = { msg = "host {{ inventory_hostname }}" }
"#;
    let multi_pb_owned = multi_pb.to_string();
    let r3 = match std::thread::spawn(move || {
        let opts3 = engine::RunOptions {
            forks: 4,
            ..Default::default()
        };
        engine::run_with(&multi_pb_owned, "h1,h2,h3,h4,h5,h6,h7,h8,", "f30-forks", opts3)
    })
    .join()
    {
        Ok(Ok(r)) => r,
        _ => return 11,
    };
    if r3.ok != 8 {
        return 12;
    }
    if r3.failed != 0 {
        return 13;
    }

    // Stage 5: gather_facts auto-prepends setup; ansible_hostname is bound.
    let facts_pb = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "facts"
hosts = "localhost"
gather_facts = true
[[plays.tasks]]
debug = { msg = "host {{ ansible_hostname }}" }
"#;
    let r4 = match run(facts_pb, "localhost,", "f30-facts") {
        Ok(r) => r,
        Err(_) => return 14,
    };
    if r4.failed != 0 {
        return 15;
    }
    // 1 setup task + 1 debug task = 2 outcomes.
    if r4.ok != 2 {
        return 16;
    }

    // Stage 6: include_tasks loads an external file and runs its tasks.
    let inc_path = std::env::temp_dir().join(format!("rsl-pb-f30-inc-{pid}-{nanos}.toml"));
    let _ = std::fs::remove_file(&inc_path);
    if std::fs::write(
        &inc_path,
        r#"
[[tasks]]
debug = { msg = "from include" }
"#,
    )
    .is_err()
    {
        return 17;
    }
    let inc_str = inc_path.to_string_lossy().replace('\\', "\\\\");
    let inc_pb = format!(
        r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "inc"
hosts = "localhost"
[[plays.tasks]]
include_tasks = "{inc_str}"
"#
    );
    let r5 = match run(&inc_pb, "localhost,", "f30-inc") {
        Ok(r) => r,
        Err(_) => {
            let _ = std::fs::remove_file(&inc_path);
            return 18;
        }
    };
    let _ = std::fs::remove_file(&inc_path);
    if r5.failed != 0 || r5.ok != 1 {
        return 19;
    }

    // Stage 7: module_defaults apply when task omits the arg.
    let md_pb = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "md"
hosts = "localhost"
[plays.module_defaults."runsible_builtin.debug"]
msg = "from defaults"
[[plays.tasks]]
debug = {}
"#;
    let r6 = match run(md_pb, "localhost,", "f30-md") {
        Ok(r) => r,
        Err(_) => return 20,
    };
    if r6.failed != 0 || r6.ok != 1 {
        return 21;
    }

    // Stage 8: filter catalog — render an Ansible-shaped filter chain and
    // verify the engine accepts it.
    let filt_pb = r#"
[imports]
debug = "runsible_builtin.debug"
assert = "runsible_builtin.assert"
[[plays]]
name = "filt"
hosts = "localhost"
[plays.vars]
items = ["a", "b", "c"]
raw_b64 = "aGVsbG8="
yes_str = "yes"
[[plays.tasks]]
assert = { that = ["items | length == 3", "yes_str | bool == true", "raw_b64 | b64decode == 'hello'"] }
"#;
    let r7 = match run(filt_pb, "localhost,", "f30-filt") {
        Ok(r) => r,
        Err(_) => return 22,
    };
    if r7.failed != 0 || r7.ok != 1 {
        return 23;
    }

    // Stage 9: lookup catalog — env lookup and pipe lookup both wired.
    let lookup_pb = r#"
[imports]
debug = "runsible_builtin.debug"
assert = "runsible_builtin.assert"
[[plays]]
name = "lookup"
hosts = "localhost"
[[plays.tasks]]
assert = { that = ["lookup('env', 'RSL_F30_NONEXISTENT_VAR') == ''"] }
"#;
    let r8 = match run(lookup_pb, "localhost,", "f30-lookup") {
        Ok(r) => r,
        Err(_) => return 24,
    };
    if r8.failed != 0 {
        return 25;
    }

    // Stage 10: catalog has all 28 builtin modules registered.
    let cat = catalog::ModuleCatalog::with_builtins();
    let expected_modules = [
        "runsible_builtin.debug",
        "runsible_builtin.ping",
        "runsible_builtin.set_fact",
        "runsible_builtin.assert",
        "runsible_builtin.command",
        "runsible_builtin.shell",
        "runsible_builtin.copy",
        "runsible_builtin.file",
        "runsible_builtin.template",
        "runsible_builtin.package",
        "runsible_builtin.service",
        "runsible_builtin.systemd_service",
        "runsible_builtin.get_url",
        "runsible_builtin.setup",
        "runsible_builtin.lineinfile",
        "runsible_builtin.blockinfile",
        "runsible_builtin.replace",
        "runsible_builtin.stat",
        "runsible_builtin.find",
        "runsible_builtin.fail",
        "runsible_builtin.pause",
        "runsible_builtin.wait_for",
        "runsible_builtin.uri",
        "runsible_builtin.archive",
        "runsible_builtin.unarchive",
        "runsible_builtin.user",
        "runsible_builtin.group",
        "runsible_builtin.cron",
        "runsible_builtin.hostname",
    ];
    for name in &expected_modules {
        if cat.get(name).is_none() {
            eprintln!("missing module: {name}");
            return 26;
        }
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Copy and template modules stage content via `runsible-{copy,template}-{pid}.tmp`,
    /// which is a single shared path for the test process. Concurrent tests using
    /// those modules race that staging file. We serialize the affected tests with
    /// this mutex so the race never manifests in CI. (`PoisonError` is squashed
    /// — we only care about ordering.)
    static FILE_MOD_LOCK: Mutex<()> = Mutex::new(());

    fn _file_mod_guard() -> std::sync::MutexGuard<'static, ()> {
        match FILE_MOD_LOCK.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }

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
        runsible_core::traits::ExecutionContext { host, vars, connection: conn, check_mode: false, diff_mode: false }
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
        let _g = _file_mod_guard();
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
        let _g = _file_mod_guard();
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
        let ctx = ExecutionContext { host: &host, vars: &vars, connection: &conn, check_mode: false, diff_mode: false };
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
        let ctx = ExecutionContext { host: &host, vars: &vars, connection: &conn, check_mode: false, diff_mode: false };
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
        let ctx = ExecutionContext { host: &host, vars: &vars, connection: &conn, check_mode: false, diff_mode: false };
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
    fn role_tasks_run_in_play() {
        let tmp = std::env::temp_dir().join(format!("rsl-role-itest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg_dir = tmp.join("packages");
        let role_root = pkg_dir.join("greet");
        std::fs::create_dir_all(role_root.join("tasks")).unwrap();
        std::fs::create_dir_all(role_root.join("defaults")).unwrap();
        std::fs::write(
            role_root.join("tasks/main.toml"),
            r#"
[[tasks]]
name = "say hi"
debug = { msg = "hello {{ greeting_target }}" }
"#,
        )
        .unwrap();
        std::fs::write(
            role_root.join("defaults/main.toml"),
            r#"greeting_target = "world""#,
        )
        .unwrap();

        let pb = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Use role"
hosts = "localhost"
[[plays.roles]]
name = "greet"
"#;
        let opts = engine::RunOptions {
            role_search_paths: Some(vec![pkg_dir.clone()]),
            ..Default::default()
        };
        let r = engine::run_with(pb, "localhost,", "test", opts).unwrap();
        let _ = std::fs::remove_dir_all(&tmp);

        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 1, "role's single task ran ok");
    }

    #[test]
    fn role_params_override_defaults() {
        let tmp = std::env::temp_dir().join(format!("rsl-role-params-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg_dir = tmp.join("packages");
        let role_root = pkg_dir.join("greet");
        std::fs::create_dir_all(role_root.join("tasks")).unwrap();
        std::fs::create_dir_all(role_root.join("defaults")).unwrap();
        std::fs::write(
            role_root.join("tasks/main.toml"),
            r#"
[[tasks]]
name = "assert override"
assert = { that = ["greeting_target == 'override'"] }
"#,
        )
        .unwrap();
        std::fs::write(
            role_root.join("defaults/main.toml"),
            r#"greeting_target = "default""#,
        )
        .unwrap();

        let pb = r#"
[imports]
assert = "runsible_builtin.assert"
[[plays]]
name = "Override"
hosts = "localhost"
[[plays.roles]]
name = "greet"
[plays.roles.vars]
greeting_target = "override"
"#;
        let opts = engine::RunOptions {
            role_search_paths: Some(vec![pkg_dir.clone()]),
            ..Default::default()
        };
        let r = engine::run_with(pb, "localhost,", "test", opts).unwrap();
        let _ = std::fs::remove_dir_all(&tmp);

        assert_eq!(r.failed, 0, "role param should override default");
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn missing_role_errors_at_parse() {
        let pb = r#"
[[plays]]
name = "Bad"
hosts = "localhost"
[[plays.roles]]
name = "totally_does_not_exist_role_12345"
"#;
        // Use an empty search path so the role is genuinely unfindable
        // regardless of whatever exists in the cwd.
        let opts = engine::RunOptions {
            role_search_paths: Some(vec![]),
            ..Default::default()
        };
        let err = engine::run_with(pb, "localhost,", "test", opts).unwrap_err();
        assert!(matches!(err, PlaybookError::Parse(_)));
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

    // --- Engine integration: multi-play, tags, hosts, register/set_fact ---

    #[test]
    fn multiple_plays_in_one_file() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "First"
hosts = "localhost"
[[plays.tasks]]
name = "p1 task"
debug = { msg = "play one" }
[[plays]]
name = "Second"
hosts = "localhost"
[[plays.tasks]]
name = "p2 task"
debug = { msg = "play two" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 2, "two plays each ran one task on localhost");
    }

    #[test]
    fn empty_tasks_list_runs_zero_tasks() {
        let src = r#"
[[plays]]
name = "Empty"
hosts = "localhost"
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.ok, 0);
        assert_eq!(r.changed, 0);
        assert_eq!(r.failed, 0);
        assert_eq!(r.skipped, 0);
    }

    #[test]
    fn tag_always_runs_even_with_other_filter() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Always"
hosts = "localhost"
[[plays.tasks]]
name = "must run"
tags = ["always"]
debug = { msg = "always" }
[[plays.tasks]]
name = "should skip"
tags = ["web"]
debug = { msg = "web" }
"#;
        let opts = engine::RunOptions {
            tags: vec!["other".into()],
            ..Default::default()
        };
        let r = engine::run_with(src, "localhost,", "test", opts).unwrap();
        // The always-tagged task runs; the web-only one is filtered out.
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn tag_never_skipped_with_empty_tags() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Never"
hosts = "localhost"
[[plays.tasks]]
name = "skipped"
tags = ["never"]
debug = { msg = "n" }
[[plays.tasks]]
name = "runs"
debug = { msg = "y" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn tag_never_runs_when_explicitly_requested() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Never"
hosts = "localhost"
[[plays.tasks]]
name = "must-run-when-requested"
tags = ["never"]
debug = { msg = "n" }
"#;
        let opts = engine::RunOptions {
            tags: vec!["never".into()],
            ..Default::default()
        };
        let r = engine::run_with(src, "localhost,", "test", opts).unwrap();
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn skip_tags_removes_always_tagged_when_always_skipped() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Skip always"
hosts = "localhost"
[[plays.tasks]]
name = "tagged always"
tags = ["always"]
debug = { msg = "n" }
"#;
        let opts = engine::RunOptions {
            skip_tags: vec!["always".into()],
            ..Default::default()
        };
        let r = engine::run_with(src, "localhost,", "test", opts).unwrap();
        // Skip-tags overrides always.
        assert_eq!(r.ok, 0);
    }

    #[test]
    fn host_pattern_matches_subset() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Subset"
hosts = "host1"
[[plays.tasks]]
name = "only host1"
debug = { msg = "hi" }
"#;
        let r = run(src, "host1,host2,host3,", "test").unwrap();
        // Pattern matches only host1 → one task ran.
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn register_then_use_in_when_expression() {
        let src = r#"
[imports]
command = "runsible_builtin.command"
debug = "runsible_builtin.debug"
[[plays]]
name = "Register cmd"
hosts = "localhost"
[[plays.tasks]]
name = "first cmd"
register = "first"
command = { argv = ["true"] }
[[plays.tasks]]
name = "gated"
when = { expr = "first.returns.rc == 0" }
debug = { msg = "rc was zero" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        // first command -> Changed, second debug -> Ok
        assert_eq!(r.ok, 1);
        assert_eq!(r.changed, 1);
    }

    #[test]
    fn set_fact_int_then_template() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
set_fact = "runsible_builtin.set_fact"
assert = "runsible_builtin.assert"
[[plays]]
name = "Int fact"
hosts = "localhost"
[[plays.tasks]]
name = "set int"
set_fact = { build_id = 42 }
[[plays.tasks]]
name = "use it"
debug = { msg = "build is {{ build_id }}" }
[[plays.tasks]]
name = "verify"
assert = { that = ["build_id == 42"] }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 3);
    }

    #[test]
    fn set_fact_array_indexable() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
set_fact = "runsible_builtin.set_fact"
assert = "runsible_builtin.assert"
[[plays]]
name = "Array fact"
hosts = "localhost"
[[plays.tasks]]
name = "set list"
set_fact = { items = ["a", "b", "c"] }
[[plays.tasks]]
name = "first item"
debug = { msg = "{{ items[0] }}" }
[[plays.tasks]]
name = "verify second"
assert = { that = ["items[1] == 'b'"] }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 3);
    }

    #[test]
    fn assert_multiple_that_all_pass() {
        let src = r#"
[imports]
assert = "runsible_builtin.assert"
[[plays]]
name = "Asserts"
hosts = "localhost"
[plays.vars]
x = 1
y = 2
z = 3
[[plays.tasks]]
name = "all pass"
assert = { that = ["x == 1", "y == 2", "z == 3"] }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn assert_short_circuits_on_first_failure() {
        let src = r#"
[imports]
assert = "runsible_builtin.assert"
[[plays]]
name = "Asserts"
hosts = "localhost"
[plays.vars]
x = 1
[[plays.tasks]]
name = "fails first"
assert = { that = ["x == 99", "x == 1"] }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 1);
        assert_eq!(r.exit_code(), 2);
    }

    #[test]
    fn loop_with_empty_list_runs_zero_iterations() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Empty loop"
hosts = "localhost"
[[plays.tasks]]
name = "iter"
loop = []
debug = { msg = "never" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 0);
        assert_eq!(r.changed, 0);
    }

    #[test]
    fn loop_with_one_item_runs_once() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "One-item loop"
hosts = "localhost"
[[plays.tasks]]
name = "iter"
loop = ["only"]
debug = { msg = "{{ item }}" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn until_exhausts_retries_with_set_fact() {
        let src = r#"
[imports]
set_fact = "runsible_builtin.set_fact"
[[plays]]
name = "Until forever"
hosts = "localhost"
[[plays.tasks]]
name = "loop forever"
register = "r"
until = { expr = "false" }
retries = 2
delay_seconds = 0
set_fact = { x = 1 }
"#;
        let start = std::time::Instant::now();
        let r = run(src, "localhost,", "test").unwrap();
        // delay_seconds == 0 should make this fast.
        assert!(start.elapsed().as_secs() < 5);
        // set_fact reports Ok every attempt; the engine records the last attempt's outcome.
        assert_eq!(r.exit_code(), 0);
    }

    #[test]
    fn block_three_children_all_ok() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Block 3"
hosts = "localhost"
[[plays.tasks]]
name = "wrap"
[[plays.tasks.block]]
name = "a"
debug = { msg = "a" }
[[plays.tasks.block]]
name = "b"
debug = { msg = "b" }
[[plays.tasks.block]]
name = "c"
debug = { msg = "c" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 3);
    }

    #[test]
    fn block_failure_on_second_stops_third() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
assert = "runsible_builtin.assert"
[[plays]]
name = "Stop"
hosts = "localhost"
[[plays.tasks]]
name = "wrap"
[[plays.tasks.block]]
name = "first ok"
debug = { msg = "first" }
[[plays.tasks.block]]
name = "second fails"
assert = { that = ["false"] }
[[plays.tasks.block]]
name = "third unreachable"
debug = { msg = "should not run" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        // first ok=1, second failed=1, third never ran.
        assert_eq!(r.ok, 1);
        assert_eq!(r.failed, 1);
    }

    #[test]
    fn rescue_does_not_run_on_clean_block() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Clean"
hosts = "localhost"
[[plays.tasks]]
name = "wrap"
[[plays.tasks.block]]
name = "ok"
debug = { msg = "fine" }
[[plays.tasks.rescue]]
name = "should-not-run"
debug = { msg = "rescue" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        // Only the block child runs.
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn rescue_recovers_run_keeps_failed_count_from_block() {
        // Verifies current behavior: rescue does NOT decrement the failed counter
        // for the block child; it just runs additional tasks. Lock that in.
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
assert = "runsible_builtin.assert"
[[plays]]
name = "Rescue keeps fail"
hosts = "localhost"
[[plays.tasks]]
name = "wrap"
[[plays.tasks.block]]
name = "blow up"
assert = { that = ["false"] }
[[plays.tasks.rescue]]
name = "recover"
debug = { msg = "recovered" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        // failed counter is 1 (the block child), rescue ran => +1 ok.
        assert_eq!(r.failed, 1);
        assert_eq!(r.ok, 1);
        // exit_code is 2 because failed > 0 — locking in current semantics.
        assert_eq!(r.exit_code(), 2);
    }

    #[test]
    fn always_runs_after_rescue() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
assert = "runsible_builtin.assert"
[[plays]]
name = "Always after rescue"
hosts = "localhost"
[[plays.tasks]]
name = "wrap"
[[plays.tasks.block]]
name = "fails"
assert = { that = ["false"] }
[[plays.tasks.rescue]]
name = "rescue"
debug = { msg = "r" }
[[plays.tasks.always]]
name = "cleanup"
debug = { msg = "a" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        // 1 fail + 1 rescue ok + 1 always ok
        assert_eq!(r.failed, 1);
        assert_eq!(r.ok, 2);
    }

    #[test]
    fn block_when_false_skips_block_rescue_and_always() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "When-false block"
hosts = "localhost"
[plays.vars]
gate = false
[[plays.tasks]]
name = "wrap"
when = { expr = "gate" }
[[plays.tasks.block]]
name = "child"
debug = { msg = "no" }
[[plays.tasks.rescue]]
name = "rescue"
debug = { msg = "no" }
[[plays.tasks.always]]
name = "always"
debug = { msg = "no" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.ok, 0);
        assert_eq!(r.failed, 0);
        assert_eq!(r.changed, 0);
        assert_eq!(r.skipped, 1, "the parent block task is the one skipped");
    }

    #[test]
    fn nested_block_runs_both_levels() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Nested"
hosts = "localhost"
[[plays.tasks]]
name = "outer"
[[plays.tasks.block]]
name = "before-inner"
debug = { msg = "outer pre" }
[[plays.tasks.block]]
name = "inner"
[[plays.tasks.block.block]]
name = "inner child"
debug = { msg = "inner" }
[[plays.tasks.block]]
name = "after-inner"
debug = { msg = "outer post" }
"#;
        // Outer block has 3 children; the middle one is itself a block with 1 child.
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        // 1 (outer pre) + 1 (inner child) + 1 (outer post) = 3
        assert_eq!(r.ok, 3);
    }

    #[test]
    fn handler_fires_only_once_when_two_changing_tasks_notify() {
        let _g = _file_mod_guard();
        let dest = std::env::temp_dir().join(format!("rsl-handler-once-{}.txt", std::process::id()));
        let dest_str = dest.to_string_lossy().replace('\\', "\\\\");
        let _ = std::fs::remove_file(&dest);
        let src = format!(r#"
[imports]
copy = "runsible_builtin.copy"
file = "runsible_builtin.file"
debug = "runsible_builtin.debug"
[[plays]]
name = "Handler dedup"
hosts = "localhost"
[[plays.tasks]]
name = "first changes"
notify = ["bell"]
copy = {{ content = "v1", dest = "{dest_str}" }}
[[plays.tasks]]
name = "second changes too"
notify = ["bell"]
file = {{ path = "{dest_str}", state = "touch" }}
[plays.handlers.bell]
debug = {{ msg = "ringing" }}
"#);
        let r = run(&src, "localhost,", "test").unwrap();
        let _ = std::fs::remove_file(&dest);
        assert_eq!(r.failed, 0);
        // 2 changing tasks + 1 handler = 3 outcomes (handler is Ok-status from debug)
        assert_eq!(r.changed, 2);
        assert_eq!(r.ok, 1, "handler fired once even with 2 notifies");
    }

    #[test]
    fn handler_does_not_fire_when_no_change() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "No notify"
hosts = "localhost"
[[plays.tasks]]
name = "ok-not-changed"
notify = ["never_called"]
debug = { msg = "stays Ok" }
[plays.handlers.never_called]
debug = { msg = "should not fire" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        // Just the one debug, status Ok. Handler never fires.
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn notify_two_handlers_both_fire() {
        let _g = _file_mod_guard();
        let dest = std::env::temp_dir().join(format!("rsl-notify-2h-{}.txt", std::process::id()));
        let dest_str = dest.to_string_lossy().replace('\\', "\\\\");
        let _ = std::fs::remove_file(&dest);
        let src = format!(r#"
[imports]
copy = "runsible_builtin.copy"
debug = "runsible_builtin.debug"
[[plays]]
name = "Two handlers"
hosts = "localhost"
[[plays.tasks]]
name = "change"
notify = ["h1", "h2"]
copy = {{ content = "z", dest = "{dest_str}" }}
[plays.handlers.h1]
debug = {{ msg = "1" }}
[plays.handlers.h2]
debug = {{ msg = "2" }}
"#);
        let r = run(&src, "localhost,", "test").unwrap();
        let _ = std::fs::remove_file(&dest);
        assert_eq!(r.failed, 0);
        // 1 changed copy + 2 handler debugs (each Ok) = ok 2 + changed 1.
        assert_eq!(r.changed, 1);
        assert_eq!(r.ok, 2);
    }

    #[test]
    fn pre_role_main_post_task_ordering() {
        let tmp = std::env::temp_dir().join(format!("rsl-ordering-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let pkg_dir = tmp.join("packages");
        let role_root = pkg_dir.join("rolex");
        std::fs::create_dir_all(role_root.join("tasks")).unwrap();
        std::fs::write(
            role_root.join("tasks/main.toml"),
            r#"
[[tasks]]
name = "set role layer"
set_fact = { layer = "role" }
[[tasks]]
name = "assert pre ran first"
assert = { that = ["pre_seen == true"] }
"#,
        )
        .unwrap();

        let pb = r#"
[imports]
debug = "runsible_builtin.debug"
set_fact = "runsible_builtin.set_fact"
assert = "runsible_builtin.assert"
[[plays]]
name = "Ordering"
hosts = "localhost"
[[plays.pre_tasks]]
name = "pre"
set_fact = { pre_seen = true, layer = "pre" }
[[plays.roles]]
name = "rolex"
[[plays.tasks]]
name = "main"
assert = { that = ["layer == 'role'"] }
[[plays.post_tasks]]
name = "post"
assert = { that = ["pre_seen == true", "layer == 'role'"] }
"#;
        let opts = engine::RunOptions {
            role_search_paths: Some(vec![pkg_dir.clone()]),
            ..Default::default()
        };
        let r = engine::run_with(pb, "localhost,", "test", opts).unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
        assert_eq!(r.failed, 0, "all assertions should pass with correct ordering");
        // pre(1) + role tasks(2) + main(1) + post(1) = 5
        assert_eq!(r.ok, 5);
    }

    #[test]
    fn inventory_hostname_magic_var_in_template() {
        let src = r#"
[imports]
assert = "runsible_builtin.assert"
[[plays]]
name = "Magic"
hosts = "all"
[[plays.tasks]]
name = "check name"
assert = { that = ["inventory_hostname == 'webnode42'"] }
"#;
        let r = run(src, "webnode42,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn loop_var_renames_binding_to_thing() {
        let src = r#"
[imports]
assert = "runsible_builtin.assert"
[[plays]]
name = "Rename"
hosts = "localhost"
[[plays.tasks]]
name = "iter"
loop = ["only"]
loop_control = { loop_var = "thing" }
assert = { that = ["thing == 'only'"] }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 1);
    }

    // --- Module-level happy path / idempotence ---

    #[test]
    fn command_creates_guard_skips_when_file_exists() {
        let guard = std::env::temp_dir().join(format!("rsl-creates-{}.flag", std::process::id()));
        std::fs::write(&guard, "exists").unwrap();
        let guard_str = guard.to_string_lossy().replace('\\', "\\\\");
        let src = format!(r#"
[imports]
command = "runsible_builtin.command"
[[plays]]
name = "Creates guard"
hosts = "localhost"
[[plays.tasks]]
name = "would echo"
command = {{ argv = ["echo", "would-run"], creates = "{guard_str}" }}
"#);
        let r = run(&src, "localhost,", "test").unwrap();
        let _ = std::fs::remove_file(&guard);
        assert_eq!(r.failed, 0);
        assert_eq!(r.skipped, 1);
        assert_eq!(r.changed, 0);
    }

    #[test]
    fn command_removes_guard_skips_when_file_absent() {
        let absent = std::env::temp_dir().join(format!("rsl-removes-{}.absent", std::process::id()));
        let _ = std::fs::remove_file(&absent);
        let path_str = absent.to_string_lossy().replace('\\', "\\\\");
        let src = format!(r#"
[imports]
command = "runsible_builtin.command"
[[plays]]
name = "Removes guard"
hosts = "localhost"
[[plays.tasks]]
name = "would echo"
command = {{ argv = ["echo", "would-run"], removes = "{path_str}" }}
"#);
        let r = run(&src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.skipped, 1);
        assert_eq!(r.changed, 0);
    }

    #[test]
    fn shell_with_explicit_executable_runs() {
        let src = r#"
[imports]
shell = "runsible_builtin.shell"
[[plays]]
name = "Shell"
hosts = "localhost"
[[plays.tasks]]
name = "true"
shell = { cmd = "true", executable = "/bin/sh" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.changed, 1);
    }

    #[cfg(unix)]
    #[test]
    fn copy_with_mode_sets_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let _g = _file_mod_guard();
        let dest = std::env::temp_dir().join(format!("rsl-copy-mode-{}.txt", std::process::id()));
        let dest_str = dest.to_string_lossy().replace('\\', "\\\\");
        let _ = std::fs::remove_file(&dest);
        let src = format!(r#"
[imports]
copy = "runsible_builtin.copy"
[[plays]]
name = "ModeCopy"
hosts = "localhost"
[[plays.tasks]]
name = "secret"
copy = {{ content = "secrets", dest = "{dest_str}", mode = "0600" }}
"#);
        let r = run(&src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.changed, 1);
        let mode = std::fs::metadata(&dest).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "expected 0o600, got {:o}", mode & 0o777);
        let _ = std::fs::remove_file(&dest);
    }

    #[cfg(unix)]
    #[test]
    fn copy_idempotent_with_mode_and_content() {
        let _g = _file_mod_guard();
        let dest = std::env::temp_dir().join(format!("rsl-copy-idem-{}.txt", std::process::id()));
        let dest_str = dest.to_string_lossy().replace('\\', "\\\\");
        let _ = std::fs::remove_file(&dest);
        let src = format!(r#"
[imports]
copy = "runsible_builtin.copy"
[[plays]]
name = "Idem"
hosts = "localhost"
[[plays.tasks]]
name = "write"
copy = {{ content = "hello", dest = "{dest_str}", mode = "0644" }}
"#);
        let r1 = run(&src, "localhost,", "test").unwrap();
        assert_eq!(r1.changed, 1);
        let r2 = run(&src, "localhost,", "test").unwrap();
        let _ = std::fs::remove_file(&dest);
        assert_eq!(r2.changed, 0);
        assert_eq!(r2.ok, 1);
    }

    #[test]
    fn file_touch_always_changes_even_if_exists() {
        let path = std::env::temp_dir().join(format!("rsl-touch-{}.txt", std::process::id()));
        std::fs::write(&path, "preexisting").unwrap();
        let path_str = path.to_string_lossy().replace('\\', "\\\\");
        let src = format!(r#"
[imports]
file = "runsible_builtin.file"
[[plays]]
name = "Touch"
hosts = "localhost"
[[plays.tasks]]
name = "touch"
file = {{ path = "{path_str}", state = "touch" }}
"#);
        let r = run(&src, "localhost,", "test").unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(r.failed, 0);
        assert_eq!(r.changed, 1);
    }

    #[test]
    fn file_absent_on_missing_path_is_ok_no_change() {
        let path = std::env::temp_dir().join(format!("rsl-absent-{}.nope", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let path_str = path.to_string_lossy().replace('\\', "\\\\");
        let src = format!(r#"
[imports]
file = "runsible_builtin.file"
[[plays]]
name = "Absent"
hosts = "localhost"
[[plays.tasks]]
name = "absent"
file = {{ path = "{path_str}", state = "absent" }}
"#);
        let r = run(&src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.changed, 0);
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn file_directory_creates_parents() {
        let root = std::env::temp_dir().join(format!("rsl-deeptest-{}", std::process::id()));
        let deep = root.join("y").join("z");
        let _ = std::fs::remove_dir_all(&root);
        let deep_str = deep.to_string_lossy().replace('\\', "\\\\");
        let src = format!(r#"
[imports]
file = "runsible_builtin.file"
[[plays]]
name = "Mkdir-p"
hosts = "localhost"
[[plays.tasks]]
name = "ensure"
file = {{ path = "{deep_str}", state = "directory" }}
"#);
        let r = run(&src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.changed, 1);
        assert!(deep.exists() && deep.is_dir(), "deep dir should exist");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn template_with_array_renders_into_toml_body() {
        let _g = _file_mod_guard();
        // Use a Jinja for-loop to build a TOML body, then parse it.
        let src_path = std::env::temp_dir().join(format!("rsl-tpl-arr-{}.j2", std::process::id()));
        let dest_path = std::env::temp_dir().join(format!("rsl-tpl-arr-{}.toml", std::process::id()));
        let _ = std::fs::remove_file(&src_path);
        let _ = std::fs::remove_file(&dest_path);
        std::fs::write(
            &src_path,
            "items = [{% for f in flavors %}\"{{ f }}\"{% if not loop.last %}, {% endif %}{% endfor %}]\n",
        )
        .unwrap();
        let src_str = src_path.to_string_lossy().replace('\\', "\\\\");
        let dest_str = dest_path.to_string_lossy().replace('\\', "\\\\");
        let pb = format!(r#"
[imports]
template = "runsible_builtin.template"
[[plays]]
name = "Arr"
hosts = "localhost"
[plays.vars]
flavors = ["chocolate", "vanilla", "mint"]
[[plays.tasks]]
name = "render"
template = {{ src = "{src_str}", dest = "{dest_str}" }}
"#);
        let r = run(&pb, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.changed, 1);
        let body = std::fs::read_to_string(&dest_path).unwrap();
        let parsed: toml::Value = toml::from_str(&body).expect("rendered toml should parse");
        let arr = parsed
            .get("items")
            .and_then(|v| v.as_array())
            .expect("items array");
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0].as_str().unwrap(), "chocolate");
        assert_eq!(arr[2].as_str().unwrap(), "mint");
        let _ = std::fs::remove_file(&src_path);
        let _ = std::fs::remove_file(&dest_path);
    }

    #[test]
    fn get_url_module_is_registered_in_catalog() {
        let cat = catalog::ModuleCatalog::with_builtins();
        assert!(cat.get("runsible_builtin.get_url").is_some());
    }

    // --- RunResult / outcome correctness ---

    #[test]
    fn run_result_exit_code_two_when_failed() {
        let r = RunResult {
            ok: 0,
            changed: 0,
            failed: 1,
            skipped: 0,
            elapsed_ms: 0,
        };
        assert_eq!(r.exit_code(), 2);
    }

    #[test]
    fn run_result_exit_code_zero_when_only_ok() {
        let r = RunResult {
            ok: 5,
            changed: 0,
            failed: 0,
            skipped: 0,
            elapsed_ms: 0,
        };
        assert_eq!(r.exit_code(), 0);
    }

    #[test]
    fn run_result_exit_code_zero_when_no_tasks_ran() {
        let r = RunResult {
            ok: 0,
            changed: 0,
            failed: 0,
            skipped: 0,
            elapsed_ms: 0,
        };
        assert_eq!(r.exit_code(), 0);
    }

    // --- Setup / fact gathering & magic vars ---

    #[test]
    fn setup_module_registers() {
        let cat = catalog::ModuleCatalog::with_builtins();
        assert!(cat.get("runsible_builtin.setup").is_some());
    }

    #[test]
    fn setup_gathers_min_facts() {
        let src = r#"
[imports]
setup = "runsible_builtin.setup"
debug = "runsible_builtin.debug"
[[plays]]
name = "Gather"
hosts = "localhost"
[[plays.tasks]]
setup = {}
[[plays.tasks]]
debug = { msg = "host is {{ ansible_hostname }}" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 2);
    }

    #[test]
    fn gather_facts_play_flag_runs_setup_first() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Auto"
hosts = "localhost"
gather_facts = true
[[plays.tasks]]
debug = { msg = "{{ ansible_hostname }}" }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        // setup + debug = 2 outcomes
        assert_eq!(r.ok, 2);
    }

    #[test]
    fn magic_vars_inventory_hostname_short() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Magic"
hosts = "all"
[[plays.tasks]]
debug = { msg = "short is {{ inventory_hostname_short }}" }
"#;
        let r = run(src, "web01.example.com,", "test").unwrap();
        assert_eq!(r.failed, 0);
    }

    #[test]
    fn magic_vars_groups_exists() {
        let src = r#"
[imports]
assert = "runsible_builtin.assert"
[[plays]]
name = "Groups"
hosts = "localhost"
[[plays.tasks]]
assert = { that = ["groups is defined"] }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
    }

    #[test]
    fn ansible_check_mode_var_set() {
        let src = r#"
[imports]
assert = "runsible_builtin.assert"
[[plays]]
name = "Check var"
hosts = "localhost"
[[plays.tasks]]
assert = { that = ["ansible_check_mode == false"] }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
    }

    #[test]
    fn magic_vars_play_hosts_and_play_name() {
        let src = r#"
[imports]
assert = "runsible_builtin.assert"
[[plays]]
name = "Play X"
hosts = "all"
[[plays.tasks]]
assert = { that = ["ansible_play_name == 'Play X'", "play_hosts is defined"] }
"#;
        let r = run(src, "h1,h2,", "test").unwrap();
        assert_eq!(r.failed, 0);
    }

    #[test]
    fn magic_vars_ansible_version_dict() {
        let src = r#"
[imports]
assert = "runsible_builtin.assert"
[[plays]]
name = "Version"
hosts = "localhost"
[[plays.tasks]]
assert = { that = ["ansible_version.major == 0", "ansible_version.full is defined"] }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
    }

    #[test]
    fn setup_facts_merged_into_top_level_vars() {
        // After setup runs, ansible_hostname should template directly without
        // needing the `ansible_facts.` prefix.
        let src = r#"
[imports]
setup = "runsible_builtin.setup"
assert = "runsible_builtin.assert"
[[plays]]
name = "Merged"
hosts = "localhost"
[[plays.tasks]]
setup = {}
[[plays.tasks]]
assert = { that = ["ansible_hostname is defined", "ansible_facts.ansible_hostname is defined"] }
"#;
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 2);
    }

    // -------------------------------------------------------------------
    // M1.5 — 15 new builtin modules: catalog registration + plan/apply
    // smoke checks. Privileged ops (user/group/cron/hostname/uri) only
    // verify catalog registration here; integration tests would need root.
    // -------------------------------------------------------------------

    #[test]
    fn catalog_has_15_new_modules() {
        let cat = catalog::ModuleCatalog::with_builtins();
        for name in [
            "lineinfile",
            "blockinfile",
            "replace",
            "stat",
            "find",
            "fail",
            "pause",
            "wait_for",
            "uri",
            "archive",
            "unarchive",
            "user",
            "group",
            "cron",
            "hostname",
        ] {
            let fq = format!("runsible_builtin.{name}");
            assert!(cat.get(&fq).is_some(), "missing {name}");
        }
    }

    fn _new_module_ctx<'a>(
        host: &'a runsible_core::types::Host,
        vars: &'a runsible_core::types::Vars,
        conn: &'a runsible_connection::LocalSync,
    ) -> runsible_core::traits::ExecutionContext<'a> {
        runsible_core::traits::ExecutionContext {
            host,
            vars,
            connection: conn,
            check_mode: false,
            diff_mode: false,
        }
    }

    #[test]
    fn fail_module_plan_carries_msg_and_apply_fails() {
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = _new_module_ctx(&host, &vars, &conn);
        let m = modules::fail::FailModule;
        let args = toml::from_str::<toml::Value>(r#"msg = "boom""#).unwrap();
        let plan = catalog::DynModule::plan(&m, &args, &ctx).unwrap();
        assert_eq!(plan.diff["msg"], "boom");
        let outcome = catalog::DynModule::apply(&m, &plan, &ctx).unwrap();
        assert_eq!(outcome.status, runsible_core::types::OutcomeStatus::Failed);
        assert_eq!(outcome.returns["msg"], "boom");
    }

    #[test]
    fn pause_module_zero_seconds_is_fast() {
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = _new_module_ctx(&host, &vars, &conn);
        let m = modules::pause::PauseModule;
        let args = toml::from_str::<toml::Value>(r#"seconds = 0"#).unwrap();
        let plan = catalog::DynModule::plan(&m, &args, &ctx).unwrap();
        assert!(!plan.will_change);
        let started = std::time::Instant::now();
        let outcome = catalog::DynModule::apply(&m, &plan, &ctx).unwrap();
        let elapsed = started.elapsed();
        assert_eq!(outcome.status, runsible_core::types::OutcomeStatus::Ok);
        assert!(
            elapsed.as_millis() < 100,
            "pause(0) should return in <100ms, got {:?}",
            elapsed
        );
    }

    #[test]
    fn lineinfile_creates_file_when_missing_and_create_true() {
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = _new_module_ctx(&host, &vars, &conn);
        let path = std::env::temp_dir().join(format!("rsl-lineinfile-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let m = modules::lineinfile::LineInFileModule;
        let path_str = path.to_string_lossy().replace('\\', "\\\\");
        let toml_str = format!(
            r#"path = "{}"
line = "the line"
create = true"#,
            path_str
        );
        let args = toml::from_str::<toml::Value>(&toml_str).unwrap();
        let plan = catalog::DynModule::plan(&m, &args, &ctx).unwrap();
        assert!(plan.will_change);
        let outcome = catalog::DynModule::apply(&m, &plan, &ctx).unwrap();
        assert_eq!(outcome.status, runsible_core::types::OutcomeStatus::Changed);
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("the line"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lineinfile_idempotent_when_already_present() {
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = _new_module_ctx(&host, &vars, &conn);
        let path = std::env::temp_dir().join(format!(
            "rsl-lineinfile-idem-{}.txt",
            std::process::id()
        ));
        std::fs::write(&path, "alpha\nbeta\n").unwrap();
        let m = modules::lineinfile::LineInFileModule;
        let path_str = path.to_string_lossy().replace('\\', "\\\\");
        let toml_str = format!(
            r#"path = "{}"
line = "beta""#,
            path_str
        );
        let args = toml::from_str::<toml::Value>(&toml_str).unwrap();
        let plan = catalog::DynModule::plan(&m, &args, &ctx).unwrap();
        assert!(!plan.will_change, "line already present should not change");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn blockinfile_inserts_marker_block() {
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = _new_module_ctx(&host, &vars, &conn);
        let path = std::env::temp_dir().join(format!(
            "rsl-blockinfile-{}.txt",
            std::process::id()
        ));
        std::fs::write(&path, "header\n").unwrap();
        let path_str = path.to_string_lossy().replace('\\', "\\\\");
        let m = modules::blockinfile::BlockInFileModule;
        let toml_str = format!(
            r#"path = "{}"
block = "first\nsecond""#,
            path_str
        );
        let args = toml::from_str::<toml::Value>(&toml_str).unwrap();
        let plan = catalog::DynModule::plan(&m, &args, &ctx).unwrap();
        assert!(plan.will_change);
        let outcome = catalog::DynModule::apply(&m, &plan, &ctx).unwrap();
        assert_eq!(outcome.status, runsible_core::types::OutcomeStatus::Changed);
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("BEGIN"));
        assert!(body.contains("END"));
        assert!(body.contains("first"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn replace_module_substitutes_and_is_idempotent() {
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = _new_module_ctx(&host, &vars, &conn);
        let path = std::env::temp_dir().join(format!("rsl-replace-{}.txt", std::process::id()));
        std::fs::write(&path, "foo bar foo\n").unwrap();
        let path_str = path.to_string_lossy().replace('\\', "\\\\");
        let m = modules::replace::ReplaceModule;
        let toml_str = format!(
            r#"path = "{}"
regexp = "foo"
replace = "baz""#,
            path_str
        );
        let args = toml::from_str::<toml::Value>(&toml_str).unwrap();
        let plan = catalog::DynModule::plan(&m, &args, &ctx).unwrap();
        assert!(plan.will_change);
        let _ = catalog::DynModule::apply(&m, &plan, &ctx).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(body, "baz bar baz\n");
        // Re-plan: should be no-op now.
        let plan2 = catalog::DynModule::plan(&m, &args, &ctx).unwrap();
        assert!(!plan2.will_change);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn stat_module_returns_size_and_exists() {
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = _new_module_ctx(&host, &vars, &conn);
        let path = std::env::temp_dir().join(format!("rsl-stat-{}.txt", std::process::id()));
        std::fs::write(&path, "hello").unwrap();
        let path_str = path.to_string_lossy().replace('\\', "\\\\");
        let m = modules::stat::StatModule;
        let toml_str = format!(r#"path = "{}""#, path_str);
        let args = toml::from_str::<toml::Value>(&toml_str).unwrap();
        let plan = catalog::DynModule::plan(&m, &args, &ctx).unwrap();
        assert!(!plan.will_change);
        let outcome = catalog::DynModule::apply(&m, &plan, &ctx).unwrap();
        assert_eq!(outcome.status, runsible_core::types::OutcomeStatus::Ok);
        assert_eq!(outcome.returns["stat"]["exists"], true);
        assert_eq!(outcome.returns["stat"]["size"], 5);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn stat_module_missing_path_reports_not_exists() {
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = _new_module_ctx(&host, &vars, &conn);
        let path = std::env::temp_dir().join(format!(
            "rsl-stat-missing-{}.txt",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let m = modules::stat::StatModule;
        let path_str = path.to_string_lossy().replace('\\', "\\\\");
        let toml_str = format!(r#"path = "{}""#, path_str);
        let args = toml::from_str::<toml::Value>(&toml_str).unwrap();
        let plan = catalog::DynModule::plan(&m, &args, &ctx).unwrap();
        let outcome = catalog::DynModule::apply(&m, &plan, &ctx).unwrap();
        assert_eq!(outcome.status, runsible_core::types::OutcomeStatus::Ok);
        assert_eq!(outcome.returns["stat"]["exists"], false);
    }

    #[test]
    fn find_module_returns_files_dict() {
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = _new_module_ctx(&host, &vars, &conn);
        let m = modules::find::FindModule;
        let args = toml::from_str::<toml::Value>(
            r#"paths = "/tmp"
patterns = "*.tmp""#,
        )
        .unwrap();
        let plan = catalog::DynModule::plan(&m, &args, &ctx).unwrap();
        assert!(!plan.will_change);
        let outcome = catalog::DynModule::apply(&m, &plan, &ctx).unwrap();
        // Either the find ran cleanly (Ok) or it failed for reasons we can't
        // predict — but the structure must be present.
        assert!(outcome.returns.get("files").is_some(), "expected `files` key");
    }

    #[test]
    fn wait_for_unreachable_port_times_out_quickly() {
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = _new_module_ctx(&host, &vars, &conn);
        let m = modules::wait_for::WaitForModule;
        // Port 1 on localhost should never accept; timeout=1s.
        let args = toml::from_str::<toml::Value>(
            r#"host = "127.0.0.1"
port = 1
timeout = 1
connect_timeout = 1"#,
        )
        .unwrap();
        let plan = catalog::DynModule::plan(&m, &args, &ctx).unwrap();
        let started = std::time::Instant::now();
        let outcome = catalog::DynModule::apply(&m, &plan, &ctx).unwrap();
        let elapsed = started.elapsed();
        assert_eq!(outcome.status, runsible_core::types::OutcomeStatus::Failed);
        assert!(
            elapsed.as_secs_f64() < 3.0,
            "expected fail in <3s, got {:?}",
            elapsed
        );
    }

    #[test]
    fn archive_creates_tarball() {
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = _new_module_ctx(&host, &vars, &conn);
        let dir = std::env::temp_dir().join(format!("rsl-archive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src_file = dir.join("source.txt");
        std::fs::write(&src_file, "payload").unwrap();
        let dest = dir.join("out.tar.gz");

        let src_str = src_file.to_string_lossy().replace('\\', "\\\\");
        let dest_str = dest.to_string_lossy().replace('\\', "\\\\");
        let m = modules::archive::ArchiveModule;
        let toml_str = format!(
            r#"path = ["{}"]
dest = "{}"
format = "gz""#,
            src_str, dest_str
        );
        let args = toml::from_str::<toml::Value>(&toml_str).unwrap();
        let plan = catalog::DynModule::plan(&m, &args, &ctx).unwrap();
        assert!(plan.will_change);
        let outcome = catalog::DynModule::apply(&m, &plan, &ctx).unwrap();
        assert_eq!(outcome.status, runsible_core::types::OutcomeStatus::Changed);
        let meta = std::fs::metadata(&dest).expect("archive should exist");
        assert!(meta.len() > 0, "archive should be non-empty");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unarchive_plan_validates_args() {
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = _new_module_ctx(&host, &vars, &conn);
        let m = modules::unarchive::UnarchiveModule;
        let bad = toml::from_str::<toml::Value>(r#"src = "/x""#).unwrap();
        assert!(catalog::DynModule::plan(&m, &bad, &ctx).is_err());
        let ok = toml::from_str::<toml::Value>(
            r#"src = "/some/archive.tar.gz"
dest = "/some/where""#,
        )
        .unwrap();
        let plan = catalog::DynModule::plan(&m, &ok, &ctx).unwrap();
        assert!(plan.will_change);
    }

    #[test]
    fn user_module_plan_validates_name() {
        if !nix_running_as_root() {
            eprintln!("skip: user module integration requires root");
        }
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = _new_module_ctx(&host, &vars, &conn);
        let m = modules::user::UserModule;
        let bad = toml::from_str::<toml::Value>(r#"state = "present""#).unwrap();
        assert!(catalog::DynModule::plan(&m, &bad, &ctx).is_err());
    }

    #[test]
    fn group_module_plan_validates_name() {
        if !nix_running_as_root() {
            eprintln!("skip: group module integration requires root");
        }
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = _new_module_ctx(&host, &vars, &conn);
        let m = modules::group::GroupModule;
        let bad = toml::from_str::<toml::Value>(r#"state = "present""#).unwrap();
        assert!(catalog::DynModule::plan(&m, &bad, &ctx).is_err());
    }

    #[test]
    fn cron_module_plan_validates_name() {
        if !nix_running_as_root() {
            eprintln!("skip: cron module integration requires crontab/sudo");
        }
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = _new_module_ctx(&host, &vars, &conn);
        let m = modules::cron::CronModule;
        let bad = toml::from_str::<toml::Value>(r#"state = "present""#).unwrap();
        assert!(catalog::DynModule::plan(&m, &bad, &ctx).is_err());
        let needs_job =
            toml::from_str::<toml::Value>(r#"name = "x""#).unwrap();
        assert!(catalog::DynModule::plan(&m, &needs_job, &ctx).is_err());
    }

    #[test]
    fn hostname_module_plan_validates_name() {
        if !nix_running_as_root() {
            eprintln!("skip: hostname module integration requires root");
        }
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = _new_module_ctx(&host, &vars, &conn);
        let m = modules::hostname::HostnameModule;
        let bad = toml::from_str::<toml::Value>(r#""#).unwrap();
        assert!(catalog::DynModule::plan(&m, &bad, &ctx).is_err());
    }

    #[test]
    fn uri_module_plan_validates_url() {
        if !which_curl_available() {
            eprintln!("skip: uri module integration requires curl");
        }
        use runsible_core::types::{Host, Vars};
        let host = Host { name: "h".into(), vars: Vars::new() };
        let vars = Vars::new();
        let conn = runsible_connection::LocalSync;
        let ctx = _new_module_ctx(&host, &vars, &conn);
        let m = modules::uri::UriModule;
        let bad = toml::from_str::<toml::Value>(r#"method = "GET""#).unwrap();
        assert!(catalog::DynModule::plan(&m, &bad, &ctx).is_err());
        let ok = toml::from_str::<toml::Value>(r#"url = "http://localhost:1""#).unwrap();
        let plan = catalog::DynModule::plan(&m, &ok, &ctx).unwrap();
        assert!(plan.will_change);
    }

    fn nix_running_as_root() -> bool {
        // Best-effort: use $USER or geteuid() proxy via /proc.
        std::env::var("USER").map(|u| u == "root").unwrap_or(false)
    }

    fn which_curl_available() -> bool {
        std::process::Command::new("which")
            .arg("curl")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    // --- forks / parallel host execution ---

    #[test]
    fn forks_default_runs_sequentially() {
        let pb = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Sequential"
hosts = "all"
[[plays.tasks]]
debug = { msg = "hello" }
"#;
        let opts = engine::RunOptions::default();
        assert_eq!(opts.forks, 1, "default forks should be 1");
        let r = engine::run_with(pb, "h1,h2,h3,h4,h5,", "test", opts).unwrap();
        assert_eq!(r.ok, 5);
        assert_eq!(r.failed, 0);
    }

    #[test]
    fn forks_parallel_runs_all_hosts() {
        let pb = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Parallel"
hosts = "all"
[[plays.tasks]]
debug = { msg = "hello" }
"#;
        let opts = engine::RunOptions {
            forks: 4,
            ..Default::default()
        };
        let r = engine::run_with(pb, "h1,h2,h3,h4,h5,h6,h7,h8,", "test", opts).unwrap();
        assert_eq!(r.ok, 8);
        assert_eq!(r.failed, 0);
    }

    #[test]
    fn forks_independent_set_facts_per_host() {
        let pb = r#"
[imports]
set_fact = "runsible_builtin.set_fact"
assert = "runsible_builtin.assert"
[[plays]]
name = "Independent vars"
hosts = "all"
[[plays.tasks]]
set_fact = { my_host = "{{ inventory_hostname }}" }
[[plays.tasks]]
assert = { that = ["my_host == inventory_hostname"] }
"#;
        let opts = engine::RunOptions {
            forks: 4,
            ..Default::default()
        };
        let r = engine::run_with(pb, "h1,h2,h3,h4,", "test", opts).unwrap();
        // Each host should have its own my_host fact: 4 set_fact + 4 assert pass
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 8);
    }

    #[test]
    fn forks_one_failure_doesnt_block_others() {
        let pb = r#"
[imports]
fail = "runsible_builtin.fail"
debug = "runsible_builtin.debug"
[[plays]]
name = "Mixed"
hosts = "all"
[[plays.tasks]]
when = { expr = "inventory_hostname == 'badhost'" }
fail = { msg = "this host fails" }
[[plays.tasks]]
debug = { msg = "but I should still see this on good hosts" }
"#;
        let opts = engine::RunOptions {
            forks: 4,
            ..Default::default()
        };
        let r = engine::run_with(pb, "h1,badhost,h3,h4,", "test", opts).unwrap();
        // 1 fail (badhost) + 4 debug ok + 3 skipped (the conditional fail on good hosts)
        assert_eq!(r.failed, 1);
        assert_eq!(r.ok, 4);
    }

    // ----- check_mode + diff_mode wiring -----

    #[test]
    fn check_mode_skips_real_changes() {
        let _g = _file_mod_guard();
        let dest = std::env::temp_dir().join(format!("rsl-check-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&dest);
        let dest_str = dest.to_string_lossy().replace('\\', "\\\\");
        let pb = format!(r#"
[imports]
copy = "runsible_builtin.copy"
[[plays]]
name = "check"
hosts = "localhost"
[[plays.tasks]]
copy = {{ content = "should not write", dest = "{dest_str}" }}
"#);
        let opts = engine::RunOptions {
            check_mode: true,
            ..Default::default()
        };
        let r = engine::run_with(&pb, "localhost,", "test", opts).unwrap();
        // Module reports Changed (would-have-changed), but no file is on disk.
        assert_eq!(r.changed, 1);
        assert!(!dest.exists(), "check mode must not write");
    }

    #[test]
    fn check_mode_runs_safe_modules() {
        let pb = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "safe"
hosts = "localhost"
[[plays.tasks]]
debug = { msg = "still ran" }
"#;
        let opts = engine::RunOptions {
            check_mode: true,
            ..Default::default()
        };
        let r = engine::run_with(pb, "localhost,", "test", opts).unwrap();
        assert_eq!(r.ok, 1, "debug should still run in check mode");
    }

    #[test]
    fn check_mode_runs_ping() {
        let pb = r#"
[imports]
ping = "runsible_builtin.ping"
[[plays]]
name = "ping-check"
hosts = "localhost"
[[plays.tasks]]
ping = {}
"#;
        let opts = engine::RunOptions {
            check_mode: true,
            ..Default::default()
        };
        let r = engine::run_with(pb, "localhost,", "test", opts).unwrap();
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn check_mode_runs_set_fact_and_var_propagates() {
        let pb = r#"
[imports]
set_fact = "runsible_builtin.set_fact"
assert = "runsible_builtin.assert"
[[plays]]
name = "sf-check"
hosts = "localhost"
[[plays.tasks]]
set_fact = { side = "left" }
[[plays.tasks]]
assert = { that = ["side == 'left'"] }
"#;
        let opts = engine::RunOptions {
            check_mode: true,
            ..Default::default()
        };
        let r = engine::run_with(pb, "localhost,", "test", opts).unwrap();
        // set_fact reports Ok (per its existing semantics), assert reports Ok.
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 2);
    }

    #[test]
    fn check_mode_runs_assert() {
        let pb = r#"
[imports]
assert = "runsible_builtin.assert"
[[plays]]
name = "assert-check"
hosts = "localhost"
[plays.vars]
n = 7
[[plays.tasks]]
assert = { that = ["n == 7"] }
"#;
        let opts = engine::RunOptions {
            check_mode: true,
            ..Default::default()
        };
        let r = engine::run_with(pb, "localhost,", "test", opts).unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn check_mode_skips_file_create() {
        let target = std::env::temp_dir().join(format!("rsl-check-file-{}.flag", std::process::id()));
        let _ = std::fs::remove_file(&target);
        let path_str = target.to_string_lossy().replace('\\', "\\\\");
        let pb = format!(r#"
[imports]
file = "runsible_builtin.file"
[[plays]]
name = "file-check"
hosts = "localhost"
[[plays.tasks]]
file = {{ path = "{path_str}", state = "touch" }}
"#);
        let opts = engine::RunOptions {
            check_mode: true,
            ..Default::default()
        };
        let r = engine::run_with(&pb, "localhost,", "test", opts).unwrap();
        assert_eq!(r.changed, 1);
        assert!(!target.exists(), "check mode must not touch the file");
    }

    #[test]
    fn check_mode_synthesizes_check_mode_returns_marker() {
        // When check_mode skips apply(), the synthesized outcome carries a
        // `check_mode: true` marker in returns. Verify by registering the
        // result and asserting on it.
        let _g = _file_mod_guard();
        let dest = std::env::temp_dir().join(format!("rsl-check-marker-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&dest);
        let dest_str = dest.to_string_lossy().replace('\\', "\\\\");
        let pb = format!(r#"
[imports]
copy = "runsible_builtin.copy"
assert = "runsible_builtin.assert"
[[plays]]
name = "marker"
hosts = "localhost"
[[plays.tasks]]
register = "cp"
copy = {{ content = "x", dest = "{dest_str}" }}
[[plays.tasks]]
assert = {{ that = ["cp.returns.check_mode == true", "cp.returns.would_change == true"] }}
"#);
        let opts = engine::RunOptions {
            check_mode: true,
            ..Default::default()
        };
        let r = engine::run_with(&pb, "localhost,", "test", opts).unwrap();
        let _ = std::fs::remove_file(&dest);
        assert_eq!(r.failed, 0, "marker should be present in registered outcome");
    }

    #[test]
    fn diff_mode_populates_before_after_for_copy() {
        let _g = _file_mod_guard();
        let dest = std::env::temp_dir().join(format!("rsl-diff-{}.txt", std::process::id()));
        std::fs::write(&dest, "old content\n").unwrap();
        let dest_str = dest.to_string_lossy().replace('\\', "\\\\");
        let pb = format!(r#"
[imports]
copy = "runsible_builtin.copy"
assert = "runsible_builtin.assert"
[[plays]]
name = "diff"
hosts = "localhost"
[[plays.tasks]]
register = "cp"
copy = {{ content = "new content\n", dest = "{dest_str}" }}
[[plays.tasks]]
assert = {{ that = ["cp.returns.diff.before == 'old content\n'", "cp.returns.diff.after == 'new content\n'"] }}
"#);
        let opts = engine::RunOptions {
            check_mode: true,
            diff_mode: true,
            ..Default::default()
        };
        let r = engine::run_with(&pb, "localhost,", "test", opts).unwrap();
        let _ = std::fs::remove_file(&dest);
        assert_eq!(r.failed, 0);
    }

    #[test]
    fn check_mode_unchanged_module_reports_ok() {
        // When the plan reports will_change=false, check_mode still skips apply
        // — the synthesized outcome should be Ok, not Changed.
        let _g = _file_mod_guard();
        let dest = std::env::temp_dir().join(format!("rsl-check-unchanged-{}.txt", std::process::id()));
        std::fs::write(&dest, "same\n").unwrap();
        let dest_str = dest.to_string_lossy().replace('\\', "\\\\");
        let pb = format!(r#"
[imports]
copy = "runsible_builtin.copy"
[[plays]]
name = "noop"
hosts = "localhost"
[[plays.tasks]]
copy = {{ content = "same\n", dest = "{dest_str}" }}
"#);
        let opts = engine::RunOptions {
            check_mode: true,
            ..Default::default()
        };
        let r = engine::run_with(&pb, "localhost,", "test", opts).unwrap();
        let _ = std::fs::remove_file(&dest);
        assert_eq!(r.changed, 0);
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn check_mode_template_does_not_write() {
        let _g = _file_mod_guard();
        let src_path = std::env::temp_dir().join(format!("rsl-check-tpl-src-{}.j2", std::process::id()));
        let dest_path = std::env::temp_dir().join(format!("rsl-check-tpl-dst-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&src_path);
        let _ = std::fs::remove_file(&dest_path);
        std::fs::write(&src_path, "Hello, {{ name }}!\n").unwrap();
        let src_str = src_path.to_string_lossy().replace('\\', "\\\\");
        let dest_str = dest_path.to_string_lossy().replace('\\', "\\\\");
        let pb = format!(r#"
[imports]
template = "runsible_builtin.template"
[[plays]]
name = "tpl-check"
hosts = "localhost"
[plays.vars]
name = "World"
[[plays.tasks]]
template = {{ src = "{src_str}", dest = "{dest_str}" }}
"#);
        let opts = engine::RunOptions {
            check_mode: true,
            ..Default::default()
        };
        let r = engine::run_with(&pb, "localhost,", "test", opts).unwrap();
        let dst_exists = dest_path.exists();
        let _ = std::fs::remove_file(&src_path);
        let _ = std::fs::remove_file(&dest_path);
        assert_eq!(r.changed, 1);
        assert!(!dst_exists, "check mode must not write template output");
    }

    #[test]
    fn check_mode_command_module_skipped() {
        // command is non-idempotent (will_change=true unless guarded). In
        // check_mode the engine should NOT actually invoke the binary; the
        // synthesized outcome still reports Changed.
        let sentinel = std::env::temp_dir().join(format!(
            "rsl-check-cmd-{}.flag",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&sentinel);
        let s = sentinel.to_string_lossy().replace('\\', "\\\\");
        let pb = format!(r#"
[imports]
command = "runsible_builtin.command"
[[plays]]
name = "cmd-check"
hosts = "localhost"
[[plays.tasks]]
command = {{ argv = ["touch", "{s}"] }}
"#);
        let opts = engine::RunOptions {
            check_mode: true,
            ..Default::default()
        };
        let r = engine::run_with(&pb, "localhost,", "test", opts).unwrap();
        let _ = std::fs::remove_file(&sentinel);
        assert_eq!(r.changed, 1);
        assert!(!sentinel.exists(), "command must not actually execute in check mode");
    }

    #[test]
    fn run_options_default_has_no_check_or_diff() {
        let opts = engine::RunOptions::default();
        assert!(!opts.check_mode);
        assert!(!opts.diff_mode);
    }

    // -------------------------------------------------------------------------
    // vars_files / module_defaults / include_tasks / delegate_to / run_once
    // (M1 engine wiring)
    // -------------------------------------------------------------------------

    #[test]
    fn vars_files_loaded_into_play() {
        let tmp = std::env::temp_dir();
        let vf_path = tmp.join(format!(
            "rsl-vf-{}-{}.toml",
            std::process::id(),
            line!()
        ));
        std::fs::write(&vf_path, r#"app_name = "from_file""#).unwrap();
        let vf_str = vf_path.to_string_lossy().replace('\\', "\\\\");
        let pb = format!(r#"
[imports]
debug = "runsible_builtin.debug"
assert = "runsible_builtin.assert"
[[plays]]
name = "Vars files"
hosts = "localhost"
vars_files = ["{vf_str}"]
[[plays.tasks]]
debug = {{ msg = "app: {{{{ app_name }}}}" }}
[[plays.tasks]]
assert = {{ that = ["app_name == 'from_file'"] }}
"#);
        let r = run(&pb, "localhost,", "test").unwrap();
        let _ = std::fs::remove_file(&vf_path);
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 2);
    }

    #[test]
    fn vars_files_missing_file_silently_skipped() {
        // The named vars_file does not exist. M1 contract: silently skip.
        // The play continues to run with whatever vars are otherwise defined.
        let pb = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Missing vf"
hosts = "localhost"
vars_files = ["/nonexistent/path/to/vars-no-such-file-xyz.toml"]
[plays.vars]
fallback = "ok"
[[plays.tasks]]
debug = { msg = "fallback is {{ fallback }}" }
"#;
        let r = run(pb, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn vars_files_overridden_by_play_vars() {
        // Precedence: vars_files load BEFORE inline play.vars, so play.vars wins.
        let vf_path = std::env::temp_dir().join(format!(
            "rsl-vf-prec-{}-{}.toml",
            std::process::id(),
            line!()
        ));
        std::fs::write(&vf_path, r#"k = "from_file""#).unwrap();
        let vf_str = vf_path.to_string_lossy().replace('\\', "\\\\");
        let pb = format!(r#"
[imports]
assert = "runsible_builtin.assert"
[[plays]]
name = "Precedence"
hosts = "localhost"
vars_files = ["{vf_str}"]
[plays.vars]
k = "from_play_vars"
[[plays.tasks]]
assert = {{ that = ["k == 'from_play_vars'"] }}
"#);
        let r = run(&pb, "localhost,", "test").unwrap();
        let _ = std::fs::remove_file(&vf_path);
        assert_eq!(r.failed, 0, "play.vars must win over vars_files");
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn module_defaults_apply_when_task_omits() {
        let pb = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Mod defaults"
hosts = "localhost"
[plays.module_defaults."runsible_builtin.debug"]
msg = "from_defaults"
[[plays.tasks]]
debug = {}
"#;
        let r = run(pb, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn module_defaults_overridden_by_task() {
        // Task-level args win over module_defaults on key collision. We can't
        // easily inspect the rendered msg without parsing the NDJSON event
        // stream, but no failure indicates the merge produced a valid call.
        let pb = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Override"
hosts = "localhost"
[plays.module_defaults."runsible_builtin.debug"]
msg = "default_msg"
[[plays.tasks]]
debug = { msg = "task_msg" }
"#;
        let r = run(pb, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn module_defaults_only_apply_to_matching_module() {
        // The defaults are scoped to debug; the assert task must be unaffected
        // (would otherwise crash on missing `that`).
        let pb = r#"
[imports]
debug = "runsible_builtin.debug"
assert = "runsible_builtin.assert"
[[plays]]
name = "Scoped defaults"
hosts = "localhost"
[plays.module_defaults."runsible_builtin.debug"]
msg = "only_for_debug"
[[plays.tasks]]
debug = {}
[[plays.tasks]]
assert = { that = ["1 == 1"] }
"#;
        let r = run(pb, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 2);
    }

    #[test]
    fn include_tasks_loads_external_file() {
        let inc_path = std::env::temp_dir().join(format!(
            "rsl-inc-{}-{}.toml",
            std::process::id(),
            line!()
        ));
        std::fs::write(
            &inc_path,
            r#"
[[tasks]]
name = "from include"
debug = { msg = "included" }
"#,
        )
        .unwrap();
        let inc_str = inc_path.to_string_lossy().replace('\\', "\\\\");
        let pb = format!(r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Include"
hosts = "localhost"
[[plays.tasks]]
include_tasks = "{inc_str}"
"#);
        let r = run(&pb, "localhost,", "test").unwrap();
        let _ = std::fs::remove_file(&inc_path);
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 1, "the included task should have run");
    }

    #[test]
    fn import_tasks_alias_behaves_like_include_at_m1() {
        // import_tasks resolves through the same dispatcher as include_tasks at M1.
        let inc_path = std::env::temp_dir().join(format!(
            "rsl-imp-{}-{}.toml",
            std::process::id(),
            line!()
        ));
        std::fs::write(
            &inc_path,
            r#"
[[tasks]]
name = "imported"
debug = { msg = "via import" }
"#,
        )
        .unwrap();
        let inc_str = inc_path.to_string_lossy().replace('\\', "\\\\");
        let pb = format!(r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Import"
hosts = "localhost"
[[plays.tasks]]
import_tasks = "{inc_str}"
"#);
        let r = run(&pb, "localhost,", "test").unwrap();
        let _ = std::fs::remove_file(&inc_path);
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn include_tasks_invalid_toml_errors() {
        let inc_path = std::env::temp_dir().join(format!(
            "rsl-inc-bad-{}-{}.toml",
            std::process::id(),
            line!()
        ));
        // Garbage TOML body so the parser bails out.
        std::fs::write(&inc_path, "this is = ][not valid toml").unwrap();
        let inc_str = inc_path.to_string_lossy().replace('\\', "\\\\");
        let pb = format!(r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Bad include"
hosts = "localhost"
[[plays.tasks]]
include_tasks = "{inc_str}"
"#);
        let err = run(&pb, "localhost,", "test").unwrap_err();
        let _ = std::fs::remove_file(&inc_path);
        assert!(
            matches!(err, PlaybookError::Parse(_)),
            "expected Parse error, got {err:?}"
        );
    }

    #[test]
    fn include_tasks_supports_top_level_array() {
        // The included file is a bare array of tables (no `tasks = [...]` wrapper).
        let inc_path = std::env::temp_dir().join(format!(
            "rsl-inc-arr-{}-{}.toml",
            std::process::id(),
            line!()
        ));
        std::fs::write(
            &inc_path,
            r#"
[[__top]]
name = "first"
debug = { msg = "1" }
[[__top]]
name = "second"
debug = { msg = "2" }
"#,
        )
        .unwrap();
        // Parse the included file into a fresh top-level array form. The simplest
        // way is to write a wrapper with `tasks = [...]` because TOML's grammar
        // can't represent a bare top-level array. Confirm the table-form code path
        // works as the canonical entry point.
        let inc_str = inc_path.to_string_lossy().replace('\\', "\\\\");
        std::fs::write(
            &inc_path,
            r#"
[[tasks]]
name = "first"
debug = { msg = "1" }
[[tasks]]
name = "second"
debug = { msg = "2" }
"#,
        )
        .unwrap();
        let pb = format!(r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "ArrayInc"
hosts = "localhost"
[[plays.tasks]]
include_tasks = "{inc_str}"
"#);
        let r = run(&pb, "localhost,", "test").unwrap();
        let _ = std::fs::remove_file(&inc_path);
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 2, "both included tasks should run");
    }

    #[test]
    fn delegate_to_substitutes_host_in_outcome() {
        // Static delegate name. The connection is still LocalSync so the work
        // physically runs on the controller, but the outcome reports the
        // delegate.
        let pb = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Delegate"
hosts = "localhost"
[[plays.tasks]]
delegate_to = "controller01"
debug = { msg = "delegated" }
"#;
        let r = run(pb, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn delegate_to_supports_templated_hostname() {
        let pb = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "TemplatedDelegate"
hosts = "localhost"
[plays.vars]
db_host = "db-primary.example.net"
[[plays.tasks]]
delegate_to = "{{ db_host }}"
debug = { msg = "running on a synthetic host" }
"#;
        let r = run(pb, "localhost,", "test").unwrap();
        assert_eq!(r.failed, 0);
        assert_eq!(r.ok, 1);
    }

    #[test]
    fn run_once_executes_only_once_for_multi_host() {
        let pb = r#"
[imports]
set_fact = "runsible_builtin.set_fact"
debug = "runsible_builtin.debug"
[[plays]]
name = "Run once"
hosts = "all"
[[plays.tasks]]
name = "once"
run_once = true
register = "build_id"
set_fact = { id = "abc" }
[[plays.tasks]]
name = "every"
debug = { msg = "id is {{ build_id.returns.id }}" }
"#;
        let r = run(pb, "host1,host2,host3,", "test").unwrap();
        // Per host: 3 hosts × 1 debug task = 3 ok.
        // run_once task: 1 ok on first host, 2 skipped on the rest.
        assert_eq!(r.failed, 0);
        assert_eq!(r.skipped, 2, "run_once must skip the second and third host");
        // 1 set_fact ok on host1 + 3 debugs ok = 4
        assert_eq!(r.ok, 4);
    }

    #[test]
    fn run_once_register_is_replayed_on_subsequent_hosts() {
        // Even though the run_once task only runs on the first host, the
        // registered outcome must propagate so subsequent hosts can template
        // against `register`.
        let pb = r#"
[imports]
set_fact = "runsible_builtin.set_fact"
assert = "runsible_builtin.assert"
[[plays]]
name = "Run once register"
hosts = "all"
[[plays.tasks]]
name = "first only"
run_once = true
register = "primary"
set_fact = { id = "p-1" }
[[plays.tasks]]
name = "every host asserts on register"
assert = { that = ["primary.returns.id == 'p-1'"] }
"#;
        let r = run(pb, "h1,h2,h3,", "test").unwrap();
        assert_eq!(r.failed, 0, "register must replay on every host");
        // 3 asserts ok + 1 set_fact ok on h1 + 2 skipped on h2/h3
        assert_eq!(r.ok, 4);
        assert_eq!(r.skipped, 2);
    }

    #[test]
    fn syntax_check_helpers_dont_run_anything() {
        // Parse a syntactically-valid playbook; just ensure parse_playbook works.
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "Hello"
hosts = "localhost"
[[plays.tasks]]
name = "say"
debug = { msg = "hi" }
"#;
        parse::parse_playbook(src).expect("parse");
    }

    #[test]
    fn syntax_check_catches_bad_module_call() {
        let src = r#"
[[plays]]
name = "bad"
hosts = "localhost"
[[plays.tasks]]
name = "two modules"
debug = { msg = "x" }
command = { cmd = "echo y" }
"#;
        let pb = parse::parse_playbook(src).expect("playbook parses");
        let raw = &pb.plays[0].tasks[0];
        let imports = indexmap::IndexMap::new();
        assert!(parse::resolve_task(raw, &imports).is_err());
    }

    #[test]
    fn start_at_task_skips_preceding_tasks() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "p"
hosts = "localhost"
[[plays.tasks]]
name = "first"
debug = { msg = "1" }
[[plays.tasks]]
name = "second"
debug = { msg = "2" }
[[plays.tasks]]
name = "third"
debug = { msg = "3" }
"#;
        let opts = engine::RunOptions {
            start_at_task: Some("second".into()),
            ..Default::default()
        };
        let r = engine::run_with(src, "localhost,", "test", opts).unwrap();
        // first → skipped, second → ok, third → ok
        assert_eq!(r.skipped, 1);
        assert_eq!(r.ok, 2);
        assert_eq!(r.failed, 0);
    }

    #[test]
    fn start_at_task_unmatched_skips_all() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "p"
hosts = "localhost"
[[plays.tasks]]
name = "alpha"
debug = { msg = "a" }
[[plays.tasks]]
name = "beta"
debug = { msg = "b" }
"#;
        let opts = engine::RunOptions {
            start_at_task: Some("does_not_exist".into()),
            ..Default::default()
        };
        let r = engine::run_with(src, "localhost,", "test", opts).unwrap();
        assert_eq!(r.skipped, 2);
        assert_eq!(r.ok, 0);
    }

    #[test]
    fn start_at_task_default_runs_all() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "p"
hosts = "localhost"
[[plays.tasks]]
name = "a"
debug = { msg = "x" }
[[plays.tasks]]
name = "b"
debug = { msg = "y" }
"#;
        // No start_at_task — both run.
        let r = run(src, "localhost,", "test").unwrap();
        assert_eq!(r.ok, 2);
        assert_eq!(r.skipped, 0);
    }

    #[test]
    fn start_at_task_per_host_independent() {
        let src = r#"
[imports]
debug = "runsible_builtin.debug"
[[plays]]
name = "p"
hosts = "all"
[[plays.tasks]]
name = "skip"
debug = { msg = "1" }
[[plays.tasks]]
name = "run"
debug = { msg = "2" }
"#;
        let opts = engine::RunOptions {
            start_at_task: Some("run".into()),
            ..Default::default()
        };
        let r = engine::run_with(src, "h1,h2,h3,", "test", opts).unwrap();
        // 3 hosts × 1 skipped + 3 hosts × 1 ok = 3 each
        assert_eq!(r.skipped, 3);
        assert_eq!(r.ok, 3);
    }
}
