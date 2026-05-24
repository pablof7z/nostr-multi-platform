//! FFI callback panic guard — UB prevention.
//!
//! Every invocation of a foreign C function pointer (a Swift / Kotlin
//! callback registered through one of the `ffi/*` registration symbols)
//! MUST be wrapped here. Two failure modes both produce undefined
//! behaviour if a Rust stack frame is unwound across the C ABI boundary:
//!
//! * the foreign side raises (an `ObjC` `NSException`, a Swift `fatalError`,
//!   a Kotlin exception) — the unwinder has no Rust landing pads to run;
//! * Rust code reachable from the callback panics (e.g. a `CStr` decode
//!   inside a Rust trait observer that the same fan-out also invokes).
//!
//! `extern "C" fn` is *not* `extern "C-unwind" fn`: an unwind reaching the
//! ABI boundary is UB, not a clean abort. [`std::panic::catch_unwind`]
//! converts that unwind into a recoverable `Result` at the call site so
//! the fan-out can log and degrade gracefully (doctrine **D6**: nothing
//! crosses the FFI as an exception).
//!
//! The actor-thread `catch_unwind` in `ffi/mod.rs` only guards the actor
//! *loop*; the per-callback fan-out sites in `actor/commands/*` and
//! `ffi/*` each need their own guard, which is what [`guard_ffi_callback`]
//! provides.

use std::panic::{catch_unwind, AssertUnwindSafe};

/// Invoke a foreign C callback (or any Rust code reachable from one)
/// inside a [`catch_unwind`] so a panic / foreign exception cannot unwind
/// across the C ABI boundary (undefined behaviour).
///
/// `site` is a short human label for the call site (e.g. `"event
/// observer"`); it keeps the call sites self-describing — the panic arm
/// itself emits no output (see the return-value note below). `body` is the
/// closure that performs the actual `(registration.callback)(...)`
/// invocation and returns its result.
///
/// Returns `Some(value)` when the callback completed normally, or `None`
/// when it panicked / threw — callers that need diagnostics can observe the
/// `None` return and log it (library code never writes to stderr itself —
/// doctrine **D6**: failures are data, not host-visible side effects).
/// Callers that fan out to multiple callbacks should invoke this once per
/// callback so one panicking observer does not abort the others; callers
/// with a return value treat `None` as "callback failed" and fall through
/// to their existing degraded path.
///
/// [`AssertUnwindSafe`] is required because a raw `fn` pointer plus a
/// `*mut c_void` context are `!UnwindSafe` by default. Asserting is sound
/// here: on the panic path the closure's captured values are never
/// observed again — the caller either returns `None` or moves to the next
/// registration with a fresh snapshot.
#[must_use]
pub fn guard_ffi_callback<R>(site: &str, body: impl FnOnce() -> R) -> Option<R> {
    // `site` is retained as a documented call-site label even though the
    // panic arm no longer logs: it keeps the call sites self-describing and
    // leaves a single edit point should a host-observable diagnostic seam
    // (D6-conforming — data, not stderr) ever be wired in.
    let _ = site;
    // D6: the `None` return IS the failure signal. Library code must never
    // write to stderr — that side effect belongs to the host. Callers that
    // need diagnostics observe `None` and log it themselves. `.ok()` maps the
    // `catch_unwind` `Result` to `Option`, discarding the panic payload.
    catch_unwind(AssertUnwindSafe(body)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_return_is_passed_through() {
        let out = guard_ffi_callback("test", || 41 + 1);
        assert_eq!(out, Some(42));
    }

    #[test]
    fn str_panic_is_caught_and_returns_none() {
        let out: Option<()> = guard_ffi_callback("test", || panic!("boom &str"));
        assert!(out.is_none());
    }

    #[test]
    fn string_panic_is_caught_and_returns_none() {
        let out: Option<()> =
            guard_ffi_callback("test", || panic!("{}", String::from("boom String")));
        assert!(out.is_none());
    }

    #[test]
    fn guard_does_not_unwind_past_itself() {
        // The whole point: a panic inside the body must not propagate.
        // If `catch_unwind` failed to contain it this test would abort
        // the process rather than complete.
        for i in 0..3 {
            let out: Option<i32> = guard_ffi_callback("loop", || {
                if i == 1 {
                    panic!("middle iteration panics");
                }
                i
            });
            if i == 1 {
                assert!(out.is_none());
            } else {
                assert_eq!(out, Some(i));
            }
        }
    }
}
