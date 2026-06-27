//! Target-conditional time shim (mirrors `nmp-core::time`).
//!
//! On `wasm32-unknown-unknown`, `std::time::SystemTime::now()` panics at
//! runtime.  `web-time 1.1` provides a drop-in replacement backed by
//! `Date.now()` — available in every JS Worker context.
//!
//! On native targets `web-time` re-exports `std::time` verbatim: the types are
//! **identical**.  Native behaviour is byte-for-byte unchanged.
//!
//! ## Usage rule
//!
//! All wasm-reachable code in this crate that needs `SystemTime` or
//! `UNIX_EPOCH` MUST import from this module rather than directly from
//! `std::time`.  D20 (doctrine-lint) enforces this automatically.
//!
//! `Duration` is the same type in both namespaces (`std::time::Duration` ==
//! `web_time::Duration`) so it is not re-exported here; import it directly
//! from `std::time`.
#[cfg(not(target_arch = "wasm32"))]
pub use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(target_arch = "wasm32")]
pub use web_time::{SystemTime, UNIX_EPOCH};
