//! Small, reusable substrate utilities that are not Nostr-specific.
//!
//! Currently this module contains [`TimeCached`], a generic TTL value wrapper,
//! and [`sort_dedup`], a convenience helper for sorted + deduped `Vec`s. Per
//! the nostrdb/notedeck distillation (`docs/design/nostrdb-notedeck-lessons.md`
//! §3.12) several runtime facts — per-relay NIP-11 info, mailbox lists,
//! capability probes, NIP-05 verification results — are expensive to recompute
//! but tolerate staleness. They all want the same shape: hold a value, hand it
//! back until a TTL elapses, then recompute on the next read.

use crate::time::Instant;
use std::time::Duration;

/// Sort `v` in place and remove consecutive duplicates.
///
/// Equivalent to the two-line idiom `v.sort(); v.dedup();` that appears at
/// every relay-URL deduplication site in the kernel and actor layers.
/// Centralising it here eliminates the repeated pattern and makes the intent
/// explicit at call sites.
pub fn sort_dedup<T: Ord>(v: &mut Vec<T>) {
    v.sort();
    v.dedup();
}

/// A value of type `T` cached behind a time-to-live.
///
/// `TimeCached` holds an optional `T` and the monotonic [`Instant`] at which
/// that value was last refreshed. Reads go through [`get_or_refresh`], which
/// returns the cached value while it is fresh and otherwise invokes a
/// caller-supplied closure to recompute it.
///
/// # Injected clock (determinism)
///
/// This type never calls [`Instant::now`] internally. The current time is
/// always supplied by the caller as a `now: Instant` parameter, consistent
/// with the actor's monotonic-time discipline (see `kernel::status::elapsed_ms`
/// and `actor::tick::flush_due`, which take `Instant` the same way). Because
/// `Instant` is monotonic by construction and the value is injected, every
/// state transition is fully deterministic and unit-testable without sleeping.
///
/// `new` is created with no anchor instant, so the **first** call to
/// [`get_or_refresh`] always invokes the refresh closure (any `initial` value
/// is treated as already stale until it is anchored to a real clock reading).
///
/// [`get_or_refresh`]: TimeCached::get_or_refresh
///
/// # Examples
///
/// ```
/// use std::cell::Cell;
/// use std::time::{Duration, Instant};
/// use nmp_core::util::TimeCached;
///
/// // Expensive-to-fetch NIP-11 relay info, refreshed at most every 5 minutes.
/// let ttl = Duration::from_secs(300);
/// let mut nip11: TimeCached<String> = TimeCached::new(ttl, None);
///
/// let fetches = Cell::new(0);
/// let mut fetch = || {
///     fetches.set(fetches.get() + 1);
///     format!("relay-info-v{}", fetches.get())
/// };
///
/// let t0 = Instant::now();
///
/// // First read: nothing anchored yet, so the closure runs.
/// assert_eq!(nip11.get_or_refresh(t0, &mut fetch), "relay-info-v1");
///
/// // Within the TTL the cached value is returned; the closure does not run.
/// let t1 = t0 + Duration::from_secs(60);
/// assert_eq!(nip11.get_or_refresh(t1, &mut fetch), "relay-info-v1");
/// assert_eq!(fetches.get(), 1);
///
/// // Past the TTL the value is recomputed.
/// let t2 = t0 + ttl;
/// assert_eq!(nip11.get_or_refresh(t2, &mut fetch), "relay-info-v2");
/// assert_eq!(fetches.get(), 2);
///
/// // An explicit invalidate forces the next read to recompute, even if fresh.
/// nip11.invalidate();
/// assert_eq!(nip11.get_or_refresh(t2, &mut fetch), "relay-info-v3");
/// assert_eq!(fetches.get(), 3);
/// ```
#[derive(Debug, Clone)]
pub struct TimeCached<T> {
    ttl: Duration,
    /// The instant at which `value` was last refreshed. `None` means the
    /// cache has no live anchor: the next read must refresh.
    anchored_at: Option<Instant>,
    value: Option<T>,
}

impl<T> TimeCached<T> {
    /// Create a new cache with the given `ttl`.
    ///
    /// `initial` may seed the slot with a value, but it is treated as already
    /// stale: there is no clock reading to anchor it to, so the first
    /// [`get_or_refresh`](Self::get_or_refresh) always recomputes. Passing
    /// `Some(_)` only affects [`peek`](Self::peek) before the first refresh.
    #[must_use]
    pub fn new(ttl: Duration, initial: Option<T>) -> Self {
        Self {
            ttl,
            anchored_at: None,
            value: initial,
        }
    }

    /// Whether the next [`get_or_refresh`](Self::get_or_refresh) at `now` would
    /// recompute the value.
    ///
    /// True when there is no anchor (fresh cache or post-[`invalidate`]) or
    /// when `now` is at or beyond `anchor + ttl`.
    ///
    /// [`invalidate`]: Self::invalidate
    #[must_use]
    pub fn is_stale(&self, now: Instant) -> bool {
        match self.anchored_at {
            None => true,
            Some(anchor) => now.saturating_duration_since(anchor) >= self.ttl,
        }
    }

    /// Return the cached value if fresh at `now`, otherwise recompute it with
    /// `refresh`, store it (anchored to `now`), and return it.
    ///
    /// The boundary is inclusive: when `now == anchor + ttl` the value is
    /// considered stale and is recomputed.
    pub fn get_or_refresh(&mut self, now: Instant, refresh: impl FnOnce() -> T) -> &T {
        // When stale, clear the slot and re-anchor; `get_or_insert_with` then
        // runs `refresh` exactly once and hands back a borrow of the stored
        // value. When fresh, `value` is `Some` by construction (the only way
        // `is_stale` is false is via a prior call that populated it), so the
        // closure does not run. The `&mut T` returned by `get_or_insert_with`
        // carries the "value is present" proof in the type system, so there
        // is no panic path here at all (no `unwrap`/`expect`/`unreachable!`).
        if self.is_stale(now) {
            self.value = None;
            self.anchored_at = Some(now);
        }
        self.value.get_or_insert_with(refresh)
    }

    /// Borrow the currently cached value without consulting the clock or
    /// refreshing. Returns `None` if nothing has been cached yet.
    #[must_use]
    pub fn peek(&self) -> Option<&T> {
        self.value.as_ref()
    }

    /// Drop the freshness anchor so the next
    /// [`get_or_refresh`](Self::get_or_refresh) recomputes the value
    /// regardless of TTL. The current value (if any) is retained for
    /// [`peek`](Self::peek) until then.
    pub fn invalidate(&mut self) {
        self.anchored_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A refresh closure factory that counts invocations and returns a
    /// monotonically increasing value, so tests assert on *call count*
    /// (deterministic) rather than wall-clock behaviour.
    fn counting_refresh(counter: &Cell<u32>) -> impl FnOnce() -> u32 + '_ {
        move || {
            counter.set(counter.get() + 1);
            counter.get()
        }
    }

    #[test]
    fn first_read_always_refreshes_even_with_initial_value() {
        let calls = Cell::new(0);
        let mut cache = TimeCached::new(Duration::from_secs(10), Some(999u32));
        let t0 = Instant::now();

        assert_eq!(*cache.get_or_refresh(t0, counting_refresh(&calls)), 1);
        assert_eq!(calls.get(), 1, "initial value is treated as stale");
    }

    #[test]
    fn returns_cached_value_within_ttl() {
        let calls = Cell::new(0);
        let ttl = Duration::from_secs(30);
        let mut cache = TimeCached::new(ttl, None);
        let t0 = Instant::now();

        assert_eq!(*cache.get_or_refresh(t0, counting_refresh(&calls)), 1);

        // Several reads strictly inside the TTL must not recompute.
        for secs in [1, 10, 29] {
            let t = t0 + Duration::from_secs(secs);
            assert_eq!(*cache.get_or_refresh(t, counting_refresh(&calls)), 1);
        }
        assert_eq!(calls.get(), 1, "closure invoked exactly once within TTL");
    }

    #[test]
    fn refreshes_after_ttl() {
        let calls = Cell::new(0);
        let ttl = Duration::from_secs(60);
        let mut cache = TimeCached::new(ttl, None);
        let t0 = Instant::now();

        assert_eq!(*cache.get_or_refresh(t0, counting_refresh(&calls)), 1);

        let past = t0 + ttl + Duration::from_secs(1);
        assert_eq!(*cache.get_or_refresh(past, counting_refresh(&calls)), 2);
        assert_eq!(calls.get(), 2);

        // The new value is re-anchored to `past`, so a read just after it is
        // still fresh.
        let just_after = past + Duration::from_secs(1);
        assert_eq!(
            *cache.get_or_refresh(just_after, counting_refresh(&calls)),
            2
        );
        assert_eq!(calls.get(), 2, "value re-anchored to the refresh instant");
    }

    #[test]
    fn ttl_boundary_is_inclusive_and_triggers_refresh() {
        let calls = Cell::new(0);
        let ttl = Duration::from_secs(45);
        let mut cache = TimeCached::new(ttl, None);
        let t0 = Instant::now();

        assert_eq!(*cache.get_or_refresh(t0, counting_refresh(&calls)), 1);

        // Exactly at `anchor + ttl` the value is stale (>= boundary).
        let at_boundary = t0 + ttl;
        assert!(cache.is_stale(at_boundary));
        assert_eq!(
            *cache.get_or_refresh(at_boundary, counting_refresh(&calls)),
            2
        );
        assert_eq!(calls.get(), 2);

        // One nanosecond before the boundary it is still fresh.
        let cache_anchor = at_boundary;
        let just_before = cache_anchor + ttl - Duration::from_nanos(1);
        assert!(!cache.is_stale(just_before));
    }

    #[test]
    fn invalidate_forces_refresh_within_ttl() {
        let calls = Cell::new(0);
        let ttl = Duration::from_secs(300);
        let mut cache = TimeCached::new(ttl, None);
        let t0 = Instant::now();

        assert_eq!(*cache.get_or_refresh(t0, counting_refresh(&calls)), 1);

        let t1 = t0 + Duration::from_secs(10);
        assert!(!cache.is_stale(t1), "still fresh before invalidate");

        cache.invalidate();
        assert!(cache.is_stale(t1), "invalidate marks the cache stale");

        // Same clock reading, but invalidate forced a recompute.
        assert_eq!(*cache.get_or_refresh(t1, counting_refresh(&calls)), 2);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn peek_observes_without_clock_or_refresh() {
        let calls = Cell::new(0);
        let mut cache = TimeCached::new(Duration::from_secs(5), Some(7u32));

        // Seeded value visible via peek before any refresh.
        assert_eq!(cache.peek(), Some(&7));
        assert_eq!(calls.get(), 0, "peek never invokes the closure");

        let t0 = Instant::now();
        assert_eq!(*cache.get_or_refresh(t0, counting_refresh(&calls)), 1);
        assert_eq!(cache.peek(), Some(&1));

        // peek after invalidate still shows the retained value.
        cache.invalidate();
        assert_eq!(cache.peek(), Some(&1));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn deterministic_sequence_under_injected_clock() {
        let calls = Cell::new(0);
        let ttl = Duration::from_secs(100);
        let mut cache = TimeCached::new(ttl, None);
        let base = Instant::now();

        // A fully scripted clock timeline yields a deterministic call count.
        let script = [
            (0u64, 1u32), // first read -> refresh #1
            (50, 1),      // fresh
            (99, 1),      // fresh (just before boundary)
            (100, 2),     // boundary -> refresh #2 (re-anchored at +100)
            (150, 2),     // fresh relative to new anchor
            (200, 3),     // +100 past new anchor -> refresh #3
        ];
        for (offset, expected) in script {
            let now = base + Duration::from_secs(offset);
            assert_eq!(
                *cache.get_or_refresh(now, counting_refresh(&calls)),
                expected,
                "offset {offset}s"
            );
        }
        assert_eq!(
            calls.get(),
            3,
            "exactly three refreshes across the timeline"
        );
    }
}
