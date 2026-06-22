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
// Active-identity validation (`signer_slot`) + snapshot push helpers. Both
// modules are always-compiled (no `cfg(wasm32)`): `signer_slot` only
// validates/canonicalizes the `SetIdentity` pubkey for the kernel active account
// (ADR-0064 §5 removed the persistent `Arc<dyn Signer>` slot). snapshot.rs
// builds the binary update frame on both targets; the JS-callback push inside
// it is `cfg(target_arch = "wasm32")`-gated, with a native no-op shim so call
// sites stay shim-free.
mod dispatch_routing;
// Honest write-path disable token (`publish_not_supported_in_web_preview`).
// ADR-0064 §5 removed the wasm `Arc<dyn Signer>.await`-inside-publish path; a
// signed wasm write is the ADR-0050 capability round-trip (`BeginSign` →
// `SignRequest` → `DeliverSignerResponse`). Always-compiled — the reason string
// is needed on the native `runtime.rs`/`dispatch.rs` write-path failure arms.
mod publish_path;
mod signer_slot;
mod snapshot;
// PR-2 — 1 Hz periodic tick driver. `tick_once` is always-compiled so the
// native `tick_for_test` helper exercises the same coalescing path as the
// wasm32 timer closure. `start_tick_interval` is wasm32-gated.
mod tick;

pub use protocol::{
    ActionDispatch, BeginSign, CapabilityFailure, CapabilityResult, ClientHello, DegradedMode,
    DeliverSignerResponse, DispatchBytes, RelayBootstrapEntry, RuntimeStatus, SetIdentity,
    StartConfig, WorkerEvent, WorkerRequest,
};
pub use runtime::{WasmRuntime, WasmRuntimeError};
