//! Neutral wire-subscription diagnostic seam (issue #2868, epic #2858).
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
use std::collections::BTreeMap;

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
    /// Logical consumer count for this wire subscription. NOTE: the kernel's
    /// `wire_subscriptions()` status path currently hardcodes this to `1` (the
    /// true per-sub refcount lives in the `subs` registry and is not yet joined
    /// into the wire-sub row). Carried through honestly; see #2868 gap notes.
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
        self.wire_subscriptions()
            .into_iter()
            .map(|status| WireSubscriptionDiagnosticSnapshot::from_status(status, &origins))
            .collect()
    }
}

impl WireSubscriptionDiagnosticSnapshot {
    fn from_status(
        status: WireSubscriptionStatus,
        origins: &BTreeMap<(String, String), Vec<InterestId>>,
    ) -> Self {
        let originating_interest_ids = origins
            .get(&(status.relay_url.clone(), status.wire_id.clone()))
            .map(|ids| ids.iter().map(|id| id.0).collect())
            .unwrap_or_default();
        Self {
            wire_id: status.wire_id,
            relay_url: status.relay_url,
            state: status.state,
            consumer_count: status.logical_consumer_count,
            events_rx: status.events_rx,
            eose_observed: status.eose_at_ms.is_some(),
            close_reason: status.close_reason,
            originating_interest_ids,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::planner::{
        InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
        LogicalInterest, MailboxSnapshot,
    };
    use crate::relay::{CanonicalRelayUrl, DEFAULT_VISIBLE_LIMIT};
    use crate::subs::WireFrame;
    use crate::time::Instant;
    use nmp_network::role::RelayRole;

    use super::super::wire_sub::WireSub;
    use super::super::Kernel;

    const RELAY: &str = "wss://relay.example/";

    fn kernel() -> Kernel {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        kernel.start();
        kernel
    }

    fn pubkey(s: &str) -> String {
        format!("{s:0>64}").chars().take(64).collect()
    }

    fn open_sub(kernel: &mut Kernel, sub_id: &str, state: &str) {
        kernel.insert_wire_sub(
            RelayRole::Content,
            CanonicalRelayUrl::parse_or_raw(RELAY),
            sub_id.to_string(),
            "kinds=[1]".to_string(),
            state,
            None,
        );
    }

    /// Directly seed lifecycle state (EOSE / events / close) on an already-open
    /// wire sub. The real ingest path sets these via EOSE / EVENT / CLOSED frame
    /// handlers; the seam only reads them, so seeding is faithful for this test.
    fn mutate_sub(kernel: &mut Kernel, sub_id: &str, f: impl Fn(&mut WireSub)) {
        for sub in kernel.wire.subs.values_mut() {
            if sub.id == sub_id {
                f(sub);
            }
        }
    }

    fn row<'a>(
        rows: &'a [super::WireSubscriptionDiagnosticSnapshot],
        wire_id: &str,
    ) -> &'a super::WireSubscriptionDiagnosticSnapshot {
        rows.iter()
            .find(|r| r.wire_id == wire_id)
            .expect("row present")
    }

    fn follow_interest(id: u64, authors: &[&str]) -> LogicalInterest {
        LogicalInterest {
            id: InterestId(id),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors: authors.iter().map(|author| pubkey(author)).collect(),
                kinds: [1u32].into_iter().collect(),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::Tailing,
            is_indexer_discovery: false,
        }
    }

    #[test]
    fn disabled_returns_empty_without_walking_subs() {
        let mut kernel = kernel();
        open_sub(&mut kernel, "sub-a", "open");
        assert!(kernel.wire_subscription_diagnostics(false).is_empty());
    }

    #[test]
    fn acceptance_scenarios_produce_neutral_rows() {
        let mut kernel = kernel();

        // (a) Open subscription (with relay effect): fresh REQ, no events, no EOSE.
        open_sub(&mut kernel, "sub-a", "open");

        // (b) Close that keeps the socket open due to a retained owner: the wire
        // stays "open" with no close reason (the CLOSE frame is never sent while
        // another logical owner holds the sub). The refcount transition itself is
        // carried by the feed-session receipt's owner_counts, not this row.
        open_sub(&mut kernel, "sub-b", "open");
        mutate_sub(&mut kernel, "sub-b", |s| s.events_rx = 3);

        // (c) Close that drops the last owner: the socket is torn down —
        // state "closed" with a close reason.
        open_sub(&mut kernel, "sub-c", "opening");
        mutate_sub(&mut kernel, "sub-c", |s| {
            s.state = "closed".to_string();
            s.close_reason = Some("last-owner-dropped".to_string());
        });

        // (d) Open + EOSE + zero events: EOSE observed, events_rx stays 0.
        open_sub(&mut kernel, "sub-d", "open");
        mutate_sub(&mut kernel, "sub-d", |s| s.eose_at = Some(Instant::now()));

        let rows = kernel.wire_subscription_diagnostics(true);
        assert_eq!(rows.len(), 4);

        let a = row(&rows, "sub-a");
        assert_eq!(a.state, "open");
        assert!(!a.eose_observed);
        assert_eq!(a.events_rx, 0);
        assert!(a.close_reason.is_none());
        // `insert_wire_sub` keys on the canonical URL (trailing slash stripped).
        assert_eq!(a.relay_url, "wss://relay.example");

        let b = row(&rows, "sub-b");
        assert_eq!(b.state, "open");
        assert!(b.close_reason.is_none());
        assert_eq!(b.events_rx, 3);

        let c = row(&rows, "sub-c");
        assert_eq!(c.state, "closed");
        assert_eq!(c.close_reason.as_deref(), Some("last-owner-dropped"));

        let d = row(&rows, "sub-d");
        assert_eq!(d.state, "open");
        assert!(d.eose_observed);
        assert_eq!(d.events_rx, 0);
    }

    #[test]
    fn diagnostics_carry_originating_interest_ids_from_current_plan() {
        let mut kernel = kernel();
        kernel
            .lifecycle
            .set_selection_budget(usize::MAX, usize::MAX);

        let mut cache = InMemoryMailboxCache::new();
        cache.put(
            pubkey("aa01"),
            MailboxSnapshot {
                write_relays: vec!["wss://relay-a.example".to_string()],
                read_relays: vec![],
                both_relays: vec![],
            },
        );
        cache.put(
            pubkey("bb02"),
            MailboxSnapshot {
                write_relays: vec!["wss://relay-b.example".to_string()],
                read_relays: vec![],
                both_relays: vec![],
            },
        );
        crate::subs::replace_test_interest(
            &mut kernel.lifecycle,
            follow_interest(991, &["aa01", "bb02"]),
        );

        let frames = kernel
            .lifecycle
            .recompile_and_diff(&cache)
            .expect("compile");
        assert!(
            frames
                .iter()
                .any(|frame| matches!(frame, WireFrame::Req { .. })),
            "compile must produce real content REQ frames"
        );
        for frame in kernel.lifecycle.current_plan_frames() {
            let WireFrame::Req {
                relay_url,
                sub_id,
                filter_json,
                ..
            } = frame
            else {
                continue;
            };
            kernel.insert_wire_sub(
                RelayRole::Content,
                CanonicalRelayUrl::parse_or_raw(&relay_url),
                sub_id,
                filter_json,
                "open",
                None,
            );
        }

        let rows = kernel.wire_subscription_diagnostics(true);
        assert_eq!(
            rows.len(),
            2,
            "author partitioning should produce one wire row per write relay"
        );
        for row in rows {
            assert_eq!(row.originating_interest_ids, vec![991]);
        }
    }
}
