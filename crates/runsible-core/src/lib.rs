//! runsible-core
//!
//! Shared types, errors, and traits for every binary in the runsible workspace.
//! No type defined here may be redefined downstream — see docs/plans/MASTER.md §7.

pub mod errors;
pub mod event;
pub mod traits;
pub mod types;

pub use errors::{Error, Result};
