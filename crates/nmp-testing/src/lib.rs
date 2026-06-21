//! Test and benchmark helpers for the NMP workspace.
//!
//! Modules:
//! - [`harness_probe`] — event-driven frame/action waits (FrameProbe, ProbeSignal).
//! - [`perf_gate`]    — shared gate-assertion type for ffi-stress and firehose-bench.
//! - [`store_harness`] — store-layer test fixtures.

pub mod harness_probe;
pub mod perf_gate;
pub mod store_harness;

pub fn crate_ready() -> bool {
    true
}
