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

use std::time::SystemTime;

/// Wall-clock used by the kernel ingest path.
///
/// Injected so tests and deterministic replay can substitute a fixed clock.
/// `Send + 'static` is enough for `Arc<dyn Clock>` storage on the `!Send`
/// kernel — the handle never crosses a thread boundary, so `Sync` is not
/// required.
pub(crate) trait Clock: Send + 'static {
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
/// reproducible.
// `dead_code` allow: the seam is intentionally dormant — no replay test
// consumes `FixedClock` yet. The trait + setter + ingest call-site swap are
// enough to make the reducer's clock injectable; the first deterministic
// replay test will exercise this.
#[cfg(any(test, feature = "test-support"))]
#[allow(dead_code)]
pub(crate) struct FixedClock(pub SystemTime);

#[cfg(any(test, feature = "test-support"))]
impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        self.0
    }
}
