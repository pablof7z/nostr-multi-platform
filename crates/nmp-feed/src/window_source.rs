//! [`FeedWindowSource`] — the per-tick materialized window a feed's typed
//! producer AND its author-resolve provider BOTH read (ADR-0063 D7, #1671 Lane H).
//!
//! ## Why this exists — the structural-pairing + no-gap fix
//!
//! Before this, two independent closures each called `snapshot_current_window()`
//! on the same feed within one snapshot tick:
//!
//! 1. the **feed-author provider** (`reconcile_feed_author_refs`, run at the TOP
//!    of `make_update`), which extracts the visible author set and auto-resolves
//!    it; and
//! 2. the **typed projection producer** (`run_typed_projections`, run LATER in
//!    the same `make_update`), which encodes the visible window onto the wire.
//!
//! Two problems:
//!
//! - **The `load_older` 1-frame gap (HIGH).** `load_older_feed` mutates the
//!   feed's render viewport SYNCHRONOUSLY on the FFI thread, OUTSIDE the actor.
//!   It can therefore widen the window in the interval BETWEEN those two
//!   `snapshot_current_window()` reads. The provider then resolves the narrower
//!   (pre-widen) author set while the typed producer emits the wider (post-widen)
//!   rows — so the newly-revealed rows' authors were never resolved and render a
//!   blank avatar for one frame.
//! - **No structural pairing (BLOCKING).** Nothing tied the two closures
//!   together, so a feed could register a typed sidecar and simply forget the
//!   author provider (exactly what the dynamic author/thread feeds did).
//!
//! `FeedWindowSource` fixes both: it materializes the window EXACTLY ONCE per
//! tick (keyed by a kernel-published monotone tick rev) and hands the SAME
//! `Arc<S>` snapshot to both the provider and the typed producer. Because both
//! read one materialization, they cannot disagree about the window — the
//! `load_older` gap is structurally impossible. And because the registration
//! helper builds BOTH closures from one `FeedWindowSource`, you cannot register a
//! sidecar without its provider.

use std::sync::{Arc, Mutex};

use crate::root_indexed::FeedAuthorRefs;

/// A feed's visible-window source, materialized once per snapshot tick.
///
/// Wraps the live `snapshot_current_window` reader and memoizes its result for
/// the current tick rev. The first caller in a tick (the author provider, which
/// runs first in `make_update`) materializes; the second (the typed producer)
/// reuses the cached `Arc<S>`. A new tick rev invalidates the memo, so a feed
/// whose window changed between ticks re-materializes.
///
/// `S` is the concrete snapshot type (`RootFeedSnapshot<C, A>`); it must
/// implement [`FeedAuthorRefs`] so the provider can extract the visible author
/// keys from the SAME snapshot the typed producer encodes.
pub struct FeedWindowSource<S> {
    /// The live window reader. Reads the engine's current viewport (honoring any
    /// `load_older` grow). Non-blocking (D8) — it only reads in-memory state.
    snapshot_fn: Arc<dyn Fn() -> S + Send + Sync + 'static>,
    /// Memoized `(tick_rev, snapshot)` for the current tick. `None` before the
    /// first materialization. `Mutex` because the typed-producer and provider
    /// closures must both be `Sync`; uncontended in production (only the actor
    /// thread drives ticks, and the two reads are sequential within one tick).
    memo: Mutex<Option<(u64, Arc<S>)>>,
}

impl<S> FeedWindowSource<S>
where
    S: FeedAuthorRefs + Send + Sync + 'static,
{
    /// Construct a window source over a live window reader.
    #[must_use]
    pub fn new(snapshot_fn: impl Fn() -> S + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            snapshot_fn: Arc::new(snapshot_fn),
            memo: Mutex::new(None),
        })
    }

    /// Return the materialized window snapshot for `tick_rev`, materializing it
    /// (and caching it) on the first call this tick.
    ///
    /// Both the author provider and the typed producer call this with the SAME
    /// `tick_rev` (the kernel publishes ONE rev per `make_update`), so they share
    /// one materialization and cannot disagree about the visible window — closing
    /// the `load_older` 1-frame gap. A poisoned memo mutex (D6) degrades to a
    /// fresh, un-memoized materialization (correct, just not shared this tick).
    #[must_use]
    pub fn snapshot_for_tick(&self, tick_rev: u64) -> Arc<S> {
        if let Ok(mut memo) = self.memo.lock() {
            if let Some((cached_rev, snapshot)) = memo.as_ref() {
                if *cached_rev == tick_rev {
                    return Arc::clone(snapshot);
                }
            }
            let snapshot = Arc::new((self.snapshot_fn)());
            *memo = Some((tick_rev, Arc::clone(&snapshot)));
            return snapshot;
        }
        // Poisoned mutex: materialize fresh without memoizing (D6 — never panic).
        Arc::new((self.snapshot_fn)())
    }

    /// The visible author keys of the materialized window for `tick_rev`.
    ///
    /// Derived from the SAME `Arc<S>` [`Self::snapshot_for_tick`] returns, so the
    /// author set the provider resolves is byte-for-byte the window the typed
    /// producer emits.
    #[must_use]
    pub fn author_keys_for_tick(&self, tick_rev: u64) -> Vec<String> {
        self.snapshot_for_tick(tick_rev)
            .visible_author_keys()
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root_indexed::{CardAuthors, RootCard, RootFeedSnapshot};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone, serde::Serialize)]
    struct Card {
        author: String,
    }
    impl CardAuthors for Card {
        fn rendered_author_keys(&self) -> Vec<String> {
            vec![self.author.clone()]
        }
    }

    fn snap(authors: &[&str]) -> RootFeedSnapshot<Card, ()> {
        RootFeedSnapshot {
            cards: authors
                .iter()
                .map(|a| RootCard {
                    card: Card {
                        author: a.to_string(),
                    },
                    attribution: Vec::new(),
                })
                .collect(),
            page: None,
            metrics: None,
        }
    }

    /// One materialization per tick: the provider and the typed producer (two
    /// reads at the SAME tick rev) get ONE underlying snapshot, even if the
    /// live window would have changed between the two reads.
    #[test]
    fn materializes_once_per_tick_no_load_older_gap() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_fn = Arc::clone(&calls);
        // The "live window" widens on every read (simulating a concurrent
        // `load_older` between the provider read and the typed-producer read).
        let source = FeedWindowSource::new(move || {
            let n = calls_for_fn.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                snap(&["alice"])
            } else {
                snap(&["alice", "bob"]) // would-be wider window on a second read
            }
        });

        // Tick 7: provider reads author keys, then the typed producer reads the
        // window — both at tick rev 7.
        let provider_authors = source.author_keys_for_tick(7);
        let emitted = source.snapshot_for_tick(7);

        // Exactly ONE materialization happened despite two reads.
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "one materialization per tick"
        );
        // The provider's author set == the emitted window's author set (no gap).
        assert_eq!(provider_authors, vec!["alice".to_string()]);
        assert_eq!(
            emitted
                .visible_author_keys()
                .into_iter()
                .collect::<Vec<_>>(),
            vec!["alice".to_string()],
            "typed producer emits the SAME window the provider resolved"
        );
    }

    /// A new tick rev re-materializes (a feed whose window grew between ticks
    /// emits the new window next tick).
    #[test]
    fn new_tick_rev_rematerializes() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_fn = Arc::clone(&calls);
        let source = FeedWindowSource::new(move || {
            let n = calls_for_fn.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                snap(&["alice"])
            } else {
                snap(&["alice", "bob"])
            }
        });

        let t1 = source.author_keys_for_tick(1);
        assert_eq!(t1, vec!["alice".to_string()]);
        // Next tick: re-materialize, now wider.
        let t2 = source.author_keys_for_tick(2);
        assert_eq!(t2, vec!["alice".to_string(), "bob".to_string()]);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
