//! Pagination controller for backwards timeline backfill.
//!
//! When a user scrolls to the end of locally-cached events, the feed engine
//! emits a backfill request through a closure sink (D7). The kernel routes
//! this to `PaginationController`, which:
//!
//! - **Deduplicates:** If a backfill for the same feed+boundary is already in-flight,
//!   skip (repeated scroll calls coalesce).
//! - **Gates against coverage:** Consult the `WatermarkFn` to check if the relay
//!   is already fully synced to (or past) the requested `until` depth. Suppress
//!   the REQ if it is (no point asking for a known absence).
//! - **Registers a bounded interest:** Create a `OneShot` interest with
//!   `until = oldest_ts - 1` (NIP-01 inclusive boundary fix) and
//!   deduplicate via `InterestRegistry::ensure_sub` keyed by `(feed_key, shape)`.
//!
//! The controller is protocol-agnostic; it receives the oldest timestamp and
//! feed ID, and emits an `InterestShape`. The wiring layer (nmp-nip01) translates
//! the feed engine's `BackfillRequest` into a kernel call with these parameters,
//! determines relay routing (D5: outbox-based), and REQ formatting.

use std::collections::BTreeMap;

use crate::planner::{InterestId, InterestShape};

/// Pagination state per feed: tracks in-flight backfill cursor, interest ID, and
/// coverage-complete gate.
#[derive(Clone, Debug)]
struct BackfillState {
    /// Feed key (e.g., "home-feed", "profile-0x123...").
    feed_key: String,
    /// Timestamp of the oldest locally-cached event.
    oldest_ts: u64,
    /// Interest ID once registered with the registry (None before registration).
    interest_id: Option<InterestId>,
    /// True iff coverage gating suppressed this request (relay fully synced to
    /// this depth, no REQ needed).
    coverage_complete: bool,
    /// True iff EOSE received on this interest (completion signal).
    eose_seen: bool,
}

/// Kernel-side pagination controller. Owns backwards-fetch cursor dedup,
/// coverage gating, and interest lifecycle for timeline backfill.
///
/// Lives in `SubscriptionLifecycle` as a D4 single-registration point.
#[derive(Debug, Default)]
pub struct PaginationController {
    /// Per-feed backfill state, keyed by feed_key.
    backfills: BTreeMap<String, BackfillState>,
}

impl PaginationController {
    /// Construct an empty controller.
    #[must_use]
    pub fn new() -> Self {
        Self {
            backfills: BTreeMap::new(),
        }
    }

    /// Called by the kernel when a feed's backfill request arrives. The wiring
    /// layer (nmp-nip01) translates the feed engine's `BackfillRequest` into a
    /// call to this method with extracted parameters.
    ///
    /// Returns an `InterestShape` if a new interest should be registered
    /// (backfill gate passed, dedup not hit), or `None` if:
    /// - The same feed+boundary is already in-flight (dedup).
    /// - The relay is fully synced to this depth (coverage gate).
    ///
    /// # Arguments
    /// - `feed_key`: Unique identifier for this feed (e.g., "home-feed", "profile-...")
    /// - `oldest_ts`: Unix seconds of the oldest locally-cached event
    /// - `authors`: Author pubkeys to backfill for (from view context)
    /// - `kinds`: Event kinds to backfill for (from view context)
    /// - `watermark_fn`: Optional coverage watermark resolver (from kernel)
    pub fn request_backfill(
        &mut self,
        feed_key: &str,
        oldest_ts: u64,
        authors: Vec<String>,
        kinds: Vec<u32>,
        watermark_fn: Option<&crate::subs::WatermarkFn>,
    ) -> Option<InterestShape> {
        // Dedup: if this feed is already backfilling, skip.
        if self.backfills.contains_key(feed_key) {
            return None;
        }

        // Coverage gating: check if the relay is fully synced to (or past) the
        // requested `until` depth.
        let until = oldest_ts.saturating_sub(1);  // NIP-01 inclusive fix: until is inclusive

        let shape = InterestShape {
            authors: authors.into_iter().collect(),
            kinds: kinds.into_iter().collect(),
            until: Some(until),
            limit: Some(200),  // Configurable backfill page size
            ..Default::default()
        };

        if let Some(watermark) = watermark_fn {
            if let Some(watermark_ts) = watermark(&shape) {
                if watermark_ts >= until {
                    // Watermark is at or past `until`; relay is fully synced.
                    // No REQ needed; record as coverage-complete.
                    self.backfills.insert(
                        feed_key.to_string(),
                        BackfillState {
                            feed_key: feed_key.to_string(),
                            oldest_ts,
                            interest_id: None,
                            coverage_complete: true,
                            eose_seen: true,  // Mark as done.
                        },
                    );
                    return None;
                }
            }
        }

        // Register the backfill state (interest_id filled in by caller after registration).
        self.backfills.insert(
            feed_key.to_string(),
            BackfillState {
                feed_key: feed_key.to_string(),
                oldest_ts,
                interest_id: None,
                coverage_complete: false,
                eose_seen: false,
            },
        );

        Some(shape)
    }

    /// Called by the kernel after the interest is registered.
    /// Updates the backfill state with the assigned interest ID.
    pub fn record_interest_id(&mut self, feed_key: &str, interest_id: InterestId) {
        if let Some(state) = self.backfills.get_mut(feed_key) {
            state.interest_id = Some(interest_id);
        }
    }

    /// Called by the planner after EOSE on a backfill interest.
    pub fn on_eose(&mut self, interest_id: &InterestId) {
        for state in self.backfills.values_mut() {
            if state.interest_id.as_ref() == Some(interest_id) {
                state.eose_seen = true;
            }
        }
    }

    /// Release a backfill request. Called when the view closes or scrolling
    /// stops. Returns the interest ID if one was registered (caller should
    /// remove it from the registry).
    pub fn release_backfill(&mut self, feed_key: &str) -> Option<InterestId> {
        self.backfills.remove(feed_key).and_then(|state| state.interest_id)
    }

    /// Clear all backfill state (e.g., on identity change).
    pub fn reset(&mut self) {
        self.backfills.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_dedup_same_boundary() {
        let mut controller = PaginationController::new();

        // First request should succeed.
        let shape1 = controller.request_backfill(
            "home-feed",
            1000,
            vec!["author-1".to_string()],
            vec![1],
            None,
        );
        assert!(shape1.is_some());

        // Second request for the same feed should be deduped.
        let shape2 = controller.request_backfill(
            "home-feed",
            1000,
            vec!["author-1".to_string()],
            vec![1],
            None,
        );
        assert!(shape2.is_none());
    }

    #[test]
    fn test_boundary_fix() {
        let mut controller = PaginationController::new();

        let shape = controller
            .request_backfill(
                "profile-feed",
                1000,
                vec!["author-1".to_string()],
                vec![1],
                None,
            )
            .expect("shape should be Some");

        // Until should be oldest - 1 (NIP-01 inclusive boundary fix).
        assert_eq!(shape.until, Some(999));
    }

    #[test]
    fn test_record_interest_id() {
        let mut controller = PaginationController::new();

        controller.request_backfill(
            "home-feed",
            1000,
            vec!["author-1".to_string()],
            vec![1],
            None,
        );

        let interest_id = InterestId(123);
        controller.record_interest_id("home-feed", interest_id.clone());

        // Verify the state is updated.
        assert_eq!(
            controller
                .backfills
                .get("home-feed")
                .and_then(|s| s.interest_id.clone()),
            Some(interest_id)
        );
    }

    #[test]
    fn test_coverage_gating_suppresses_req_when_watermark_complete() {
        use crate::planner::InterestShape;

        let mut controller = PaginationController::new();

        // Mock watermark function that returns a fixed timestamp >= until.
        let watermark_fn: crate::subs::WatermarkFn = Arc::new(|_: &InterestShape| {
            Some(1000)  // Watermark at 1000 (relay fully synced to this point)
        });

        // Request backfill for oldest_ts = 1000. The until will be 999, and
        // the watermark is at 1000, so it's >= until. No REQ should be issued.
        let shape = controller.request_backfill(
            "home-feed",
            1000,
            vec!["author-1".to_string()],
            vec![1],
            Some(&watermark_fn),
        );

        // Request should be suppressed (None returned).
        assert!(shape.is_none());

        // Verify the backfill was recorded as coverage-complete.
        let state = controller.backfills.get("home-feed");
        assert!(state.is_some());
        assert!(state.unwrap().coverage_complete);
        assert!(state.unwrap().eose_seen);
    }

    #[test]
    fn test_coverage_gating_allows_req_when_watermark_incomplete() {
        use crate::planner::InterestShape;

        let mut controller = PaginationController::new();

        // Mock watermark function that returns a timestamp < until.
        let watermark_fn: crate::subs::WatermarkFn = Arc::new(|_: &InterestShape| {
            Some(998)  // Watermark at 998 (relay not synced to 999)
        });

        // Request backfill for oldest_ts = 1000. The until will be 999, and
        // the watermark is 998, so watermark < until. REQ should be issued.
        let shape = controller.request_backfill(
            "home-feed",
            1000,
            vec!["author-1".to_string()],
            vec![1],
            Some(&watermark_fn),
        );

        // Request should be allowed (Some returned).
        assert!(shape.is_some());
        assert_eq!(shape.unwrap().until, Some(999));

        // Verify the backfill was recorded but not coverage-complete.
        let state = controller.backfills.get("home-feed");
        assert!(state.is_some());
        assert!(!state.unwrap().coverage_complete);
    }
}
