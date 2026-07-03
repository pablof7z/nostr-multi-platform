use std::collections::BTreeMap;

use crate::{
    XrayCommandOutcome, XrayOutcomeStatus, XrayReceipt, XrayReceiptEventKind, XrayRelayEffect,
    XrayWireSubscriptionSnapshot,
};

#[must_use]
pub fn correlate_receipts_with_wire_subscriptions(
    receipts: Vec<XrayReceipt>,
    wire_subscriptions: &[XrayWireSubscriptionSnapshot],
) -> Vec<XrayReceipt> {
    let index = CorrelationIndex::new(wire_subscriptions);
    receipts
        .into_iter()
        .map(|receipt| correlate_receipt(receipt, &index))
        .collect()
}

struct CorrelationIndex<'a> {
    by_wire: BTreeMap<&'a str, Vec<&'a XrayWireSubscriptionSnapshot>>,
    by_interest: BTreeMap<&'a str, Vec<&'a XrayWireSubscriptionSnapshot>>,
}

impl<'a> CorrelationIndex<'a> {
    fn new(rows: &'a [XrayWireSubscriptionSnapshot]) -> Self {
        let mut by_wire: BTreeMap<&str, Vec<&XrayWireSubscriptionSnapshot>> = BTreeMap::new();
        let mut by_interest: BTreeMap<&str, Vec<&XrayWireSubscriptionSnapshot>> = BTreeMap::new();
        for row in rows {
            by_wire.entry(&row.wire_id).or_default().push(row);
            for interest_id in &row.originating_interest_ids {
                by_interest.entry(interest_id).or_default().push(row);
            }
        }
        Self {
            by_wire,
            by_interest,
        }
    }
}

fn correlate_receipt(receipt: XrayReceipt, index: &CorrelationIndex<'_>) -> XrayReceipt {
    let Some(interest) = receipt.interest.as_ref() else {
        return receipt;
    };
    let rows = interest
        .wire_id_hint
        .as_deref()
        .and_then(|wire_id| index.by_wire.get(wire_id))
        .or_else(|| {
            interest
                .planner_interest_id_hint
                .as_deref()
                .and_then(|interest_id| index.by_interest.get(interest_id))
        });
    let Some(rows) = rows else {
        let outcome = outcome_without_matching_row(&receipt);
        return receipt.with_outcome(outcome);
    };
    let effects = rows
        .iter()
        .map(|row| relay_effect(row, receipt.event))
        .collect::<Vec<_>>();
    let outcome = outcome_from_effects(receipt.event, &effects);
    receipt.with_relay_effects(effects).with_outcome(outcome)
}

fn relay_effect(
    row: &XrayWireSubscriptionSnapshot,
    event: XrayReceiptEventKind,
) -> XrayRelayEffect {
    XrayRelayEffect::new(
        row.relay_url.clone(),
        Some(row.wire_id.clone()),
        row.state.clone(),
        row.consumer_count,
        row.events_rx,
    )
    .with_outcome(effect_outcome(row, event))
}

fn outcome_without_matching_row(receipt: &XrayReceipt) -> XrayCommandOutcome {
    match receipt.event {
        XrayReceiptEventKind::Close if receipt.owner_counts.after == Some(0) => {
            XrayCommandOutcome::applied()
        }
        XrayReceiptEventKind::Close => XrayCommandOutcome::unknown("uncorrelated"),
        XrayReceiptEventKind::Open
        | XrayReceiptEventKind::Replace
        | XrayReceiptEventKind::Refresh => XrayCommandOutcome::unknown("uncorrelated"),
    }
}

fn outcome_from_effects(
    event: XrayReceiptEventKind,
    effects: &[XrayRelayEffect],
) -> XrayCommandOutcome {
    if effects
        .iter()
        .any(|effect| effect.outcome.code == "eose_zero_events")
    {
        return XrayCommandOutcome::unknown("eose_zero_events");
    }
    if matches!(event, XrayReceiptEventKind::Close)
        && effects
            .iter()
            .any(|effect| matches!(effect.outcome.status, XrayOutcomeStatus::Retained))
    {
        return XrayCommandOutcome::retained();
    }
    if effects
        .iter()
        .any(|effect| matches!(effect.outcome.status, XrayOutcomeStatus::Pending))
    {
        return XrayCommandOutcome::pending("wire_pending");
    }
    XrayCommandOutcome::applied()
}

fn effect_outcome(
    row: &XrayWireSubscriptionSnapshot,
    event: XrayReceiptEventKind,
) -> XrayCommandOutcome {
    if row.eose_observed && row.events_rx == 0 {
        return XrayCommandOutcome::unknown("eose_zero_events");
    }
    if row.close_reason.is_some() {
        return XrayCommandOutcome::failed("relay_closed");
    }
    match event {
        XrayReceiptEventKind::Close if row.consumer_count > 0 && is_open_state(&row.state) => {
            XrayCommandOutcome::retained()
        }
        XrayReceiptEventKind::Open
        | XrayReceiptEventKind::Replace
        | XrayReceiptEventKind::Refresh
            if is_pending_state(&row.state) =>
        {
            XrayCommandOutcome::pending("wire_pending")
        }
        _ => XrayCommandOutcome::applied(),
    }
}

fn is_open_state(state: &str) -> bool {
    matches!(state, "open" | "live" | "active" | "opening")
}

fn is_pending_state(state: &str) -> bool {
    matches!(state, "opening" | "auth_paused" | "pending")
}

#[cfg(test)]
#[path = "relay_correlation_tests.rs"]
mod relay_correlation_tests;
