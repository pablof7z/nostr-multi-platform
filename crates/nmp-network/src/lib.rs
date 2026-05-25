//! `nmp-network` — Layer-1 native WebSocket transport
//! (`docs/architecture/crate-boundaries.md` §3.8 / §5 step 8).
//!
//! ## Step 8 phase A — extraction (shipped)
//!
//! Four modules, moved verbatim from `nmp-core` so the kernel crate no
//! longer owns the `tungstenite`/`mio`/`rustls` graph:
//!
//! 1. [`relay_protocol`] — wire-transport-agnostic constants and helpers
//!    (backoff bounds, keepalive thresholds, per-URL deterministic jitter,
//!    HTTP-denial classifier). Compiles unconditionally so the wasm32
//!    browser driver (phase C, this crate) can reuse the exact same values
//!    without depending on the native I/O stack.
//! 2. [`relay_worker`] — the native WebSocket worker thread (one socket per
//!    resolved relay URL, mid-session reconnect with jittered exponential
//!    backoff, T120b keepalive FSM). Gated behind the `native` Cargo
//!    feature so wasm32 builds compile without the
//!    `tungstenite`/`mio`/`rustls` graph.
//! 3. [`keepalive`] — the pure FSM the worker drives. Internal to the
//!    transport layer; `nmp-core` no longer re-exports it.
//! 4. [`role::RelayRole`] — the transport-lane discriminator the worker
//!    tags every `RelayEvent` with. Moved from `nmp_core::relay::RelayRole`
//!    and re-exported by `nmp-core` under the prior path
//!    (`nmp_core::RelayRole`) so downstream callers keep compiling
//!    unchanged.
//!
//! ## Dependency direction
//!
//! `nmp-network` does **not** depend on `nmp-core` — that direction would
//! re-introduce the cycle the step-8 extraction is meant to break. The
//! kernel-facing `RelayFrame` enum stays in `nmp-core`; the
//! `tungstenite::Message → RelayFrame` adapter (which bridges this
//! crate's wire type to the kernel's frame enum) lives in
//! `nmp_core::actor::dispatch` at the actor seam. The phase-C browser
//! driver preserves the same direction by taking its kernel touchpoints
//! through a `Rc<dyn Fn>` callback bag (`BrowserKernelHandlers`)
//! constructed in `nmp-wasm`.
//!
//! ## Step 8 phase B — push-model [`pool::Pool`] API (shipped)
//!
//! Adds the [`pool`] module: `Pool` / `RelayHandle` / `PoolEvent` /
//! `PoolConfig` / `PoolSnapshot` per spec §3.8. Implemented as a thin
//! wrapper around the existing `relay_worker::spawn_relay_worker`
//! lifecycle (preserves the per-URL state machine, jittered
//! exponential backoff, T120b keepalive FSM bit-for-bit). The
//! generational `RelayHandle` makes stale handles structurally
//! invalid: a handle from before a reconnect cannot silently target
//! the wrong generation of the same URL, and there is no
//! "send to all" method on `Pool` (the structural answer to NDK
//! issue #175).
//!
//! ## Step 8 phase F — actor cut-over + legacy surface withdrawn (this PR)
//!
//! The kernel actor in `crates/nmp-core/src/actor/` no longer drives
//! `spawn_relay_worker` directly — every per-URL socket is owned by a
//! process-wide [`pool::Pool`] and the actor consumes
//! [`pool::PoolEvent`]s on its dedicated relay-event channel. With no
//! external consumer left, the legacy `relay_worker` module is now
//! crate-private (`pub(crate)`); `RelayEvent` / `RelayCommand` /
//! `spawn_relay_worker` / `spawn_relay_worker_with_keepalive` survive
//! only as the implementation detail [`pool::Pool`] wraps internally.
//! Out-of-crate callers MUST use the `pool` module.
//!
//! ## Step 8 phase C — [`browser_driver`] move (this PR)
//!
//! Adds the [`browser_driver`] module — the wasm32 equivalent of
//! [`relay_worker`], moved verbatim from `nmp-wasm/src/relay_driver.rs`.
//! Both transports now live in this crate, behind their respective target
//! gates: `relay_worker` under `#[cfg(feature = "native")]`,
//! `browser_driver` under `#[cfg(target_arch = "wasm32")]`. The driver's
//! kernel touchpoints were converted from a `Rc<RefCell<KernelReducer>>`
//! reference to a small [`browser_driver::BrowserKernelHandlers`] struct of
//! `Rc<dyn Fn>` callbacks; `nmp-wasm::relay_pool` constructs the callbacks
//! from its own `KernelReducer` handle. This preserves the layering
//! invariant (`nmp-network` MUST NOT depend on `nmp-core`) while keeping
//! the driver's behavior, event ordering, and borrow semantics identical
//! to the pre-move version.
//!
//! ## Step 8 phase D — `nmp-signer-broker` rides `Pool` (shipped)
//!
//! `nmp-signer-broker::relay_client` is now a thin wrapper over
//! [`pool::Pool`] (`PoolRelayClient`). The duplicate mio/tungstenite
//! readiness loop in the broker is gone — V-13 Stage 2 dedupe. The
//! broker owns ONE `Pool` per active bunker session; the dispatcher
//! thread replays installed subscriptions on each fresh
//! [`pool::PoolEvent::Opened`] so the inbound REQ survives a relay flap
//! (V-14). The broker's Cargo.toml no longer names `tungstenite` /
//! `mio` / `rustls` directly — only this crate.
//!
//! ## Step 8 phase E — NIP-42 AUTH wire/FSM split (shipped)
//!
//! Pre-classifies the inbound `["AUTH", <challenge>]` NIP-42 frame at
//! the wire layer into a typed [`pool::RelayFrame::Auth`] variant so the
//! kernel sees a structured signal instead of re-discovering AUTH from
//! raw text on the ingest fast-path. Implemented in
//! [`pool::inner::classify_text_frame`] via the dependency-free
//! `nmp-nip42-types::parse_auth_frame` parser.
//!
//! Out-of-scope for this crate (deliberate layering, per
//! `docs/architecture/crate-boundaries.md` §3.8):
//!
//! - **Build the kind:22242 reply event** — lives in
//!   [`nmp_nip42::build_auth_event`]. `nmp-network` MUST NOT name
//!   kind 22242 anywhere.
//! - **Pause / replay subscriptions during a challenge** — lives in
//!   `nmp_core::subs::AuthGate`. `nmp-network` MUST NOT name the gate
//!   nor the per-relay `RelayAuthState` enum.
//! - **Per-relay handshake driver state** — lives in
//!   `nmp_nip42::flow::Nip42Driver` and the kernel's `kernel/auth.rs`
//!   mirror.
//!
//! Each phase is a separate PR with its own acceptance criteria.

pub mod keepalive;
pub mod relay_protocol;
mod role;

pub use role::RelayRole;

// Phase F: `relay_worker` is the legacy per-URL worker primitive `pool::Pool`
// wraps internally. With the kernel actor cut over to the `pool` surface
// there are no remaining out-of-crate callers, so the module is crate-private
// — out-of-crate consumers must reach the transport through `nmp_network::pool`.
#[cfg(feature = "native")]
pub(crate) mod relay_worker;

#[cfg(feature = "native")]
pub mod pool;

// Step 8 phase C — wasm32 browser driver. Gated to `wasm32` because it
// depends on `web_sys`/`js-sys`/`wasm-bindgen`; the native build of
// `nmp-network` does not see this module. `nmp-wasm` is the sole caller
// today (it constructs `BrowserKernelHandlers` from its own kernel handle
// and feeds them into `BrowserRelayDriver::new`).
#[cfg(target_arch = "wasm32")]
pub mod browser_driver;
