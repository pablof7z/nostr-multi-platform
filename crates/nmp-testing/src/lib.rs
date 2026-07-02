//! Test and benchmark helpers for the NMP workspace.

pub mod harness_probe;
pub mod perf_report;
pub mod store_harness;

pub fn crate_ready() -> bool {
    true
}

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
