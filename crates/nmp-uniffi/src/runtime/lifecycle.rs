//! App-lifecycle UniFFI methods — M14-C6.
//!
//! Migrates three of the four C-ABI symbols from `nmp-ffi/src/lifecycle.rs`
//! to typed `#[uniffi::export] impl NmpApp` methods:
//!
//! | UniFFI method          | C-ABI counterpart                  |
//! |------------------------|------------------------------------|
//! | `lifecycle_foreground` | `nmp_app_lifecycle_foreground`     |
//! | `lifecycle_background` | `nmp_app_lifecycle_background`     |
//! | `is_alive`             | `nmp_app_is_alive`                 |
//!
//! `nmp_app_set_lifecycle_callback` is NOT migrated — see the M14-D blocker
//! note in `crates/nmp-uniffi/src/runtime/mod.rs` for the reason.
//!
//! ## Doctrine
//!
//! * All three methods are fire-and-forget or pull-only; they carry no
//!   callback interface and require no quiescence contract.
//! * D6: `lifecycle_foreground` / `lifecycle_background` dispatch commands
//!   best-effort on the actor channel (a closed channel is a silent no-op).
//!   `is_alive` is a lock-based probe that never panics.

use crate::NmpApp;

#[uniffi::export]
impl NmpApp {
    /// Report the platform entering the foreground (`scenePhase == .active` on
    /// iOS, or equivalent). Fire-and-forget.
    ///
    /// The actor folds the phase into the kernel and fires the registered
    /// lifecycle observer on a `Background → Foreground` (or first-after-boot)
    /// transition. Repeated `Foreground` calls debounce to a no-op.
    ///
    /// D6: a dead actor (channel closed) silently drops the command.
    pub fn lifecycle_foreground(&self) {
        self.inner.lifecycle_foreground();
    }

    /// Report the platform entering the background (`scenePhase == .background`
    /// on iOS, or equivalent). Fire-and-forget. Symmetric to
    /// [`lifecycle_foreground`].
    ///
    /// D6: a dead actor silently drops the command.
    pub fn lifecycle_background(&self) {
        self.inner.lifecycle_background();
    }

    /// Actor-liveness probe: returns `true` when the actor `JoinHandle` is
    /// still running, `false` otherwise.
    ///
    /// This is the pull-side companion to the `UpdateEnvelope::Panic` push
    /// frame (D7): a host that missed the panic frame while backgrounded can
    /// call this on resume to learn the same fact.
    ///
    /// Returns `false` before `start()` or after the actor has exited (clean
    /// shutdown or panic).
    pub fn is_alive(&self) -> bool {
        self.inner.is_alive()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::NmpApp;

    /// Parity with `nmp_app_is_alive` C-ABI test
    /// `is_alive_after_new_returns_zero_before_start`: `is_alive()` must
    /// return `false` before `start()`.
    #[test]
    fn parity_is_alive_false_before_start() {
        let app = NmpApp::new();
        assert!(
            !app.is_alive(),
            "actor must not be alive before start()"
        );
    }

    /// Parity with `nmp_app_is_alive` C-ABI test
    /// `is_alive_after_new_returns_zero_before_start` (post-start part):
    /// `is_alive()` returns `true` after `start()`.
    #[test]
    fn parity_is_alive_true_after_start() {
        let app = NmpApp::new();
        app.start(256, 4);
        assert!(app.is_alive(), "actor must be alive after start()");
        app.shutdown();
    }

    /// Parity with the C-ABI foreground/background tests:
    /// `lifecycle_foreground` and `lifecycle_background` must not panic and
    /// must be callable before and after `start()`.
    #[test]
    fn parity_lifecycle_signals_no_panic() {
        let app = NmpApp::new();
        // Before start: commands queue (passive handle).
        app.lifecycle_foreground();
        app.lifecycle_background();
        app.start(256, 4);
        // After start: commands reach the actor.
        app.lifecycle_foreground();
        app.lifecycle_background();
        app.shutdown();
    }

    /// `lifecycle_foreground` after shutdown is a silent no-op (D6:
    /// closed channel drops the send).
    #[test]
    fn parity_lifecycle_after_shutdown_no_panic() {
        let app = NmpApp::new();
        app.start(256, 4);
        app.shutdown();
        // Must not panic; the actor channel is closed.
        app.lifecycle_foreground();
        app.lifecycle_background();
    }
}
