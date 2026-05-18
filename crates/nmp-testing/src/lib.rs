//! Test and benchmark helpers for the NMP workspace.
//!
//! The first concrete artifact is the `reactivity-bench` binary. The library
//! module stays intentionally small until shared fixtures are needed by tests.

pub mod store_harness;

pub fn crate_ready() -> bool {
    true
}
