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
        self.wire_subscriptions()
            .into_iter()
            .map(WireSubscriptionDiagnosticSnapshot::from_status)
            .collect()
    }
}

impl WireSubscriptionDiagnosticSnapshot {
    fn from_status(status: WireSubscriptionStatus) -> Self {
        Self {
            wire_id: status.wire_id,
            relay_url: status.relay_url,
            state: status.state,
            consumer_count: status.logical_consumer_count,
            events_rx: status.events_rx,
            eose_observed: status.eose_at_ms.is_some(),
            close_reason: status.close_reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::relay::{CanonicalRelayUrl, DEFAULT_VISIBLE_LIMIT};
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
}
