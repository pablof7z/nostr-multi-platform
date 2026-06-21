//! Gate-assertion re-exports from the shared `nmp_testing::perf_gate` module.
//!
//! `ffi-stress` and `firehose-bench` both use the same [`Gate`] type so their
//! `gates` arrays are schema-compatible (`SCHEMA_VERSION 1`).

pub(crate) use nmp_testing::perf_gate::{Gate, SCHEMA_VERSION};
