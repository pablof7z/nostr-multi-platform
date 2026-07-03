//! Neutral wire-subscription diagnostic seam (issues #2868/#2891, epic #2858).
//!
//! The dev-only `nmp-devtools` X-Ray surface needs per-wire-subscription facts
//! (relay URL, open/close state, consumer count, events received, EOSE-observed,
//! close reason) to join against live feed-session diagnostic receipts. Those
//! facts live in the kernel's `WireSubscriptionState`. This module exposes them
//! as a plain-data row so devtools can *obtain* them WITHOUT the kernel ever
//! depending on devtools — the dependency points the other way, exactly like the
//! `FeedSessionDiagnosticsSink` seam in `nmp-feed-session`. The kernel emits the
//! neutral facts; devtools translates them into its own X-Ray vocabulary.

use super::{Kernel, WireSubscriptionStatus};
use crate::planner::InterestId;
use std::collections::{BTreeMap, BTreeSet};

/// One neutral fact row per active `(relay_url, wire_id)` subscription.
///
/// Substrate-owned and protocol-generic: no X-Ray / devtools vocabulary leaks
/// in. `nmp-devtools` maps this into its `XrayWireSubscriptionSnapshot`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireSubscriptionDiagnosticSnapshot {
    /// Full wire id (hex REQ subscription id).
    pub wire_id: String,
    /// Resolved relay URL this subscription was opened on.
    pub relay_url: String,
    /// Raw state string, e.g. `"open"`, `"opening"`, `"closed"`.
    pub state: String,
    /// Logical consumer count for this wire subscription, computed by joining
    /// the current plan's originating interests to the registry's owner
    /// refcounts. Rows no longer present in the current plan report `0`.
    pub consumer_count: u32,
    /// EVENT frames received on this wire subscription.
    pub events_rx: u64,
    /// `true` once EOSE has been observed for this subscription.
    pub eose_observed: bool,
    /// Close-reason prose, or `None` while the subscription is still open.
    pub close_reason: Option<String>,
    /// Planner logical interests whose compiled `SubShape` produced this wire
    /// subscription. Empty when the row is no longer present in the current
    /// compiled plan.
    pub originating_interest_ids: Vec<u64>,
}

impl Kernel {
    /// Neutral wire-subscription diagnostic rows for the X-Ray seam.
    ///
    /// `enabled` gates the disabled path to zero cost: when X-Ray recording is
    /// off the caller passes `false` and no wire-sub state is walked or cloned —
    /// mirroring `XrayFeedSessionRecorder`'s `is_enabled()` short-circuit. Rows
    /// are ordered by `wire_id` (inherited from `wire_subscriptions()`).
    #[must_use]
    pub fn wire_subscription_diagnostics(
        &self,
        enabled: bool,
    ) -> Vec<WireSubscriptionDiagnosticSnapshot> {
        if !enabled {
            return Vec::new();
        }
        let origins = self.lifecycle.current_plan_wire_origins();
        let owner_counts = self.lifecycle.current_interest_owner_counts();
        self.wire_subscriptions()
            .into_iter()
            .map(|status| {
                WireSubscriptionDiagnosticSnapshot::from_status(status, &origins, &owner_counts)
            })
            .collect()
    }
}

impl WireSubscriptionDiagnosticSnapshot {
    fn from_status(
        status: WireSubscriptionStatus,
        origins: &BTreeMap<(String, String), Vec<InterestId>>,
        owner_counts: &BTreeMap<InterestId, usize>,
    ) -> Self {
        let originating_interests = origins
            .get(&(status.relay_url.clone(), status.wire_id.clone()))
            .cloned()
            .unwrap_or_default();
        let consumer_count =
            consumer_count_for_originating_interests(&originating_interests, owner_counts);
        Self {
            wire_id: status.wire_id,
            relay_url: status.relay_url,
            state: status.state,
            consumer_count,
            events_rx: status.events_rx,
            eose_observed: status.eose_at_ms.is_some(),
            close_reason: status.close_reason,
            originating_interest_ids: originating_interests.into_iter().map(|id| id.0).collect(),
        }
    }
}

fn consumer_count_for_originating_interests(
    originating_interests: &[InterestId],
    owner_counts: &BTreeMap<InterestId, usize>,
) -> u32 {
    let mut seen = BTreeSet::new();
    let mut total = 0usize;
    for interest_id in originating_interests {
        if seen.insert(interest_id.0) {
            total = total.saturating_add(*owner_counts.get(interest_id).unwrap_or(&0));
        }
    }
    total.min(u32::MAX as usize) as u32
}

#[cfg(test)]
#[path = "wire_sub_diagnostics_tests.rs"]
mod tests;
