//! runsible-core
//!
//! Shared types, errors, and traits for every binary in the runsible workspace.
//! No type defined here may be redefined downstream — see docs/plans/MASTER.md §7.

pub mod errors;
pub mod event;
pub mod traits;
pub mod types;

pub use errors::{Error, Result};

/// Smoke gate: serialize a real `RunStart` event to NDJSON, parse it back,
/// and check the wire format the engine actually produces. Returns 0 on
/// success.  Used by the runsible-core-test binary's TRIPLE SIMS.
pub fn f30() -> i32 {
    let ev = event::Event::RunStart {
        playbook: "f30.toml".into(),
        inventory: Some("localhost,".into()),
        host_count: 1,
        runsible_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let line = match ev.to_ndjson_line() {
        Ok(l) => l,
        Err(_) => return 1,
    };
    if !line.ends_with('\n') {
        return 2;
    }
    let v: serde_json::Value = match serde_json::from_str(line.trim_end()) {
        Ok(v) => v,
        Err(_) => return 3,
    };
    if v.get("kind").and_then(|k| k.as_str()) != Some("run_start") {
        return 4;
    }
    if v.get("host_count").and_then(|k| k.as_u64()) != Some(1) {
        return 5;
    }
    // Round-trip: deserialize back into the typed enum, re-serialize, must match.
    let back: event::Event = match serde_json::from_str(line.trim_end()) {
        Ok(e) => e,
        Err(_) => return 6,
    };
    let s2 = match serde_json::to_string(&back) {
        Ok(s) => s,
        Err(_) => return 7,
    };
    if s2.as_str() != line.trim_end() {
        return 8;
    }
    0
}
