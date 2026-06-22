//! Browser-facing surface for NMP.
//!
//! This crate keeps the wire contract host-testable while the browser actor
//! driver grows behind it. Web hosts render snapshots and execute capabilities;
//! Rust remains responsible for policy and state transitions.

pub mod protocol;
// V-01 Stage 3 — `BrowserRelayDriver`, the wasm32 transport that closes the
// gap the Stage 2 `KernelReducer` left open. Step 8 phase C moved the
// driver itself into `nmp_network::browser_driver` so both transports live
// in `nmp-network`; this crate now consumes it (constructing the
// `BrowserKernelHandlers` callback bag from its `KernelReducer` handle in
// `relay_pool::build_handlers`).
// V-01 Stage 3 — runtime-side pool helpers: spawn one driver per bootstrap
// entry, build the kernel-handler callback bag + outbound sink, tear them
// all down on Stop. `pub(crate)` because nothing outside the crate
// constructs a relay pool directly — the runtime owns the lifecycle.
#[cfg(target_arch = "wasm32")]
mod relay_pool;
// Pure relay-driver planning (one socket per URL). Always-compiled so the
// dedup/role-collapse logic is unit-tested on native CI, even though the
// `relay_pool` consumer that turns plans into live `BrowserRelayDriver`s is
// wasm32-only.
mod relay_plan;
mod runtime;
// V-01 Stage 3b — signer install path + snapshot push helpers. Both modules
// are always-compiled (no `cfg(wasm32)`): the signer slot is a `Signer`
// trait object usable on any target (Nip07Signer.sign() returns Unsupported
// off-wasm, which is the same honest answer the runtime would give anyway).
// snapshot.rs builds the binary update frame on both targets; the JS-callback push
// inside it is `cfg(target_arch = "wasm32")`-gated, with a native no-op
// shim so call sites stay shim-free.
mod dispatch_routing;
// V-01 Stage 3c — async publish path for app-level write actions on wasm32.
// Always-compiled (the pure reason-string helpers are needed on the native
// `runtime.rs` write-path failure arms too); the `publish_app_action` async
// function and `fan_out_outbound` helper are `cfg(target_arch = "wasm32")`-
// gated because they own `BrowserRelayDriver` and `js_sys::Function`
// references — neither exists off-wasm.
mod publish_path;
mod signer_slot;
mod snapshot;
// PR-2 — 1 Hz periodic tick driver. `tick_once` is always-compiled so the
// native `tick_for_test` helper exercises the same coalescing path as the
// wasm32 timer closure. `start_tick_interval` is wasm32-gated.
mod tick;

pub use protocol::{
    ActionDispatch, AppAction, AppActionDispatch, BeginSign, CapabilityFailure, CapabilityResult,
    ClientHello, DegradedMode, DeliverSignerResponse, RelayBootstrapEntry, RuntimeStatus,
    SetSigner, StartConfig, WorkerEvent, WorkerRequest,
};
pub use runtime::{WasmRuntime, WasmRuntimeError};
