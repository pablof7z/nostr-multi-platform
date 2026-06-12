//! Injectable wall-clock for the kernel ingest path.
//!
//! `SystemTime::now()` was called directly inside the kernel reducer
//! (`kernel/ingest/`), which makes the reducer non-deterministic and blocks
//! the deterministic replay path (`kernel/replay.rs`). This module extracts
//! the wall-clock read behind a `Clock` trait so tests and replay can
//! substitute a fixed time.
//!
//! Scope note: only `SystemTime::now()` reads that feed business logic
//! (event `created_at` stamps, `received_at_ms` passed to `EventStore`)
//! route through `Clock`. `Instant::now()` reads used purely for
//! performance timing (emit latency, EOSE timing) stay as direct calls —
//! they never affect replay output.

use crate::time::SystemTime;

/// Wall-clock used by the kernel ingest path.
///
/// Injected so tests and deterministic replay can substitute a fixed clock.
/// `Send + Sync + 'static`: the kernel reads the clock single-threaded inside
/// the reducer, but the test-support injection seam
/// (`NmpApp::set_kernel_clock_for_test`) shares the SAME clock `Arc` between
/// the host thread (which may advance an `AtomicU64`-backed test clock) and the
/// actor thread (which reads it), so `Sync` is required for that handoff.
/// Production's `SystemClock` is trivially `Sync`.
pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> SystemTime;
}

/// Production clock — delegates to `SystemTime::now()`.
pub(crate) struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// Fixed-time clock for tests and deterministic replay. Returns the same
/// `SystemTime` on every call so the reducer's timestamp output is
/// reproducible. Exercised by `kernel/clock_injection_tests.rs`, which
/// proves `received_at_ms` routes through the injected `Clock`.
// `allow(dead_code)`: the struct is consumed only in `#[cfg(test)]` code
// (`clock_injection_tests.rs`); the `test-support` exposure is for external
// crate integration tests that inject a fixed clock via `kernel.set_clock`.
#[cfg_attr(not(test), allow(dead_code))]
#[cfg(any(test, feature = "test-support"))]
pub(crate) struct FixedClock(pub SystemTime);

#[cfg(any(test, feature = "test-support"))]
impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        self.0
    }
}

/// Test-support advanceable clock: returns `base + advance` seconds, where
/// `advance` starts at zero and the test bumps it via [`Self::advance_secs`].
///
/// This exists so end-to-end FFI tests that publish two replaceable events
/// (e.g. a kind:3 follow then unfollow) can give the second event a strictly
/// greater `created_at` WITHOUT a wall-clock sleep (D8) — the kernel stamps
/// `created_at` from its injected [`Clock`], so advancing this clock between
/// the two dispatches makes the second event win the NIP-01 replaceable
/// supersession deterministically. The host thread (test) and the actor thread
/// (kernel reader) share one `Arc<MonotonicSecondClock>`; the `AtomicU64`
/// offset makes the cross-thread advance race-free (`Sync`).
#[cfg(any(test, feature = "test-support"))]
pub struct MonotonicSecondClock {
    base: SystemTime,
    advance_secs: std::sync::atomic::AtomicU64,
}

#[cfg(any(test, feature = "test-support"))]
impl MonotonicSecondClock {
    /// Build a clock anchored at `base` with a zero advance.
    #[must_use]
    pub fn new(base: SystemTime) -> Self {
        Self {
            base,
            advance_secs: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Bump the returned time forward by `secs` seconds. Subsequent
    /// [`Clock::now`] reads observe the new offset (`Release`/`Acquire`).
    pub fn advance_secs(&self, secs: u64) {
        self.advance_secs
            .fetch_add(secs, std::sync::atomic::Ordering::Release);
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Clock for MonotonicSecondClock {
    fn now(&self) -> SystemTime {
        let secs = self
            .advance_secs
            .load(std::sync::atomic::Ordering::Acquire);
        self.base + std::time::Duration::from_secs(secs)
    }
}
