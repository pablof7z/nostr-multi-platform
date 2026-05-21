//! Positive D15 fixture — must trigger at least one D15 finding.
//!
//! This file is NEVER compiled (Cargo only picks up files referenced from
//! a Cargo.toml `path = ...` entry). It exists solely as text for the
//! doctrine-lint smoke test to scan.

use std::sync::Mutex;

pub struct DispatchRegistry {
    observer: Option<Box<dyn Fn(String) + Send + Sync>>,
}

impl DispatchRegistry {
    /// Unguarded bare `observer(` invocation — D15 must fire here. The host
    /// observer is a stored `Box<dyn Fn>`; calling it without a
    /// `catch_unwind` lets a panic in foreign code abort the kernel.
    pub fn deliver_unguarded(&self, payload: String) {
        if let Some(observer) = self.observer.as_ref() {
            observer(payload);
        }
    }
}

pub struct FfiSlot {
    callback: Box<dyn Fn(*const u8)>,
}

impl FfiSlot {
    /// Unguarded `(self.callback)(...)` parens-wrapped invocation — D15
    /// must fire here too. The leading parens are the unambiguous shape
    /// the rule recognises for a stored `Box<dyn Fn>` call site.
    pub fn invoke_unguarded(&self, payload: *const u8) {
        (self.callback)(payload);
    }
}

/// A second pattern: an `extern "C"`-fn-pointer call. D15 expects this to
/// be wrapped in `guard_ffi_callback`; the unguarded shape must fire.
pub fn cabi_unguarded(callback: extern "C" fn(*const u8), payload: *const u8) {
    callback(payload);
}
