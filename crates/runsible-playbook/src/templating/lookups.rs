//! Ansible-compatible lookup catalog.
//!
//! Each lookup is registered both:
//!   * as a direct callable: `{{ env('PATH') }}`
//!   * via the dispatcher: `{{ lookup('env', 'PATH') }}`
//!
//! Where Ansible's lookups are non-deterministic (`password`), we substitute
//! a deterministic seed derived from the path so tests are reproducible —
//! see `lookup_password` for the explicit deviation note.

use std::collections::BTreeMap;

use minijinja::value::{Rest, ValueKind};
use minijinja::{Environment, Error, ErrorKind, Value as JValue};

fn invalid<S: Into<String>>(msg: S) -> Error {
    Error::new(ErrorKind::InvalidOperation, msg.into())
}

fn val_to_string(v: &JValue) -> String {
    if let Some(s) = v.as_str() {
        s.to_string()
    } else if v.is_undefined() || v.is_none() {
        String::new()
    } else {
        format!("{v}")
    }
}

// ---------- env ----------

fn lookup_env(args: Rest<JValue>) -> Result<String, Error> {
    let name = args.get(0).ok_or_else(|| invalid("env: missing name"))?;
    let default = args.get(1).map(val_to_string).unwrap_or_default();
    let var = val_to_string(name);
    Ok(std::env::var(&var).unwrap_or(default))
}

// ---------- file ----------

fn lookup_file(args: Rest<JValue>) -> Result<String, Error> {
    let path = args
        .get(0)
        .map(val_to_string)
        .ok_or_else(|| invalid("file: missing path"))?;
    std::fs::read_to_string(&path).map_err(|e| invalid(format!("file: {path}: {e}")))
}

// ---------- pipe ----------

fn lookup_pipe(args: Rest<JValue>) -> Result<String, Error> {
    let cmd = args
        .get(0)
        .map(val_to_string)
        .ok_or_else(|| invalid("pipe: missing command"))?;
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .output()
        .map_err(|e| invalid(format!("pipe: spawn: {e}")))?;
    let mut s = String::from_utf8_lossy(&out.stdout).to_string();
    if s.ends_with('\n') {
        s.pop();
        if s.ends_with('\r') {
            s.pop();
        }
    }
    Ok(s)
}

// ---------- password ----------

/// DEVIATION FROM ANSIBLE: the upstream `password` lookup writes a random
/// secret to the path argument and reuses it on subsequent calls. We instead
/// derive a *deterministic* secret from `hash(path)` so tests are
/// reproducible without filesystem side effects. Document this behavior in
/// user-facing docs.
fn lookup_password(args: Rest<JValue>) -> Result<String, Error> {
    let spec = args
        .get(0)
        .map(val_to_string)
        .ok_or_else(|| invalid("password: missing spec"))?;
    // Ansible spec: "path/to/file length=16 chars=ascii_letters"
    let mut pieces = spec.split_whitespace();
    let path = pieces.next().unwrap_or("default").to_string();
    let mut length: usize = 20;
    let mut charset = "ascii_letters,digits".to_string();
    for p in pieces {
        if let Some(rest) = p.strip_prefix("length=") {
            if let Ok(n) = rest.parse() {
                length = n;
            }
        } else if let Some(rest) = p.strip_prefix("chars=") {
            charset = rest.to_string();
        }
    }
    let alphabet = charset_alphabet(&charset);
    if alphabet.is_empty() {
        return Err(invalid("password: empty alphabet"));
    }
    use sha2::{Digest, Sha256};
    let mut state: Vec<u8> = Sha256::digest(path.as_bytes()).to_vec();
    let mut out = String::with_capacity(length);
    let mut idx: usize = 0;
    while out.len() < length {
        if idx >= state.len() {
            // Re-hash to extend the deterministic stream.
            let mut h = Sha256::new();
            h.update(&state);
            h.update(b"|extend");
            state = h.finalize().to_vec();
            idx = 0;
        }
        let pick = state[idx] as usize % alphabet.len();
        out.push(alphabet[pick]);
        idx += 1;
    }
    Ok(out)
}

fn charset_alphabet(spec: &str) -> Vec<char> {
    let mut out: Vec<char> = Vec::new();
    for token in spec.split(|c: char| c == ',' || c == ' ') {
        match token {
            "ascii_letters" => out.extend(('a'..='z').chain('A'..='Z')),
            "ascii_lowercase" => out.extend('a'..='z'),
            "ascii_uppercase" => out.extend('A'..='Z'),
            "digits" => out.extend('0'..='9'),
            "punctuation" => out.extend(['!', '@', '#', '$', '%', '^', '&', '*']),
            "" => {}
            other => out.extend(other.chars()),
        }
    }
    out
}

// ---------- vars ----------

/// Note: this is a runtime-context lookup. We can't access the rendering
/// state's vars from a global `add_function`, so this lookup form returns
/// the key string itself wrapped — useful for explicit
/// `{{ vars(some_var_name) }}` patterns where templates already chase the
/// var via `{{ context[some_var_name] }}` or similar.
fn lookup_vars(args: Rest<JValue>) -> Result<JValue, Error> {
    // TODO_M2: requires access to render-state vars; for now we echo the
    // requested name back so templates that wrap with `default` keep
    // working without crashing.
    let name = args.get(0).map(val_to_string).unwrap_or_default();
    Ok(JValue::from(name))
}

// ---------- items ----------

fn lookup_items(args: Rest<JValue>) -> Result<Vec<JValue>, Error> {
    let mut out = Vec::new();
    for v in args.iter() {
        if matches!(v.kind(), ValueKind::Seq | ValueKind::Iterable) {
            let iter = v.try_iter().map_err(|e| invalid(format!("items: {e}")))?;
            for it in iter {
                out.push(it);
            }
        } else {
            out.push(v.clone());
        }
    }
    Ok(out)
}

// ---------- indexed_items ----------

fn lookup_indexed_items(args: Rest<JValue>) -> Result<Vec<Vec<JValue>>, Error> {
    let flat = lookup_items(args)?;
    Ok(flat
        .into_iter()
        .enumerate()
        .map(|(i, v)| vec![JValue::from(i as i64), v])
        .collect())
}

// ---------- lines ----------

fn lookup_lines(args: Rest<JValue>) -> Result<Vec<String>, Error> {
    let cmd = args
        .get(0)
        .map(val_to_string)
        .ok_or_else(|| invalid("lines: missing command"))?;
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .output()
        .map_err(|e| invalid(format!("lines: spawn: {e}")))?;
    let s = String::from_utf8_lossy(&out.stdout);
    Ok(s.lines().map(|l| l.to_string()).collect())
}

// ---------- fileglob ----------

fn lookup_fileglob(args: Rest<JValue>) -> Result<Vec<String>, Error> {
    let pattern = args
        .get(0)
        .map(val_to_string)
        .ok_or_else(|| invalid("fileglob: missing pattern"))?;
    let path = std::path::Path::new(&pattern);
    let (root, glob_str) = if pattern.contains('/') {
        let parent = path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("/"));
        let file = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "*".to_string());
        (parent, file)
    } else {
        (std::path::PathBuf::from("."), pattern.clone())
    };
    let matcher = globset::Glob::new(&glob_str)
        .map_err(|e| invalid(format!("fileglob: {e}")))?
        .compile_matcher();
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&root) {
        for entry in rd.flatten() {
            let name = entry.file_name();
            let name_s = name.to_string_lossy();
            if matcher.is_match(name_s.as_ref()) {
                let p = entry.path();
                out.push(p.to_string_lossy().to_string());
            }
        }
    }
    out.sort();
    Ok(out)
}

// ---------- first_found ----------

fn lookup_first_found(args: Rest<JValue>) -> Result<String, Error> {
    let candidates: Vec<String> = if args.len() == 1 && matches!(args[0].kind(), ValueKind::Seq) {
        args[0]
            .try_iter()
            .map_err(|e| invalid(format!("first_found: {e}")))?
            .map(|v| val_to_string(&v))
            .collect()
    } else {
        args.iter().map(val_to_string).collect()
    };
    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return Ok(path.clone());
        }
    }
    Err(invalid(format!(
        "first_found: no candidate found in {candidates:?}"
    )))
}

// ---------- dispatcher ----------

fn lookup_dispatch(args: Rest<JValue>) -> Result<JValue, Error> {
    let name = args
        .get(0)
        .map(val_to_string)
        .ok_or_else(|| invalid("lookup: missing name"))?;
    let rest_vec: Vec<JValue> = args.iter().skip(1).cloned().collect();
    let rest = Rest(rest_vec);
    match name.as_str() {
        "env" => lookup_env(rest).map(JValue::from),
        "file" => lookup_file(rest).map(JValue::from),
        "pipe" => lookup_pipe(rest).map(JValue::from),
        "password" => lookup_password(rest).map(JValue::from),
        "vars" => lookup_vars(rest),
        "items" => lookup_items(rest).map(JValue::from),
        "indexed_items" => {
            let xs = lookup_indexed_items(rest)?;
            let outer: Vec<JValue> = xs
                .into_iter()
                .map(|row| JValue::from(row))
                .collect();
            Ok(JValue::from(outer))
        }
        "lines" => lookup_lines(rest).map(JValue::from),
        "fileglob" => lookup_fileglob(rest).map(JValue::from),
        "first_found" => lookup_first_found(rest).map(JValue::from),
        other => Err(invalid(format!("unknown lookup: {other}"))),
    }
}

pub fn register_lookups(env: &mut Environment<'static>) {
    env.add_function("lookup", lookup_dispatch);
    env.add_function("query", lookup_dispatch);

    // Direct callable forms.
    env.add_function("env", |args: Rest<JValue>| lookup_env(args));
    env.add_function("file", |args: Rest<JValue>| lookup_file(args));
    env.add_function("pipe", |args: Rest<JValue>| lookup_pipe(args));
    env.add_function("password", |args: Rest<JValue>| lookup_password(args));
    env.add_function("vars", |args: Rest<JValue>| lookup_vars(args));
    env.add_function("items", |args: Rest<JValue>| lookup_items(args));
    env.add_function("indexed_items", |args: Rest<JValue>| {
        lookup_indexed_items(args)
    });
    env.add_function("lines", |args: Rest<JValue>| lookup_lines(args));
    env.add_function("fileglob", |args: Rest<JValue>| lookup_fileglob(args));
    env.add_function("first_found", |args: Rest<JValue>| lookup_first_found(args));

    // Stash the dispatcher table as a global var for introspection / docs.
    let mut available: BTreeMap<String, JValue> = BTreeMap::new();
    for name in [
        "env",
        "file",
        "pipe",
        "password",
        "vars",
        "items",
        "indexed_items",
        "lines",
        "fileglob",
        "first_found",
    ] {
        available.insert(name.to_string(), JValue::from(true));
    }
    env.add_global("_runsible_lookups", JValue::from_serialize(&available));
}
