//! Ansible-compatible filter and test catalog.
//!
//! Most Ansible filters/tests are registered here via
//! [`register_filters_and_tests`]. Implementations are deliberately
//! pure-Rust and side-effect free where possible. Where Ansible's behavior
//! is non-deterministic (random/shuffle, password generation), we substitute
//! a deterministic seed derived from the input so tests are reproducible.
//!
//! Adding a filter? Drop a closure on `env.add_filter(...)` and a unit test
//! in [`super::tests`].

use std::collections::{BTreeMap, BTreeSet};

use base64::Engine as _;
use minijinja::value::{Kwargs, Rest, ValueKind};
use minijinja::{Environment, Error, ErrorKind, Value as JValue};
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512};

// ---------- helpers ----------

fn invalid<S: Into<String>>(msg: S) -> Error {
    Error::new(ErrorKind::InvalidOperation, msg.into())
}

fn jvalue_to_json(v: &JValue) -> serde_json::Value {
    serde_json::to_value(v).unwrap_or(serde_json::Value::Null)
}

fn json_to_jvalue(v: &serde_json::Value) -> JValue {
    JValue::from_serialize(v)
}

fn to_string_loose(v: &JValue) -> String {
    if let Some(s) = v.as_str() {
        s.to_string()
    } else if v.is_undefined() || v.is_none() {
        String::new()
    } else {
        format!("{v}")
    }
}

/// Coerce arbitrary JValue → bool (Ansible-style truthiness for the `bool`
/// filter). Non-strings: numbers > 0 are true, 0 is false; bool passes
/// through; undefined/none are false.
fn coerce_bool(v: &JValue) -> bool {
    if v.is_undefined() || v.is_none() {
        return false;
    }
    if let Some(s) = v.as_str() {
        return matches!(
            s.to_ascii_lowercase().as_str(),
            "true" | "yes" | "on" | "1" | "y" | "t"
        );
    }
    if matches!(v.kind(), ValueKind::Bool) {
        return v.is_true();
    }
    if let Some(i) = v.as_i64() {
        return i != 0;
    }
    if let Ok(f) = f64::try_from(v.clone()) {
        return f != 0.0;
    }
    v.is_true()
}

/// Hash a string-keyed input deterministically into a u64 for seeding LCG /
/// password generation. Uses SHA-256 truncated to 8 bytes — overkill for the
/// use case but keeps the impl short.
fn hash_seed(input: &str) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    u64::from_le_bytes(bytes)
}

/// Tiny LCG. `next` advances state and returns a value in `0..n`.
struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self {
            // Avoid a zero seed which would lock the LCG at 0 forever.
            state: seed.wrapping_add(0xa1b2_c3d4_e5f6_0789),
        }
    }

    fn next(&mut self) -> u64 {
        // Numerical Recipes constants.
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    fn pick(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next() % n as u64) as usize
    }
}

/// Hash digest helper used by `hash`/`checksum`/`password_hash`.
fn digest_hex(algo: &str, input: &str) -> Result<String, Error> {
    Ok(match algo {
        "sha224" => hex::encode(Sha224::digest(input.as_bytes())),
        "sha256" => hex::encode(Sha256::digest(input.as_bytes())),
        "sha384" => hex::encode(Sha384::digest(input.as_bytes())),
        "sha512" => hex::encode(Sha512::digest(input.as_bytes())),
        // md5/sha1 are not in workspace deps; emit sha256 as fallback so
        // templates that name them don't crash.
        "md5" | "sha1" => hex::encode(Sha256::digest(input.as_bytes())),
        other => return Err(invalid(format!("unknown hash algorithm: {other}"))),
    })
}

// ---------- string filters ----------

fn filter_bool(v: JValue) -> bool {
    coerce_bool(&v)
}

fn filter_quote(v: JValue) -> String {
    let s = to_string_loose(&v);
    let escaped = s.replace('\'', "'\\''");
    format!("'{escaped}'")
}

fn filter_regex_replace(s: String, pattern: String, replacement: String) -> Result<String, Error> {
    let re = regex::Regex::new(&pattern)
        .map_err(|e| invalid(format!("regex_replace: bad pattern: {e}")))?;
    Ok(re.replace_all(&s, replacement.as_str()).into_owned())
}

fn filter_regex_search(s: String, pattern: String) -> Result<String, Error> {
    let re = regex::Regex::new(&pattern)
        .map_err(|e| invalid(format!("regex_search: bad pattern: {e}")))?;
    Ok(re.find(&s).map(|m| m.as_str().to_string()).unwrap_or_default())
}

fn filter_regex_findall(s: String, pattern: String) -> Result<Vec<String>, Error> {
    let re = regex::Regex::new(&pattern)
        .map_err(|e| invalid(format!("regex_findall: bad pattern: {e}")))?;
    Ok(re.find_iter(&s).map(|m| m.as_str().to_string()).collect())
}

fn filter_regex_escape(s: String) -> String {
    regex::escape(&s)
}

fn filter_comment(s: String, style: Option<String>) -> String {
    match style.as_deref() {
        // erlang: `% ` prefix
        Some("erlang") => prefix_lines(&s, "% "),
        // C-style block comment
        Some("c") => format!("/*\n{}\n*/", indent_lines(&s, " * ")),
        // plain (default): `# ` prefix per line
        _ => prefix_lines(&s, "# "),
    }
}

fn prefix_lines(s: &str, prefix: &str) -> String {
    s.lines()
        .map(|l| format!("{prefix}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn indent_lines(s: &str, indent: &str) -> String {
    s.lines()
        .map(|l| format!("{indent}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Mimics crypt(5) `$6$salt$hash` for sha512 — the salt+hash construction
/// here is a stand-in for actual SHA-512-CRYPT (which is iterative and
/// considerably more complex). For test-reproducibility we emit a stable
/// digest of "salt$input" so callers get a deterministic value.
fn filter_password_hash(v: JValue, args: Rest<JValue>) -> Result<String, Error> {
    let s = to_string_loose(&v);
    let algo = args
        .get(0)
        .and_then(|x| x.as_str())
        .unwrap_or("sha512")
        .to_string();
    let salt = args
        .get(1)
        .and_then(|x| x.as_str())
        .unwrap_or("rsl")
        .to_string();
    let combined = format!("{salt}${s}");
    let digest = digest_hex(&algo, &combined)?;
    let prefix = match algo.as_str() {
        "sha256" | "sha224" => "$5$",
        "sha512" | "sha384" => "$6$",
        "md5" => "$1$",
        _ => "$6$",
    };
    Ok(format!("{prefix}{salt}${digest}"))
}

fn filter_hash(v: JValue, algo: Option<String>) -> Result<String, Error> {
    let s = to_string_loose(&v);
    digest_hex(algo.as_deref().unwrap_or("sha256"), &s)
}

fn filter_b64encode(v: JValue) -> String {
    let s = to_string_loose(&v);
    base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
}

fn filter_b64decode(s: String) -> Result<String, Error> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(s.as_bytes())
        .map_err(|e| invalid(format!("b64decode: {e}")))?;
    String::from_utf8(bytes).map_err(|e| invalid(format!("b64decode: invalid utf8: {e}")))
}

fn filter_from_json(s: String) -> Result<JValue, Error> {
    let parsed: serde_json::Value = serde_json::from_str(&s)
        .map_err(|e| invalid(format!("from_json: {e}")))?;
    Ok(json_to_jvalue(&parsed))
}

fn filter_to_json(v: JValue) -> Result<String, Error> {
    let json = jvalue_to_json(&v);
    serde_json::to_string(&json).map_err(|e| invalid(format!("to_json: {e}")))
}

fn filter_to_nice_json(v: JValue) -> Result<String, Error> {
    let json = jvalue_to_json(&v);
    serde_json::to_string_pretty(&json).map_err(|e| invalid(format!("to_nice_json: {e}")))
}

fn filter_from_yaml(s: String) -> Result<JValue, Error> {
    let parsed: serde_yaml::Value = serde_yaml::from_str(&s)
        .map_err(|e| invalid(format!("from_yaml: {e}")))?;
    let json: serde_json::Value = serde_json::to_value(parsed)
        .map_err(|e| invalid(format!("from_yaml: {e}")))?;
    Ok(json_to_jvalue(&json))
}

fn filter_to_yaml(v: JValue) -> Result<String, Error> {
    let json = jvalue_to_json(&v);
    serde_yaml::to_string(&json).map_err(|e| invalid(format!("to_yaml: {e}")))
}

fn filter_to_nice_yaml(v: JValue) -> Result<String, Error> {
    // serde_yaml's serializer uses 2-space indent + line breaks already;
    // we just emit the standard form — keeps the round-trip stable.
    filter_to_yaml(v)
}

/// Hand-rolled URL parser. Output keys match Ansible's `urlsplit`:
/// scheme, netloc, hostname, port, path, query, fragment, username, password.
fn filter_urlsplit(url: String) -> JValue {
    let mut scheme = String::new();
    let mut rest = url.as_str();

    if let Some(idx) = rest.find("://") {
        scheme = rest[..idx].to_string();
        rest = &rest[idx + 3..];
    }

    // fragment
    let (no_frag, fragment) = match rest.find('#') {
        Some(i) => (&rest[..i], rest[i + 1..].to_string()),
        None => (rest, String::new()),
    };
    // query
    let (no_query, query) = match no_frag.find('?') {
        Some(i) => (&no_frag[..i], no_frag[i + 1..].to_string()),
        None => (no_frag, String::new()),
    };
    // path
    let (authority, path) = match no_query.find('/') {
        Some(i) => (&no_query[..i], no_query[i..].to_string()),
        None => (no_query, String::new()),
    };
    // userinfo @ authority
    let (userinfo, hostport) = match authority.rfind('@') {
        Some(i) => (Some(&authority[..i]), &authority[i + 1..]),
        None => (None, authority),
    };
    let (username, password) = match userinfo {
        Some(u) => match u.find(':') {
            Some(i) => (u[..i].to_string(), u[i + 1..].to_string()),
            None => (u.to_string(), String::new()),
        },
        None => (String::new(), String::new()),
    };
    let (hostname, port_s) = if hostport.starts_with('[') {
        // IPv6 literal: [::1]:8080
        if let Some(end) = hostport.find(']') {
            let host = &hostport[1..end];
            let after = &hostport[end + 1..];
            let port = after.strip_prefix(':').unwrap_or("").to_string();
            (host.to_string(), port)
        } else {
            (hostport.to_string(), String::new())
        }
    } else {
        match hostport.rfind(':') {
            Some(i) => (hostport[..i].to_string(), hostport[i + 1..].to_string()),
            None => (hostport.to_string(), String::new()),
        }
    };
    let netloc = if let Some(u) = userinfo {
        format!("{u}@{hostport}")
    } else {
        hostport.to_string()
    };

    let mut map: BTreeMap<String, JValue> = BTreeMap::new();
    map.insert("scheme".into(), JValue::from(scheme));
    map.insert("netloc".into(), JValue::from(netloc));
    map.insert("hostname".into(), JValue::from(hostname));
    map.insert("port".into(), JValue::from(port_s));
    map.insert("path".into(), JValue::from(path));
    map.insert("query".into(), JValue::from(query));
    map.insert("fragment".into(), JValue::from(fragment));
    map.insert("username".into(), JValue::from(username));
    map.insert("password".into(), JValue::from(password));
    JValue::from_serialize(&map)
}

fn filter_expanduser(s: String) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    if s == "~" {
        return home;
    }
    if let Some(rest) = s.strip_prefix("~/") {
        return format!("{home}/{rest}");
    }
    s
}

fn filter_expandvars(s: String) -> String {
    // Replace $VAR (alphanumeric+_) and ${VAR} sequences with env values.
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'{' {
                if let Some(end) = s[i + 2..].find('}') {
                    let name = &s[i + 2..i + 2 + end];
                    out.push_str(&std::env::var(name).unwrap_or_default());
                    i += 2 + end + 1;
                    continue;
                }
            } else if bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == b'_' {
                let mut end = i + 1;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_')
                {
                    end += 1;
                }
                let name = &s[i + 1..end];
                out.push_str(&std::env::var(name).unwrap_or_default());
                i = end;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn filter_basename(s: String) -> String {
    std::path::Path::new(&s)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn filter_dirname(s: String) -> String {
    std::path::Path::new(&s)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn filter_realpath(s: String) -> String {
    std::fs::canonicalize(&s)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or(s)
}

fn filter_splitext(s: String) -> Vec<String> {
    let p = std::path::Path::new(&s);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = p
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let parent = p
        .parent()
        .map(|d| d.to_string_lossy().to_string())
        .unwrap_or_default();
    let head = if parent.is_empty() {
        stem
    } else {
        format!("{parent}/{stem}")
    };
    vec![head, ext]
}

// ---------- list/dict filters ----------

fn filter_dict2items(v: JValue) -> Result<Vec<JValue>, Error> {
    let mut out = Vec::new();
    let iter = v
        .try_iter()
        .map_err(|e| invalid(format!("dict2items: {e}")))?;
    for key in iter {
        let val = v.get_item(&key).unwrap_or(JValue::UNDEFINED);
        let mut entry: BTreeMap<String, JValue> = BTreeMap::new();
        entry.insert("key".into(), key);
        entry.insert("value".into(), val);
        out.push(JValue::from_serialize(&entry));
    }
    Ok(out)
}

fn filter_items2dict(v: JValue, kwargs: Kwargs) -> Result<JValue, Error> {
    let key_name: String = kwargs.get("key_name").unwrap_or_else(|_| "key".to_string());
    let value_name: String = kwargs
        .get("value_name")
        .unwrap_or_else(|_| "value".to_string());
    let _ = kwargs.assert_all_used();
    let iter = v
        .try_iter()
        .map_err(|e| invalid(format!("items2dict: {e}")))?;
    let mut map: BTreeMap<String, JValue> = BTreeMap::new();
    for item in iter {
        let k = item
            .get_attr(&key_name)
            .or_else(|_| item.get_item(&JValue::from(key_name.as_str())))
            .map_err(|e| invalid(format!("items2dict: {e}")))?;
        let val = item
            .get_attr(&value_name)
            .or_else(|_| item.get_item(&JValue::from(value_name.as_str())))
            .unwrap_or(JValue::UNDEFINED);
        map.insert(to_string_loose(&k), val);
    }
    Ok(JValue::from_serialize(&map))
}

fn filter_combine(base: JValue, others: Rest<JValue>) -> Result<JValue, Error> {
    let mut json = jvalue_to_json(&base);
    for other in others.iter() {
        let other_json = jvalue_to_json(other);
        merge_json(&mut json, other_json, true);
    }
    Ok(json_to_jvalue(&json))
}

/// Recursively merge `b` into `a`. Arrays replace; objects merge.
fn merge_json(a: &mut serde_json::Value, b: serde_json::Value, recursive: bool) {
    match (a, b) {
        (serde_json::Value::Object(ax), serde_json::Value::Object(bx)) => {
            for (k, v) in bx {
                if recursive {
                    if let Some(existing) = ax.get_mut(&k) {
                        merge_json(existing, v, true);
                        continue;
                    }
                }
                ax.insert(k, v);
            }
        }
        (slot, b) => {
            *slot = b;
        }
    }
}

fn filter_flatten(v: JValue, levels: Option<i64>) -> Result<Vec<JValue>, Error> {
    let max = levels.unwrap_or(-1);
    let mut out = Vec::new();
    flatten_into(&v, max, &mut out)?;
    Ok(out)
}

fn flatten_into(v: &JValue, levels: i64, out: &mut Vec<JValue>) -> Result<(), Error> {
    if matches!(v.kind(), ValueKind::Seq | ValueKind::Iterable) && levels != 0 {
        let iter = v
            .try_iter()
            .map_err(|e| invalid(format!("flatten: {e}")))?;
        let next_levels = if levels < 0 { -1 } else { levels - 1 };
        for item in iter {
            flatten_into(&item, next_levels, out)?;
        }
    } else {
        out.push(v.clone());
    }
    Ok(())
}

fn filter_unique(v: JValue) -> Result<Vec<JValue>, Error> {
    let iter = v
        .try_iter()
        .map_err(|e| invalid(format!("unique: {e}")))?;
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for item in iter {
        let k = format!("{item:?}");
        if !seen.contains(&k) {
            seen.push(k);
            out.push(item);
        }
    }
    Ok(out)
}

fn collect_strings(v: &JValue) -> Result<Vec<String>, Error> {
    let iter = v
        .try_iter()
        .map_err(|e| invalid(format!("set-op: {e}")))?;
    let mut out = Vec::new();
    for item in iter {
        out.push(format!("{item:?}"));
    }
    Ok(out)
}

fn filter_intersect(a: JValue, b: JValue) -> Result<Vec<JValue>, Error> {
    let bs: BTreeSet<String> = collect_strings(&b)?.into_iter().collect();
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for item in a
        .try_iter()
        .map_err(|e| invalid(format!("intersect: {e}")))?
    {
        let k = format!("{item:?}");
        if bs.contains(&k) && seen.insert(k) {
            out.push(item);
        }
    }
    Ok(out)
}

fn filter_union(a: JValue, b: JValue) -> Result<Vec<JValue>, Error> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for item in a.try_iter().map_err(|e| invalid(format!("union: {e}")))? {
        let k = format!("{item:?}");
        if seen.insert(k) {
            out.push(item);
        }
    }
    for item in b.try_iter().map_err(|e| invalid(format!("union: {e}")))? {
        let k = format!("{item:?}");
        if seen.insert(k) {
            out.push(item);
        }
    }
    Ok(out)
}

fn filter_difference(a: JValue, b: JValue) -> Result<Vec<JValue>, Error> {
    let bs: BTreeSet<String> = collect_strings(&b)?.into_iter().collect();
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for item in a
        .try_iter()
        .map_err(|e| invalid(format!("difference: {e}")))?
    {
        let k = format!("{item:?}");
        if !bs.contains(&k) && seen.insert(k) {
            out.push(item);
        }
    }
    Ok(out)
}

fn filter_symmetric_difference(a: JValue, b: JValue) -> Result<Vec<JValue>, Error> {
    let av = collect_strings(&a)?;
    let bv = collect_strings(&b)?;
    let aset: BTreeSet<&String> = av.iter().collect();
    let bset: BTreeSet<&String> = bv.iter().collect();
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for item in a
        .try_iter()
        .map_err(|e| invalid(format!("sym_diff: {e}")))?
    {
        let k = format!("{item:?}");
        if !bset.contains(&k) && seen.insert(k) {
            out.push(item);
        }
    }
    for item in b
        .try_iter()
        .map_err(|e| invalid(format!("sym_diff: {e}")))?
    {
        let k = format!("{item:?}");
        if !aset.contains(&k) && seen.insert(k) {
            out.push(item);
        }
    }
    Ok(out)
}

fn filter_random(v: JValue) -> Result<JValue, Error> {
    let items: Vec<JValue> = v
        .try_iter()
        .map_err(|e| invalid(format!("random: {e}")))?
        .collect();
    if items.is_empty() {
        return Ok(JValue::UNDEFINED);
    }
    let seed = hash_seed(&format!("{v:?}"));
    let mut rng = DeterministicRng::new(seed);
    let idx = rng.pick(items.len());
    Ok(items[idx].clone())
}

fn filter_shuffle(v: JValue) -> Result<Vec<JValue>, Error> {
    let mut items: Vec<JValue> = v
        .try_iter()
        .map_err(|e| invalid(format!("shuffle: {e}")))?
        .collect();
    let seed = hash_seed(&format!("{v:?}"));
    let mut rng = DeterministicRng::new(seed);
    // Fisher-Yates with deterministic RNG.
    for i in (1..items.len()).rev() {
        let j = rng.pick(i + 1);
        items.swap(i, j);
    }
    Ok(items)
}

fn filter_zip(a: JValue, others: Rest<JValue>) -> Result<Vec<Vec<JValue>>, Error> {
    let mut iters: Vec<Vec<JValue>> = Vec::new();
    iters.push(
        a.try_iter()
            .map_err(|e| invalid(format!("zip: {e}")))?
            .collect(),
    );
    for o in others.iter() {
        iters.push(
            o.try_iter()
                .map_err(|e| invalid(format!("zip: {e}")))?
                .collect(),
        );
    }
    let min_len = iters.iter().map(|v| v.len()).min().unwrap_or(0);
    let mut out = Vec::with_capacity(min_len);
    for i in 0..min_len {
        let row: Vec<JValue> = iters.iter().map(|col| col[i].clone()).collect();
        out.push(row);
    }
    Ok(out)
}

fn filter_subelements(v: JValue, key: String) -> Result<Vec<Vec<JValue>>, Error> {
    let mut out = Vec::new();
    let iter = v
        .try_iter()
        .map_err(|e| invalid(format!("subelements: {e}")))?;
    for item in iter {
        let sub = item
            .get_attr(&key)
            .or_else(|_| item.get_item(&JValue::from(key.as_str())))
            .unwrap_or(JValue::UNDEFINED);
        let elements: Vec<JValue> = sub.try_iter().map(|it| it.collect()).unwrap_or_default();
        for el in elements {
            out.push(vec![item.clone(), el]);
        }
    }
    Ok(out)
}

// ---------- type filters ----------

fn filter_string(v: JValue) -> String {
    to_string_loose(&v)
}

fn filter_mandatory(v: JValue) -> Result<JValue, Error> {
    if v.is_undefined() {
        Err(Error::new(
            ErrorKind::UndefinedError,
            "mandatory variable is not defined",
        ))
    } else {
        Ok(v)
    }
}

fn filter_ternary(v: JValue, true_val: JValue, false_val: Option<JValue>) -> JValue {
    if v.is_true() {
        true_val
    } else {
        false_val.unwrap_or(JValue::from(""))
    }
}

// ---------- tests ----------

fn test_defined(v: JValue) -> bool {
    !v.is_undefined()
}

fn test_undefined(v: JValue) -> bool {
    v.is_undefined()
}

fn test_none(v: JValue) -> bool {
    v.is_none()
}

fn test_string(v: JValue) -> bool {
    matches!(v.kind(), ValueKind::String)
}

fn test_number(v: JValue) -> bool {
    matches!(v.kind(), ValueKind::Number)
}

fn test_sequence(v: JValue) -> bool {
    matches!(v.kind(), ValueKind::Seq | ValueKind::Iterable)
}

fn test_mapping(v: JValue) -> bool {
    matches!(v.kind(), ValueKind::Map | ValueKind::Plain)
}

fn test_match(v: JValue, pattern: String) -> Result<bool, Error> {
    let s = to_string_loose(&v);
    let re = regex::Regex::new(&format!("^(?:{pattern})$"))
        .map_err(|e| invalid(format!("match: {e}")))?;
    Ok(re.is_match(&s))
}

fn test_search(v: JValue, pattern: String) -> Result<bool, Error> {
    let s = to_string_loose(&v);
    let re = regex::Regex::new(&pattern).map_err(|e| invalid(format!("search: {e}")))?;
    Ok(re.is_match(&s))
}

fn test_version(v: JValue, req: String, op: String) -> Result<bool, Error> {
    let actual = to_string_loose(&v);
    let actual_v = parse_lenient_version(&actual)
        .map_err(|e| invalid(format!("version: parse '{actual}': {e}")))?;
    let req_v = parse_lenient_version(&req)
        .map_err(|e| invalid(format!("version: parse '{req}': {e}")))?;
    Ok(match op.as_str() {
        ">" | "gt" => actual_v > req_v,
        ">=" | "ge" => actual_v >= req_v,
        "<" | "lt" => actual_v < req_v,
        "<=" | "le" => actual_v <= req_v,
        "==" | "eq" => actual_v == req_v,
        "!=" | "ne" => actual_v != req_v,
        other => return Err(invalid(format!("version: unknown operator: {other}"))),
    })
}

fn parse_lenient_version(s: &str) -> Result<semver::Version, semver::Error> {
    // Pad partial versions like "1.0" -> "1.0.0".
    let parts: Vec<&str> = s.split('.').collect();
    let padded = match parts.len() {
        1 => format!("{}.0.0", parts[0]),
        2 => format!("{}.{}.0", parts[0], parts[1]),
        _ => s.to_string(),
    };
    semver::Version::parse(&padded)
}

fn outcome_status_is(v: &JValue, wanted: &[&str]) -> bool {
    let status = v
        .get_attr("status")
        .or_else(|_| v.get_item(&JValue::from("status")))
        .ok();
    let s = status
        .as_ref()
        .and_then(|s| s.as_str().map(|x| x.to_string()))
        .unwrap_or_default();
    wanted.iter().any(|w| *w == s)
}

fn test_succeeded(v: JValue) -> bool {
    outcome_status_is(&v, &["ok", "changed"])
}
fn test_failed(v: JValue) -> bool {
    outcome_status_is(&v, &["failed", "unreachable"])
}
fn test_changed(v: JValue) -> bool {
    outcome_status_is(&v, &["changed"])
}
fn test_skipped(v: JValue) -> bool {
    outcome_status_is(&v, &["skipped"])
}

// ---------- registration ----------

pub fn register_filters_and_tests(env: &mut Environment<'static>) {
    // strings
    env.add_filter("bool", filter_bool);
    env.add_filter("quote", filter_quote);
    env.add_filter("regex_replace", filter_regex_replace);
    env.add_filter("regex_search", filter_regex_search);
    env.add_filter("regex_findall", filter_regex_findall);
    env.add_filter("regex_escape", filter_regex_escape);
    env.add_filter("comment", filter_comment);
    env.add_filter("password_hash", filter_password_hash);
    env.add_filter("hash", filter_hash);
    env.add_filter("checksum", filter_hash);
    env.add_filter("b64encode", filter_b64encode);
    env.add_filter("b64decode", filter_b64decode);
    env.add_filter("from_json", filter_from_json);
    env.add_filter("to_json", filter_to_json);
    env.add_filter("to_nice_json", filter_to_nice_json);
    env.add_filter("from_yaml", filter_from_yaml);
    env.add_filter("to_yaml", filter_to_yaml);
    env.add_filter("to_nice_yaml", filter_to_nice_yaml);
    env.add_filter("urlsplit", filter_urlsplit);
    env.add_filter("expanduser", filter_expanduser);
    env.add_filter("expandvars", filter_expandvars);
    env.add_filter("basename", filter_basename);
    env.add_filter("dirname", filter_dirname);
    env.add_filter("realpath", filter_realpath);
    env.add_filter("splitext", filter_splitext);

    // list/dict
    env.add_filter("dict2items", filter_dict2items);
    env.add_filter("items2dict", filter_items2dict);
    env.add_filter("combine", filter_combine);
    env.add_filter("flatten", filter_flatten);
    env.add_filter("unique", filter_unique);
    env.add_filter("intersect", filter_intersect);
    env.add_filter("union", filter_union);
    env.add_filter("difference", filter_difference);
    env.add_filter("symmetric_difference", filter_symmetric_difference);
    env.add_filter("random", filter_random);
    env.add_filter("shuffle", filter_shuffle);
    env.add_filter("zip", filter_zip);
    env.add_filter("subelements", filter_subelements);

    // type
    env.add_filter("string", filter_string);
    env.add_filter("str", filter_string);
    env.add_filter("mandatory", filter_mandatory);
    env.add_filter("ternary", filter_ternary);

    // TODO_M2: ipaddr / ipv4 / ipv6 — full implementation requires the IPv6
    // address algebra (subnet expansion, prefix arithmetic). Stub passes the
    // input through unchanged so templates referencing it don't crash.
    env.add_filter("ipaddr", |v: JValue| v);
    env.add_filter("ipv4", |v: JValue| v);
    env.add_filter("ipv6", |v: JValue| v);

    // tests
    env.add_test("defined", test_defined);
    env.add_test("undefined", test_undefined);
    env.add_test("none", test_none);
    env.add_test("string", test_string);
    env.add_test("number", test_number);
    env.add_test("sequence", test_sequence);
    env.add_test("mapping", test_mapping);
    env.add_test("match", test_match);
    env.add_test("search", test_search);
    env.add_test("version", test_version);
    env.add_test("succeeded", test_succeeded);
    env.add_test("success", test_succeeded);
    env.add_test("failed", test_failed);
    env.add_test("failure", test_failed);
    env.add_test("changed", test_changed);
    env.add_test("skipped", test_skipped);
    env.add_test("skip", test_skipped);
}
