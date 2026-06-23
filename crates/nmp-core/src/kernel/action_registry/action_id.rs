//! Correlation-id generation for the action registry.
//!
//! [`new_action_id`] is the sole exported symbol. It lives in a sub-module so
//! `action_registry.rs` stays under the 500-line hard cap.

use crate::substrate::ActionId;

/// Generate a unique 32-hex-char action correlation id.
///
/// Combines the caller-supplied wall-clock millisecond stamp (`now_ms`, read
/// at the FFI system boundary by `ffi/action.rs`) with a process-lifetime
/// atomic counter so two ids minted at the same instant still differ. This is
/// a correlation handle, not a security token — no cryptographic randomness
/// is required (the M6 ledger may swap in a UUID later without touching
/// callers). The clock is injected rather than read here so tests can pin the
/// leading hex word for deterministic id assertions.
pub(super) fn new_action_id(now_ms: u64) -> ActionId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    // 64-bit now_ms + 64-bit sequence → 32 hex. The sequence guarantees
    // uniqueness within a single millisecond.
    format!("{now_ms:016x}{seq:016x}")
}
