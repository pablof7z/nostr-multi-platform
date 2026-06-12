//! Target-conditional time shim for `nmp-store`.
//!
//! On `wasm32-unknown-unknown`, `std::time::Instant::now()` panics at runtime
//! (the platform has no OS clock). `web-time 1.1` provides a drop-in
//! replacement backed by `performance.now()`, which is always available in a
//! JS Worker context (including the Chirp wasm worker that drives the kernel).
//!
//! On native targets `web-time` re-exports `std::time` verbatim — behaviour
//! is byte-for-byte identical with a direct `std::time` import.
//!
//! ## Usage rule (D20)
//!
//! All wasm-reachable `nmp-store` code that needs `Instant` MUST import from
//! this module rather than directly from `std::time`. `Duration` is the same
//! type in both namespaces and may be imported from `std::time` directly.
//!
//! ## Scope
//!
//! This shim is intentionally narrow: only `Instant` is re-exported because
//! that is the only time type `nmp-store` uses on wasm-reachable paths
//! (`MemEventStore::gc_step` — wall-clock budget timer). `SystemTime` /
//! `UNIX_EPOCH` are not needed here (the store never reads absolute time; the
//! kernel passes `now_secs` as a caller-supplied `u64` per D7).
#[cfg(not(target_arch = "wasm32"))]
pub use std::time::Instant;
#[cfg(target_arch = "wasm32")]
pub use web_time::Instant;
