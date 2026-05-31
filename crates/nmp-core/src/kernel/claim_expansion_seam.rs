//! Test-seam accessors for the W5 claim-expansion controller.
//!
//! Extracted from `claim_expansion.rs` to keep the production file under the
//! D-V12 500-LOC ceiling. All methods in this file are `#[cfg(any(test,
//! feature = "test-support"))]` — they compile only in test / integration-test
//! builds and are never part of the production binary.

use std::collections::BTreeSet;

use super::{
    claim_expansion::{PendingClaim, Phase},
    Kernel,
};

impl Kernel {
    /// Returns true if `pending_claims` is empty. Test seam.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn pending_claims_is_empty(&self) -> bool {
        self.pending_claims.is_empty()
    }

    /// Returns the count of pending claims. Test seam.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_pending_claims_count(&self) -> usize {
        self.pending_claims.len()
    }

    /// Returns the current phase of a claim identified by `primary_id`. Test seam.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_claim_phase(&self, primary_id: &str) -> Option<Phase> {
        self.pending_claims
            .values()
            .find(|c: &&PendingClaim| c.primary_id == primary_id)
            .map(|c| c.phase.clone())
    }

    /// Returns the in-flight relay count for a claim. Test seam.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_claim_in_flight_count(&self, primary_id: &str) -> usize {
        self.pending_claims
            .values()
            .find(|c: &&PendingClaim| c.primary_id == primary_id)
            .map(|c| {
                // Count unique relays in in_flight_attempts (not tuples)
                c.in_flight_attempts
                    .iter()
                    .map(|(relay, _)| relay.as_str())
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
            })
            .unwrap_or(0)
    }

    /// Returns all in-flight sub_ids for a claim (for compatibility). Test seam.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_claim_in_flight_sub_ids(&self, primary_id: &str) -> Vec<String> {
        self.pending_claims
            .values()
            .find(|c: &&PendingClaim| c.primary_id == primary_id)
            .map(|c| {
                c.in_flight_attempts
                    .iter()
                    .map(|(_, sub_id)| sub_id.clone())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns all in-flight (relay, sub_id) pairs for a claim. Test seam.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_claim_in_flight_attempts(
        &self,
        primary_id: &str,
    ) -> std::collections::BTreeSet<(String, String)> {
        self.pending_claims
            .values()
            .find(|c: &&PendingClaim| c.primary_id == primary_id)
            .map(|c| c.in_flight_attempts.clone())
            .unwrap_or_default()
    }

    /// Returns the count of entries in claim_sub_index. Test seam.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_claim_sub_index_len(&self) -> usize {
        self.claim_sub_index.len()
    }

    /// Returns the `InterestId` of the pending claim for `primary_id`, if any.
    /// Test seam: lets a test build the `WireFrame::Req { interest_id, … }`
    /// that the planner-frame bridge keys on, so `claim_sub_index` /
    /// `oneshot_subs` get wired exactly as in production.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_claim_interest_id(
        &self,
        primary_id: &str,
    ) -> Option<crate::planner::InterestId> {
        self.pending_claims
            .values()
            .find(|c: &&PendingClaim| c.primary_id == primary_id)
            .map(|c| c.interest_id.clone())
    }

    /// Returns the attempted relay set for a claim. Test seam.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_claim_attempted(&self, primary_id: &str) -> BTreeSet<String> {
        self.pending_claims
            .values()
            .find(|c: &&PendingClaim| c.primary_id == primary_id)
            .map(|c| c.attempted.clone())
            .unwrap_or_default()
    }

    /// Returns the count of attempted relays for a claim (0 if not found). Test seam.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_claim_attempted_count(&self, primary_id: &str) -> usize {
        self.pending_claims
            .values()
            .find(|c: &&PendingClaim| c.primary_id == primary_id)
            .map(|c| c.attempted.len())
            .unwrap_or(0)
    }

    /// Mark an event as known in the store (for §8.7 preflight testing). Test seam.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_mark_event_known(&mut self, primary_id: &str) {
        // Insert a minimal StoredEvent into the events map so `event_already_known` returns true.
        // `StoredEvent` lives in `kernel/types.rs` (pub(super) within the kernel module).
        use super::types::StoredEvent;
        let stored = StoredEvent {
            id: primary_id.to_string(),
            author: "0".repeat(64),
            kind: 1,
            created_at: 0,
            tags: vec![],
            content: String::new(),
            relay_count: 0,
        };
        self.events.insert(primary_id.to_string(), stored);
        self.cached_estimated_store_bytes.set(None);
    }

    /// Add a relay to a claim's attempted set. Test seam for §8.1 relay_failed tests.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_mark_claim_attempted(&mut self, primary_id: &str, relay_url: &str) {
        use crate::relay::CanonicalRelayUrl;
        if let Some(claim) = self
            .pending_claims
            .values_mut()
            .find(|c| c.primary_id == primary_id)
        {
            let canonical = CanonicalRelayUrl::parse_or_raw(relay_url).into_string();
            claim.attempted.insert(canonical);
        }
    }

    /// Returns `oneshot.in_flight()` — the number of registered OneshotTokens.
    /// §8.2: must stay at 1 per claim across Phase 1 → Phase 2 transition.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_oneshot_in_flight(&self) -> usize {
        self.oneshot.in_flight()
    }
}
