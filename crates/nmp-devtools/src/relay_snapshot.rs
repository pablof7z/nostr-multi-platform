//! X-Ray wire-subscription snapshot rows + the mapping from the neutral
//! `nmp-core` diagnostic seam (`WireSubscriptionDiagnosticSnapshot`).
//!
//! This is the wire-subscription analogue of `receipts_from_feed_session_batch`:
//! the kernel owns the facts (via the [`nmp_core::WireSubscriptionDiagnosticSnapshot`]
//! seam), devtools translates them into its own X-Ray vocabulary. The kernel
//! never depends on devtools — the edge points devtools → core.
//!
use nmp_core::WireSubscriptionDiagnosticSnapshot;

/// X-Ray wire-subscription snapshot row, consumed by
/// `correlate_receipts_with_wire_subscriptions` to join wire-level facts against
/// live feed-session diagnostic receipts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XrayWireSubscriptionSnapshot {
    /// Full wire id (hex REQ subscription id) — the join key against receipts.
    pub wire_id: String,
    /// Relay URL this subscription was opened on.
    pub relay_url: String,
    /// Raw state string, e.g. `"open"`, `"opening"`, `"closed"`.
    pub state: String,
    /// Planner logical interests whose compiled sub-shape produced this row.
    pub originating_interest_ids: Vec<String>,
    /// Logical consumer count for this wire subscription.
    pub consumer_count: u32,
    /// EVENT frames received on this wire subscription.
    pub events_rx: u64,
    /// `true` once EOSE has been observed for this subscription.
    pub eose_observed: bool,
    /// Close-reason prose, or `None` while the subscription is still open.
    pub close_reason: Option<String>,
}

impl XrayWireSubscriptionSnapshot {
    #[must_use]
    pub fn new(
        wire_id: impl Into<String>,
        relay_url: impl Into<String>,
        state: impl Into<String>,
        consumer_count: u32,
        events_rx: u64,
    ) -> Self {
        Self {
            wire_id: wire_id.into(),
            relay_url: relay_url.into(),
            state: state.into(),
            originating_interest_ids: Vec::new(),
            consumer_count,
            events_rx,
            eose_observed: false,
            close_reason: None,
        }
    }

    #[must_use]
    pub fn with_originating_interest_ids<I, S>(mut self, interest_ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.originating_interest_ids = interest_ids.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub fn with_eose_observed(mut self, eose_observed: bool) -> Self {
        self.eose_observed = eose_observed;
        self
    }

    #[must_use]
    pub fn with_close_reason(mut self, close_reason: impl Into<String>) -> Self {
        self.close_reason = Some(close_reason.into());
        self
    }
}

/// Translate neutral `nmp-core` wire-subscription diagnostic rows into the X-Ray
/// vocabulary. Pure mapping — the kernel already owns and shapes the facts.
#[must_use]
pub fn snapshots_from_wire_subscription_diagnostics(
    rows: &[WireSubscriptionDiagnosticSnapshot],
) -> Vec<XrayWireSubscriptionSnapshot> {
    rows.iter().map(snapshot_from_row).collect()
}

fn snapshot_from_row(row: &WireSubscriptionDiagnosticSnapshot) -> XrayWireSubscriptionSnapshot {
    XrayWireSubscriptionSnapshot {
        wire_id: row.wire_id.clone(),
        relay_url: row.relay_url.clone(),
        state: row.state.clone(),
        originating_interest_ids: row
            .originating_interest_ids
            .iter()
            .map(u64::to_string)
            .collect(),
        consumer_count: row.consumer_count,
        events_rx: row.events_rx,
        eose_observed: row.eose_observed,
        close_reason: row.close_reason.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        wire_id: &str,
        state: &str,
        consumer_count: u32,
        events_rx: u64,
        eose_observed: bool,
        close_reason: Option<&str>,
    ) -> WireSubscriptionDiagnosticSnapshot {
        WireSubscriptionDiagnosticSnapshot {
            wire_id: wire_id.to_string(),
            relay_url: "wss://relay.example/".to_string(),
            state: state.to_string(),
            consumer_count,
            events_rx,
            eose_observed,
            close_reason: close_reason.map(str::to_string),
            originating_interest_ids: vec![42],
        }
    }

    /// The four #2868 acceptance scenarios, mapped from the neutral seam rows the
    /// kernel produces into the X-Ray snapshot rows `correlate_*` consumes.
    #[test]
    fn acceptance_scenarios_map_into_xray_snapshots() {
        let rows = vec![
            // (a) Open subscription (with relay effect): fresh REQ, no events/EOSE.
            row("sub-a", "open", 1, 0, false, None),
            // (b) Close that keeps the socket open due to a retained owner: the
            // wire is still "open" with no close reason.
            row("sub-b", "open", 1, 3, false, None),
            // (c) Close that drops the last owner: socket torn down.
            row("sub-c", "closed", 0, 3, true, Some("last-owner-dropped")),
            // (d) Open + EOSE + zero events.
            row("sub-d", "open", 1, 0, true, None),
        ];

        let snapshots = snapshots_from_wire_subscription_diagnostics(&rows);
        assert_eq!(snapshots.len(), 4);

        let a = &snapshots[0];
        assert_eq!(a.wire_id, "sub-a");
        assert_eq!(a.originating_interest_ids, ["42".to_string()]);
        assert_eq!(a.state, "open");
        assert!(!a.eose_observed);
        assert_eq!(a.events_rx, 0);
        assert!(a.close_reason.is_none());
        assert_eq!(a.relay_url, "wss://relay.example/");

        let b = &snapshots[1];
        assert_eq!(b.state, "open");
        assert!(b.close_reason.is_none());
        assert_eq!(b.events_rx, 3);

        let c = &snapshots[2];
        assert_eq!(c.state, "closed");
        assert_eq!(c.close_reason.as_deref(), Some("last-owner-dropped"));
        assert_eq!(c.consumer_count, 0);

        let d = &snapshots[3];
        assert_eq!(d.state, "open");
        assert!(d.eose_observed);
        assert_eq!(d.events_rx, 0);
    }

    #[test]
    fn empty_rows_map_to_empty_snapshots() {
        assert!(snapshots_from_wire_subscription_diagnostics(&[]).is_empty());
    }
}
