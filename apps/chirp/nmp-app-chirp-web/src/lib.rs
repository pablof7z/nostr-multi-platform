//! `nmp-app-chirp-web` — wasm32 composition root for the Chirp web client.
//!
//! This crate wires the OP-centric home feed (`nmp.feed.home`) into
//! [`nmp_wasm::WasmRuntime`]. It is the web twin of
//! `apps/chirp/nmp-app-chirp/src/ffi/interest_feed.rs`: the native crate uses
//! `NmpApp::register_event_observer` + `build_actor_claim_sink`; this crate
//! uses `KernelReducer::register_event_observer` + a queuing claim sink whose
//! drain runs in the post-tick hook.
//!
//! # Crate layout
//!
//! * [`claim_queue`] — `PendingClaimQueue`, `build_queuing_claim_sink`, and
//!   `drain_pending_claims`. Solves the RefCell re-entrancy hazard: the engine
//!   fires its claim sink during the kernel's observer fan-out, which runs
//!   while `KernelReducer` is mutably borrowed inside
//!   `handle_relay_frame`. Instead of calling `claim_event` immediately
//!   (which would attempt a second `borrow_mut` → panic), the sink queues
//!   the `ClaimRequest` in a `VecDeque`. After `handle_relay_frame` returns
//!   (borrow released), the post-tick drain processes the queue.
//!
//! * [`composition`] — `setup_chirp_web_feeds` wires everything together:
//!   creates the `ActiveFollowSet`, builds the engine via `register_op_feed`,
//!   registers the engine as a `KernelEventObserver`, registers the typed
//!   `nmp.feed.home` snapshot projection, and installs the drain hook into the
//!   runtime's tick cadence.
//!
//! # RefCell safety invariant
//!
//! `tick_once` in `nmp-wasm/src/tick.rs` holds a scoped `borrow_mut` that is
//! released before the function returns. The post-tick drain fires after
//! `tick_once` returns, so `drain_pending_claims` can safely call
//! `reducer.borrow_mut().claim_event(…)` without triggering a panic.
//!
//! The same invariant holds for the observer fan-out path: the claim sink
//! (closure captured inside the engine) runs synchronously on the actor/wasm
//! thread during `handle_relay_frame`. Because the sink QUEUES rather than
//! drains, it never touches the `KernelReducer` borrow directly.

pub mod claim_queue;
pub mod composition;
// wasm32 composition-root entry point. Compiled only for the wasm32 target so
// `wasm-bindgen` glue is never emitted for native builds or test binaries.
#[cfg(target_arch = "wasm32")]
pub mod wasm_binding;

pub use composition::{setup_chirp_web_feeds, ChirpWebFeedSetup};
