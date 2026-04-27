//! Templating subsystem — MiniJinja `Environment` plus the Ansible-compatible
//! filter/test/lookup catalogs.
//!
//! Structure:
//!   * [`core`] — [`Templater`] wrapper, value conversion, render helpers.
//!   * [`filters`] — `register_filters_and_tests` populates the env with the
//!     ≥40-filter Ansible-compatible catalog plus the test catalog.
//!   * [`lookups`] — `register_lookups` registers the Ansible-style `lookup()`
//!     dispatcher and direct callable forms.
//!
//! Adding a new filter? Add it inside [`filters::register_filters_and_tests`].
//! Adding a new lookup? Add it inside [`lookups::register_lookups`] (and to the
//! dispatcher map).

pub mod core;
pub mod filters;
pub mod lookups;

pub use self::core::Templater;

#[cfg(test)]
mod tests;
