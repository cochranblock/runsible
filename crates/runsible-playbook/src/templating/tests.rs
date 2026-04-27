//! Tests for the templating subsystem — both the original render/eval suite
//! and the Ansible-compatible filter/test/lookup catalog.

use super::core::Templater;
use runsible_core::types::Vars;

fn vars_from_toml(src: &str) -> Vars {
    let v: toml::Value = toml::from_str(src).expect("toml parse");
    let tbl = v.as_table().expect("top-level table");
    tbl.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// One-shot render helper for filter assertions.
fn render(src: &str) -> String {
    let t = Templater::new();
    t.render_str(src, &Vars::new()).unwrap()
}

fn render_with(src: &str, vars: &Vars) -> String {
    let t = Templater::new();
    t.render_str(src, vars).unwrap()
}

// ---------------------------------------------------------------------------
// Original render/eval tests (preserved from src/templating.rs)
// ---------------------------------------------------------------------------

#[test]
fn render_simple_var() {
    let t = Templater::new();
    let vars = vars_from_toml(r#"name = "alice""#);
    let out = t.render_str("{{ name }}", &vars).unwrap();
    assert_eq!(out, "alice");
}

#[test]
fn render_no_template_passthrough() {
    let t = Templater::new();
    let vars = Vars::new();
    let out = t.render_str("no template here", &vars).unwrap();
    assert_eq!(out, "no template here");
}

#[test]
fn render_nested_table() {
    let t = Templater::new();
    let vars = vars_from_toml(r#"name = "alice""#);
    let value: toml::Value = toml::from_str(
        r#"
msg = "hi {{ name }}"
port = 80
"#,
    )
    .unwrap();
    let rendered = t.render_value(&value, &vars).unwrap();
    let tbl = rendered.as_table().unwrap();
    assert_eq!(tbl["msg"].as_str().unwrap(), "hi alice");
    assert_eq!(tbl["port"].as_integer().unwrap(), 80);
}

#[test]
fn render_array_of_strings() {
    let t = Templater::new();
    let vars = vars_from_toml(
        r#"
a = "1"
b = "2"
"#,
    );
    let value: toml::Value = toml::from_str(r#"items = ["{{ a }}", "{{ b }}"]"#).unwrap();
    let rendered = t.render_value(&value, &vars).unwrap();
    let arr = rendered.as_table().unwrap()["items"].as_array().unwrap();
    assert_eq!(arr[0].as_str().unwrap(), "1");
    assert_eq!(arr[1].as_str().unwrap(), "2");
}

#[test]
fn eval_bool_true() {
    let t = Templater::new();
    let vars = vars_from_toml("x = 1");
    assert!(t.eval_bool("x == 1", &vars).unwrap());
}

#[test]
fn eval_bool_false() {
    let t = Templater::new();
    let vars = vars_from_toml("x = 2");
    assert!(!t.eval_bool("x == 1", &vars).unwrap());
}

#[test]
fn eval_bool_and() {
    let t = Templater::new();
    let vars = vars_from_toml(
        r#"
x = 1
y = 2
"#,
    );
    assert!(t.eval_bool("x == 1 and y == 2", &vars).unwrap());
}

#[test]
fn eval_bool_in_string() {
    let t = Templater::new();
    let vars = vars_from_toml(r#"s = "foobar""#);
    assert!(t.eval_bool("'foo' in s", &vars).unwrap());
}

#[test]
fn missing_var_errors() {
    let t = Templater::new();
    let vars = Vars::new();
    let err = t.render_str("{{ undefined }}", &vars).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("undefined"),
        "expected error to mention 'undefined', got: {msg}"
    );
}

#[test]
fn render_int_var() {
    let t = Templater::new();
    let vars = vars_from_toml("port = 8080");
    let out = t.render_str("{{ port }}", &vars).unwrap();
    assert_eq!(out, "8080");
}

#[test]
fn render_bool_var() {
    let t = Templater::new();
    let vars = vars_from_toml("enabled = true");
    let out = t.render_str("{{ enabled }}", &vars).unwrap();
    assert_eq!(out, "true");
}

#[test]
fn render_float_var() {
    let t = Templater::new();
    let vars = vars_from_toml("ratio = 1.5");
    let out = t.render_str("{{ ratio }}", &vars).unwrap();
    assert!(out.starts_with("1.5"));
}

#[test]
fn render_nested_table_access() {
    let t = Templater::new();
    let vars = vars_from_toml(
        r#"
[user]
name = "alice"
age = 30
"#,
    );
    let out = t.render_str("{{ user.name }}", &vars).unwrap();
    assert_eq!(out, "alice");
}

#[test]
fn render_array_index() {
    let t = Templater::new();
    let vars = vars_from_toml(r#"items = ["a", "b", "c"]"#);
    let out = t.render_str("{{ items[0] }}", &vars).unwrap();
    assert_eq!(out, "a");
    let out2 = t.render_str("{{ items[2] }}", &vars).unwrap();
    assert_eq!(out2, "c");
}

#[test]
fn render_default_filter() {
    let t = Templater::new();
    let vars = Vars::new();
    let out = t
        .render_str("{{ missing | default('x') }}", &vars)
        .unwrap();
    assert_eq!(out, "x");
}

#[test]
fn render_length_filter_on_array() {
    let t = Templater::new();
    let vars = vars_from_toml(r#"items = ["a", "b", "c"]"#);
    let out = t.render_str("{{ items | length }}", &vars).unwrap();
    assert_eq!(out, "3");
}

#[test]
fn render_upper_filter() {
    let t = Templater::new();
    let vars = vars_from_toml(r#"who = "alice""#);
    let out = t.render_str("{{ who | upper }}", &vars).unwrap();
    assert_eq!(out, "ALICE");
}

#[test]
fn render_lower_filter() {
    let t = Templater::new();
    let vars = vars_from_toml(r#"who = "ALICE""#);
    let out = t.render_str("{{ who | lower }}", &vars).unwrap();
    assert_eq!(out, "alice");
}

#[test]
fn eval_bool_parenthesized() {
    let t = Templater::new();
    let vars = vars_from_toml(
        r#"
a = 1
b = 2
c = 3
"#,
    );
    assert!(t.eval_bool("(a == 1 and b == 2) or c == 99", &vars).unwrap());
    assert!(t.eval_bool("a == 1 and (b == 2 or c == 99)", &vars).unwrap());
}

#[test]
fn eval_bool_not_operator() {
    let t = Templater::new();
    let vars = vars_from_toml("x = 5");
    assert!(t.eval_bool("not (x == 1)", &vars).unwrap());
    assert!(!t.eval_bool("not (x == 5)", &vars).unwrap());
}

#[test]
fn render_value_array_of_templates() {
    let t = Templater::new();
    let vars = vars_from_toml(
        r#"
a = "first"
b = "second"
"#,
    );
    let raw: toml::Value = toml::Value::Array(vec![
        toml::Value::String("{{ a }}".into()),
        toml::Value::String("{{ b }}".into()),
    ]);
    let rendered = t.render_value(&raw, &vars).unwrap();
    let arr = rendered.as_array().unwrap();
    assert_eq!(arr[0].as_str().unwrap(), "first");
    assert_eq!(arr[1].as_str().unwrap(), "second");
}

// ---------------------------------------------------------------------------
// Filter catalog tests
// ---------------------------------------------------------------------------

#[test]
fn filter_bool_yes() {
    assert_eq!(render("{{ 'yes' | bool }}"), "true");
}

#[test]
fn filter_bool_off() {
    assert_eq!(render("{{ 'off' | bool }}"), "false");
}

#[test]
fn filter_bool_int_nonzero() {
    assert_eq!(render("{{ 7 | bool }}"), "true");
}

#[test]
fn filter_quote_basic() {
    assert_eq!(render("{{ 'hello world' | quote }}"), "'hello world'");
}

#[test]
fn filter_quote_with_inner_quote() {
    assert_eq!(render("{{ \"it's\" | quote }}"), "'it'\\''s'");
}

#[test]
fn filter_regex_replace_basic() {
    assert_eq!(
        render(r#"{{ 'hello world' | regex_replace('world', 'rust') }}"#),
        "hello rust"
    );
}

#[test]
fn filter_regex_replace_capture() {
    assert_eq!(
        render(r#"{{ 'foo42' | regex_replace('foo(\d+)', 'bar$1') }}"#),
        "bar42"
    );
}

#[test]
fn filter_regex_search_match() {
    assert_eq!(
        render(r#"{{ 'abc123def' | regex_search('\d+') }}"#),
        "123"
    );
}

#[test]
fn filter_regex_search_no_match() {
    assert_eq!(render(r#"{{ 'abc' | regex_search('\d+') }}"#), "");
}

#[test]
fn filter_regex_findall_collects() {
    let out = render(r#"{{ 'a1 b2 c3' | regex_findall('\d') | length }}"#);
    assert_eq!(out, "3");
}

#[test]
fn filter_regex_escape_quotes_metachars() {
    let out = render(r#"{{ 'a.b*c' | regex_escape }}"#);
    assert!(out.contains("\\.") && out.contains("\\*"));
}

#[test]
fn filter_comment_default() {
    assert_eq!(render(r#"{{ 'hi' | comment }}"#), "# hi");
}

#[test]
fn filter_comment_erlang() {
    assert_eq!(render(r#"{{ 'hi' | comment('erlang') }}"#), "% hi");
}

#[test]
fn filter_password_hash_deterministic() {
    let a = render(r#"{{ 'pw' | password_hash('sha512', 'mysalt') }}"#);
    let b = render(r#"{{ 'pw' | password_hash('sha512', 'mysalt') }}"#);
    assert_eq!(a, b);
    assert!(a.starts_with("$6$mysalt$"));
}

#[test]
fn filter_password_hash_default_salt() {
    let out = render(r#"{{ 'x' | password_hash('sha512') }}"#);
    assert!(out.starts_with("$6$rsl$"));
}

#[test]
fn filter_hash_sha256() {
    // sha256("hi") = 8f434346648f6b96df89dda901c5176b10a6d83961dd3c1ac88b59b2dc327aa4
    let out = render(r#"{{ 'hi' | hash('sha256') }}"#);
    assert_eq!(
        out,
        "8f434346648f6b96df89dda901c5176b10a6d83961dd3c1ac88b59b2dc327aa4"
    );
}

#[test]
fn filter_checksum_default_sha256() {
    let a = render(r#"{{ 'abc' | checksum }}"#);
    assert_eq!(
        a,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn filter_b64encode() {
    assert_eq!(render(r#"{{ 'hi' | b64encode }}"#), "aGk=");
}

#[test]
fn filter_b64decode() {
    assert_eq!(render(r#"{{ 'aGk=' | b64decode }}"#), "hi");
}

#[test]
fn filter_to_json() {
    let vars = vars_from_toml(r#"x = "alice""#);
    let out = render_with(r#"{{ x | to_json }}"#, &vars);
    assert_eq!(out, "\"alice\"");
}

#[test]
fn filter_from_json() {
    let out = render(r#"{{ ('{"k": 5}' | from_json).k }}"#);
    assert_eq!(out, "5");
}

#[test]
fn filter_to_nice_json_pretty() {
    let vars = vars_from_toml(r#"x = "y""#);
    let out = render_with(r#"{{ x | to_nice_json }}"#, &vars);
    assert!(out.contains('\"'));
}

#[test]
fn filter_from_yaml_then_access() {
    let out = render(r#"{{ ('a: 1' | from_yaml).a }}"#);
    assert_eq!(out, "1");
}

#[test]
fn filter_to_yaml() {
    let vars = vars_from_toml(r#"name = "alice""#);
    let out = render_with(r#"{{ name | to_yaml }}"#, &vars);
    assert!(out.contains("alice"));
}

#[test]
fn filter_to_nice_yaml() {
    let vars = vars_from_toml(r#"name = "alice""#);
    let out = render_with(r#"{{ name | to_nice_yaml }}"#, &vars);
    assert!(out.contains("alice"));
}

#[test]
fn filter_urlsplit_scheme_and_path() {
    assert_eq!(
        render(r#"{{ ('https://example.com:8080/p?q=1#f' | urlsplit).scheme }}"#),
        "https"
    );
    assert_eq!(
        render(r#"{{ ('https://example.com:8080/p?q=1#f' | urlsplit).hostname }}"#),
        "example.com"
    );
    assert_eq!(
        render(r#"{{ ('https://example.com:8080/p?q=1#f' | urlsplit).port }}"#),
        "8080"
    );
    assert_eq!(
        render(r#"{{ ('https://example.com:8080/p?q=1#f' | urlsplit).path }}"#),
        "/p"
    );
    assert_eq!(
        render(r#"{{ ('https://example.com:8080/p?q=1#f' | urlsplit).query }}"#),
        "q=1"
    );
    assert_eq!(
        render(r#"{{ ('https://example.com:8080/p?q=1#f' | urlsplit).fragment }}"#),
        "f"
    );
}

#[test]
fn filter_urlsplit_userinfo() {
    assert_eq!(
        render(r#"{{ ('http://u:pw@host/p' | urlsplit).username }}"#),
        "u"
    );
    assert_eq!(
        render(r#"{{ ('http://u:pw@host/p' | urlsplit).password }}"#),
        "pw"
    );
}

#[test]
fn filter_expanduser_replaces_tilde() {
    std::env::set_var("HOME", "/home/runsible");
    assert_eq!(render(r#"{{ '~/x' | expanduser }}"#), "/home/runsible/x");
    assert_eq!(render(r#"{{ '~' | expanduser }}"#), "/home/runsible");
}

#[test]
fn filter_expandvars_replaces_dollar() {
    std::env::set_var("RSL_TEST_VAR", "VALUE");
    assert_eq!(
        render(r#"{{ '$RSL_TEST_VAR/path' | expandvars }}"#),
        "VALUE/path"
    );
    assert_eq!(
        render(r#"{{ '${RSL_TEST_VAR}/path' | expandvars }}"#),
        "VALUE/path"
    );
}

#[test]
fn filter_basename() {
    assert_eq!(render(r#"{{ '/etc/hosts' | basename }}"#), "hosts");
}

#[test]
fn filter_dirname() {
    assert_eq!(render(r#"{{ '/etc/hosts' | dirname }}"#), "/etc");
}

#[test]
fn filter_splitext_returns_pair() {
    let out = render(r#"{{ ('/etc/x.conf' | splitext)[1] }}"#);
    assert_eq!(out, ".conf");
}

#[test]
fn filter_realpath_passthrough_on_nonexistent() {
    let out = render(r#"{{ '/this/path/does/not/exist/zz' | realpath }}"#);
    assert_eq!(out, "/this/path/does/not/exist/zz");
}

#[test]
fn filter_dict2items() {
    let vars = vars_from_toml(
        r#"
[d]
a = 1
b = 2
"#,
    );
    let out = render_with(r#"{{ (d | dict2items) | length }}"#, &vars);
    assert_eq!(out, "2");
    let key = render_with(r#"{{ (d | dict2items)[0].key }}"#, &vars);
    assert_eq!(key, "a");
}

#[test]
fn filter_items2dict_round_trip() {
    let vars = vars_from_toml(
        r#"
[d]
a = "x"
b = "y"
"#,
    );
    let out = render_with(
        r#"{{ ((d | dict2items) | items2dict).a }}"#,
        &vars,
    );
    assert_eq!(out, "x");
}

#[test]
fn filter_combine_merges() {
    let vars = vars_from_toml(
        r#"
[a]
x = 1
y = 2
[b]
y = 99
z = 3
"#,
    );
    let out = render_with(r#"{{ (a | combine(b)).y }}"#, &vars);
    assert_eq!(out, "99");
    let z = render_with(r#"{{ (a | combine(b)).z }}"#, &vars);
    assert_eq!(z, "3");
}

#[test]
fn filter_flatten_default() {
    let vars = vars_from_toml(r#"x = [[1, 2], [3, 4]]"#);
    let out = render_with(r#"{{ x | flatten | length }}"#, &vars);
    assert_eq!(out, "4");
}

#[test]
fn filter_unique_preserves_order() {
    let vars = vars_from_toml(r#"x = [1, 2, 1, 3, 2]"#);
    let out = render_with(r#"{{ (x | unique) | length }}"#, &vars);
    assert_eq!(out, "3");
}

#[test]
fn filter_intersect() {
    let vars = vars_from_toml(
        r#"
a = [1, 2, 3]
b = [2, 3, 4]
"#,
    );
    let out = render_with(r#"{{ (a | intersect(b)) | length }}"#, &vars);
    assert_eq!(out, "2");
}

#[test]
fn filter_union() {
    let vars = vars_from_toml(
        r#"
a = [1, 2, 3]
b = [3, 4]
"#,
    );
    let out = render_with(r#"{{ (a | union(b)) | length }}"#, &vars);
    assert_eq!(out, "4");
}

#[test]
fn filter_difference() {
    let vars = vars_from_toml(
        r#"
a = [1, 2, 3]
b = [2]
"#,
    );
    let out = render_with(r#"{{ (a | difference(b)) | length }}"#, &vars);
    assert_eq!(out, "2");
}

#[test]
fn filter_symmetric_difference() {
    let vars = vars_from_toml(
        r#"
a = [1, 2, 3]
b = [3, 4]
"#,
    );
    let out = render_with(r#"{{ (a | symmetric_difference(b)) | length }}"#, &vars);
    assert_eq!(out, "3");
}

#[test]
fn filter_random_picks_some() {
    let vars = vars_from_toml(r#"x = [1, 2, 3]"#);
    let a = render_with(r#"{{ x | random }}"#, &vars);
    let b = render_with(r#"{{ x | random }}"#, &vars);
    assert_eq!(a, b, "deterministic random must be reproducible");
}

#[test]
fn filter_shuffle_reproducible() {
    let vars = vars_from_toml(r#"x = ["a", "b", "c", "d"]"#);
    let a = render_with(r#"{{ x | shuffle | length }}"#, &vars);
    assert_eq!(a, "4");
    let b = render_with(r#"{{ x | shuffle | length }}"#, &vars);
    assert_eq!(b, "4");
}

#[test]
fn filter_zip_pairs() {
    let vars = vars_from_toml(
        r#"
a = [1, 2, 3]
b = ["x", "y", "z"]
"#,
    );
    let out = render_with(r#"{{ (a | zip(b)) | length }}"#, &vars);
    assert_eq!(out, "3");
    let val = render_with(r#"{{ (a | zip(b))[0][1] }}"#, &vars);
    assert_eq!(val, "x");
}

#[test]
fn filter_subelements_nested() {
    let vars = vars_from_toml(
        r#"
[[hosts]]
name = "h1"
ports = [80, 443]

[[hosts]]
name = "h2"
ports = [22]
"#,
    );
    let out = render_with(r#"{{ (hosts | subelements('ports')) | length }}"#, &vars);
    assert_eq!(out, "3");
}

#[test]
fn filter_string_coerce() {
    assert_eq!(render(r#"{{ 42 | string }}"#), "42");
}

#[test]
fn filter_str_alias() {
    assert_eq!(render(r#"{{ 42 | str }}"#), "42");
}

#[test]
fn filter_int_coerce() {
    assert_eq!(render(r#"{{ '42' | int }}"#), "42");
}

#[test]
fn filter_mandatory_passes_when_defined() {
    let vars = vars_from_toml(r#"x = "hello""#);
    let out = render_with(r#"{{ x | mandatory }}"#, &vars);
    assert_eq!(out, "hello");
}

#[test]
fn filter_mandatory_errors_on_undefined() {
    let t = Templater::new();
    let err = t
        .render_str("{{ does_not_exist | mandatory }}", &Vars::new())
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.to_lowercase().contains("undefined") || msg.contains("mandatory"));
}

#[test]
fn filter_ternary_true() {
    assert_eq!(render(r#"{{ true | ternary('yes', 'no') }}"#), "yes");
}

#[test]
fn filter_ternary_false() {
    assert_eq!(render(r#"{{ false | ternary('yes', 'no') }}"#), "no");
}

#[test]
fn filter_default_value() {
    let vars = Vars::new();
    let out = render_with(r#"{{ missing | default('fallback') }}"#, &vars);
    assert_eq!(out, "fallback");
}

#[test]
fn filter_min_max() {
    let vars = vars_from_toml(r#"x = [3, 1, 4, 1, 5, 9]"#);
    assert_eq!(render_with(r#"{{ x | min }}"#, &vars), "1");
    assert_eq!(render_with(r#"{{ x | max }}"#, &vars), "9");
}

// ---------------------------------------------------------------------------
// Test catalog tests (`x is foo`)
// ---------------------------------------------------------------------------

#[test]
fn test_defined() {
    let t = Templater::new();
    let vars = vars_from_toml(r#"x = 1"#);
    assert!(t.eval_bool("x is defined", &vars).unwrap());
    assert!(!t.eval_bool("y is defined", &vars).unwrap());
}

#[test]
fn test_undefined() {
    let t = Templater::new();
    let vars = vars_from_toml(r#"x = 1"#);
    assert!(t.eval_bool("y is undefined", &vars).unwrap());
    assert!(!t.eval_bool("x is undefined", &vars).unwrap());
}

#[test]
fn test_string_kind() {
    let t = Templater::new();
    let vars = vars_from_toml(
        r#"
s = "hi"
n = 1
"#,
    );
    assert!(t.eval_bool("s is string", &vars).unwrap());
    assert!(!t.eval_bool("n is string", &vars).unwrap());
}

#[test]
fn test_number_kind() {
    let t = Templater::new();
    let vars = vars_from_toml(
        r#"
s = "hi"
n = 7
"#,
    );
    assert!(t.eval_bool("n is number", &vars).unwrap());
    assert!(!t.eval_bool("s is number", &vars).unwrap());
}

#[test]
fn test_sequence_kind() {
    let t = Templater::new();
    let vars = vars_from_toml(r#"x = [1, 2, 3]"#);
    assert!(t.eval_bool("x is sequence", &vars).unwrap());
}

#[test]
fn test_mapping_kind() {
    let t = Templater::new();
    let vars = vars_from_toml(
        r#"
[m]
a = 1
"#,
    );
    assert!(t.eval_bool("m is mapping", &vars).unwrap());
}

#[test]
fn test_match_full() {
    let t = Templater::new();
    let vars = vars_from_toml(r#"s = "hello""#);
    assert!(t.eval_bool("s is match('hel.*')", &vars).unwrap());
    assert!(!t.eval_bool("s is match('foo')", &vars).unwrap());
}

#[test]
fn test_search_partial() {
    let t = Templater::new();
    let vars = vars_from_toml(r#"s = "abcdef""#);
    assert!(t.eval_bool("s is search('cde')", &vars).unwrap());
}

#[test]
fn test_version_gt() {
    let t = Templater::new();
    let vars = vars_from_toml(r#"v = "1.2.3""#);
    assert!(t.eval_bool("v is version('1.0', '>')", &vars).unwrap());
    assert!(!t.eval_bool("v is version('2.0', '>')", &vars).unwrap());
}

#[test]
fn test_succeeded_outcome() {
    let t = Templater::new();
    let vars = vars_from_toml(
        r#"
[r]
status = "ok"
"#,
    );
    assert!(t.eval_bool("r is succeeded", &vars).unwrap());
    assert!(t.eval_bool("r is success", &vars).unwrap());
}

#[test]
fn test_failed_outcome() {
    let t = Templater::new();
    let vars = vars_from_toml(
        r#"
[r]
status = "failed"
"#,
    );
    assert!(t.eval_bool("r is failed", &vars).unwrap());
    assert!(t.eval_bool("r is failure", &vars).unwrap());
}

#[test]
fn test_changed_outcome() {
    let t = Templater::new();
    let vars = vars_from_toml(
        r#"
[r]
status = "changed"
"#,
    );
    assert!(t.eval_bool("r is changed", &vars).unwrap());
    assert!(!t.eval_bool("r is failed", &vars).unwrap());
}

#[test]
fn test_skipped_outcome() {
    let t = Templater::new();
    let vars = vars_from_toml(
        r#"
[r]
status = "skipped"
"#,
    );
    assert!(t.eval_bool("r is skipped", &vars).unwrap());
}

#[test]
fn test_none_value() {
    let t = Templater::new();
    let vars = Vars::new();
    assert!(t.eval_bool("none is none", &vars).unwrap());
}

// ---------------------------------------------------------------------------
// Lookup catalog tests
// ---------------------------------------------------------------------------

#[test]
fn lookup_env_dispatch_default() {
    let out = render(r#"{{ lookup('env', 'NONEXISTENT_RSL_TEST_XYZ', 'fallback') }}"#);
    assert_eq!(out, "fallback");
}

#[test]
fn lookup_env_direct_with_default() {
    let out = render(r#"{{ env('NONEXISTENT_RSL_TEST_XYZ', 'fb') }}"#);
    assert_eq!(out, "fb");
}

#[test]
fn lookup_env_real_var() {
    std::env::set_var("RSL_TEST_LOOKUP_ENV", "hello");
    let out = render(r#"{{ lookup('env', 'RSL_TEST_LOOKUP_ENV') }}"#);
    assert_eq!(out, "hello");
}

#[test]
fn lookup_pipe_runs_echo() {
    let out = render(r#"{{ lookup('pipe', 'echo hi there') }}"#);
    assert_eq!(out, "hi there");
}

#[test]
fn lookup_pipe_direct() {
    let out = render(r#"{{ pipe('echo direct') }}"#);
    assert_eq!(out, "direct");
}

#[test]
fn lookup_lines_splits_output() {
    let out = render(r#"{{ lookup('lines', 'printf "a\nb\nc"') | length }}"#);
    assert_eq!(out, "3");
}

#[test]
fn lookup_password_deterministic() {
    let a = render(r#"{{ lookup('password', '/tmp/somepath length=10') }}"#);
    let b = render(r#"{{ lookup('password', '/tmp/somepath length=10') }}"#);
    assert_eq!(a, b, "password lookup must be deterministic");
    assert_eq!(a.len(), 10);
}

#[test]
fn lookup_password_charset_digits() {
    let out = render(r#"{{ lookup('password', '/tmp/x length=8 chars=digits') }}"#);
    assert_eq!(out.len(), 8);
    assert!(out.chars().all(|c| c.is_ascii_digit()));
}

#[test]
fn lookup_items_flattens() {
    let vars = vars_from_toml(r#"xs = [1, 2, 3]"#);
    let out = render_with(r#"{{ (lookup('items', xs)) | length }}"#, &vars);
    assert_eq!(out, "3");
}

#[test]
fn lookup_indexed_items_pairs() {
    let vars = vars_from_toml(r#"xs = ["a", "b"]"#);
    let out = render_with(r#"{{ (lookup('indexed_items', xs))[1][0] }}"#, &vars);
    assert_eq!(out, "1");
}

#[test]
fn lookup_file_reads_a_file() {
    let path = std::env::temp_dir().join(format!("rsl-lookup-file-{}.txt", std::process::id()));
    std::fs::write(&path, "hello-from-file").unwrap();
    let path_s = path.to_string_lossy().to_string();
    let src = format!(r#"{{{{ lookup('file', '{path_s}') }}}}"#);
    let out = render(&src);
    let _ = std::fs::remove_file(&path);
    assert_eq!(out, "hello-from-file");
}

#[test]
fn lookup_first_found_picks_existing() {
    let path = std::env::temp_dir().join(format!("rsl-firstfound-{}.txt", std::process::id()));
    std::fs::write(&path, "x").unwrap();
    let path_s = path.to_string_lossy().to_string();
    let src = format!(
        r#"{{{{ lookup('first_found', ['/no/such/path/abc', '{path_s}']) }}}}"#
    );
    let out = render(&src);
    let _ = std::fs::remove_file(&path);
    assert_eq!(out, path_s);
}

#[test]
fn lookup_fileglob_finds_temp_file() {
    let dir = std::env::temp_dir().join(format!("rsl-fileglob-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.conf"), "a").unwrap();
    std::fs::write(dir.join("b.conf"), "b").unwrap();
    std::fs::write(dir.join("c.txt"), "c").unwrap();
    let dir_s = dir.to_string_lossy().to_string();
    let src = format!(r#"{{{{ (lookup('fileglob', '{dir_s}/*.conf')) | length }}}}"#);
    let out = render(&src);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(out, "2");
}

#[test]
fn lookup_unknown_errors() {
    let t = Templater::new();
    let err = t
        .render_str("{{ lookup('does_not_exist', 'x') }}", &Vars::new())
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.to_lowercase().contains("unknown"));
}

#[test]
fn lookup_query_alias() {
    std::env::set_var("RSL_TEST_QUERY_ALIAS", "ok");
    let out = render(r#"{{ query('env', 'RSL_TEST_QUERY_ALIAS') }}"#);
    assert_eq!(out, "ok");
}

#[test]
fn lookup_vars_echoes_name() {
    let out = render(r#"{{ lookup('vars', 'someName') }}"#);
    assert_eq!(out, "someName");
}
