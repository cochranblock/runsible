//! `runsible-test` — developer-facing test runner for runsible packages.
//!
//! M0 scope: single-package sanity + units + env discovery.

pub mod config;
pub mod env;
pub mod errors;
pub mod sanity;
pub mod units;

pub use env::{discover_env, EnvReport};
pub use errors::{Result, TestError};
pub use sanity::{run_sanity, SanityFinding, SanityReport, Severity};
pub use units::{run_units, UnitsReport};
