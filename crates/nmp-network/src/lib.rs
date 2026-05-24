//! `nmp-network` — Layer-1 native WebSocket transport
//! (`docs/architecture/crate-boundaries.md` §3.8 / §5 step 8).
//!
//! ## This PR (step 8 phase A — extraction only)
//!
//! Four modules, moved verbatim from `nmp-core` so the kernel crate no
//! longer owns the `tungstenite`/`mio`/`rustls` graph:
//!
//! 1. [`relay_protocol`] — wire-transport-agnostic constants and helpers
//!    (backoff bounds, keepalive thresholds, per-URL deterministic jitter,
//!    HTTP-denial classifier). Compiles unconditionally so the wasm32
//!    `BrowserRelayDriver` in `nmp-wasm` can keep reusing the exact same
//!    values without depending on the native I/O stack.
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
//! `nmp_core::actor::dispatch` at the actor seam.
//!
//! ## This PR (step 8 phase B — push-model [`pool::Pool`] API)
//!
//! Adds the [`pool`] module: `Pool` / `RelayHandle` / `PoolEvent` /
//! `PoolConfig` / `PoolSnapshot` per spec §3.8. Implemented as a thin
//! wrapper around the existing [`relay_worker::spawn_relay_worker`]
//! lifecycle (preserves the per-URL state machine, jittered
//! exponential backoff, T120b keepalive FSM bit-for-bit). The
//! generational `RelayHandle` makes stale handles structurally
//! invalid: a handle from before a reconnect cannot silently target
//! the wrong generation of the same URL, and there is no
//! "send to all" method on `Pool` (the structural answer to NDK
//! issue #175).
//!
//! The legacy [`relay_worker::RelayEvent`] / [`relay_worker::RelayCommand`] /
//! [`relay_worker::spawn_relay_worker`] entry points stay
//! re-exported alongside `Pool` so the actor in
//! `crates/nmp-core/src/actor/relay_mgmt.rs` (today's ~38 call sites)
//! compiles unchanged. The actor migration to `Pool` is the next PR
//! in this lane — see `WIP.md`.
//!
//! ## Deferred to follow-up PRs (step 8 phases C/D/E)
//!
//! - **Phase C** — move `nmp-wasm/src/relay_driver.rs` (the
//!   `BrowserRelayDriver`) into `nmp-network` behind a wasm-only feature
//!   gate so the two transports live side-by-side in the same crate.
//! - **Phase D** — migrate `nmp-signer-broker` onto the new `Pool` primitive
//!   (V-13 dedupe: today `relay_client.rs` mirrors `relay_worker`'s mio +
//!   tungstenite + jitter dance line-for-line).
//! - **Phase E** — NIP-42 AUTH wire/FSM split. The pool performs the
//!   wire handshake (surfaces inbound `AUTH` as a `RelayFrame` variant)
//!   but does NOT compute the kind:22242 event (lives in `nmp-nip42`)
//!   nor pause/replay subscriptions (lives in the planner's `AuthGate`).
//!
//! Each phase is a separate PR with its own acceptance criteria.

pub mod keepalive;
pub mod relay_protocol;
mod role;

pub use role::RelayRole;

#[cfg(feature = "native")]
pub mod relay_worker;

#[cfg(feature = "native")]
pub mod pool;
