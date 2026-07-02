#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unused_imports
)]
#[path = "generated/nmp_update_generated.rs"]
pub mod nmp_update_generated;

pub use nmp_update_generated::nmp::transport as wire;

// ADR-0071 / S2 (#1750) — the open write envelope (`DispatchEnvelope`). Separate
// flatc-generated unit from the read-direction `UpdateFrame`; carried as raw
// bytes across the one byte doorway on each boundary. The decode + fail-closed
// gates + namespace routing live in `super::dispatch_envelope`.
#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unused_imports
)]
#[path = "generated/dispatch_envelope_generated.rs"]
pub mod dispatch_envelope_generated;

pub use dispatch_envelope_generated::nmp::transport as write_wire;

// The decode + fail-closed gates + opaque-payload carry. Public so the wasm
// runtime and the native FFI byte doorway both reach the one decode path.
pub mod dispatch_envelope;
