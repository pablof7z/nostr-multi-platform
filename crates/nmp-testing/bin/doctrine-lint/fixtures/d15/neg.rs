//! Negative D15 fixture — must produce zero D15 findings.
//!
//! Every host-closure invocation here is wrapped in either `catch_unwind`
//! (Rust observers) or `guard_ffi_callback` (C-ABI fn pointers), matching
//! the canonical pattern in `nmp-core/src/actor/commands/event_observer.rs`
//! and `nmp-core/src/capability_socket.rs`.

use std::panic::{catch_unwind, AssertUnwindSafe};

pub struct DispatchRegistry {
    observer: Option<Box<dyn Fn(String) + Send + Sync>>,
}

impl DispatchRegistry {
    /// Same-line guard: `catch_unwind(AssertUnwindSafe(|| observer(...)))`
    /// is the canonical Rust-observer panic-isolation shape.
    pub fn deliver_guarded_same_line(&self, payload: String) {
        if let Some(observer) = self.observer.as_ref() {
            let _ = catch_unwind(AssertUnwindSafe(|| observer(payload)));
        }
    }
}

pub struct FfiSlot {
    callback: Box<dyn Fn(*const u8)>,
}

impl FfiSlot {
    /// Multi-line guard: a `catch_unwind(` that opens on one line and the
    /// invocation lives inside the block. The brace-depth tracker keeps
    /// the guard scope live across line boundaries.
    pub fn invoke_guarded_multi_line(&self, payload: *const u8) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            (self.callback)(payload);
        }));
    }
}

/// `guard_ffi_callback` is recognised as a guard alongside `catch_unwind`
/// — it's the same machinery scoped to the C-ABI direction. An unguarded
/// `extern "C" fn` call wrapped in it must NOT fire D15.
pub fn cabi_guarded(callback: extern "C" fn(*const u8), payload: *const u8) {
    let _: Option<()> = guard_ffi_callback("cabi shim", || callback(payload));
}

// Stub so the fixture's `guard_ffi_callback(...)` call type-checks at
// review time. Never compiled — the lint scans text only.
fn guard_ffi_callback<R>(_site: &str, _body: impl FnOnce() -> R) -> Option<R> {
    None
}

/// Per-line `// doctrine-allow: D15 — reason` opt-out: the rule has an
/// approved escape hatch for the cases where a panic is the intended
/// signal (e.g. the actor command drain).
pub fn legitimately_loud_callsite(observer: &dyn Fn()) {
    observer(); // doctrine-allow: D15 — internal closure, no FFI surface
}

/// `fn observer(` is a function DEFINITION, not an invocation. The rule
/// must not false-positive on the signature line.
pub fn observer(_payload: &str) {
    // body intentionally empty
}
