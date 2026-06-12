//! Target-conditional time shim.
//!
//! On `wasm32-unknown-unknown`, `std::time::Instant::now()` and
//! `std::time::SystemTime::now()` panic at runtime (the platform has no OS
//! clock). `web-time 1.1` provides drop-in replacements backed by
//! `performance.now()` (for `Instant`) and `Date.now()` (for `SystemTime`) —
//! both are available in every JS Worker context, including the Chirp wasm
//! worker that drives the kernel.
//!
//! On native targets `web-time` re-exports `std::time` verbatim: the types
//! are **identical**. Native behaviour is byte-for-byte unchanged.
//!
//! ## Usage rule
//!
//! All wasm-reachable kernel code that needs `Instant`, `SystemTime`, or
//! `UNIX_EPOCH` MUST import from this module rather than directly from
//! `std::time`. D20 (doctrine-lint) enforces this automatically.
//!
//! `Duration` is the same type in both namespaces (`std::time::Duration` ==
//! `web_time::Duration`) and may be imported directly from `std::time`.
//!
//! `UNIX_EPOCH` must be imported from this module whenever used with a
//! `SystemTime` value obtained from `Clock::now()` or `SystemTime::now()`,
//! because on wasm32 the types differ and `duration_since` would fail to
//! compile with a mismatched `UNIX_EPOCH`.
//!
//! ## Version note
//!
//! `web-time 1.1.0` was already in `Cargo.lock` (pulled transitively via
//! egui/winit) before this shim was introduced, so no new crate version
//! enters the dependency tree.
#[cfg(not(target_arch = "wasm32"))]
pub use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[cfg(target_arch = "wasm32")]
pub use web_time::{Duration, Instant, SystemTime, UNIX_EPOCH};
