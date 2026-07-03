use crate::{
    XrayCommandOutcome, XrayInterestDescriptor, XrayOwnerCounts, XrayProjectionContext, XrayReason,
    XrayReasonCode, XrayReceipt, XrayReceiptEventKind, XrayTimestamp, XrayTransactionMarker,
};

use super::{correlate_receipts_with_wire_subscriptions, XrayWireSubscriptionSnapshot};

fn receipt(event: XrayReceiptEventKind, planner_interest_id: &str) -> XrayReceipt {
    let mut interest = XrayInterestDescriptor::new(
        "interest-a",
        "active-account",
        "lifecycle=tailing:shape=redacted",
        "active-follow-timeline",
    );
    interest.planner_interest_id_hint = Some(planner_interest_id.to_string());
    XrayReceipt::new(
        XrayProjectionContext::new(
            "chirp.feed",
            "root-indexed",
            "owner",
            XrayReason::new(XrayReasonCode::FeedSessionSync),
        ),
        XrayTransactionMarker::new(1, 1),
        XrayTimestamp::new(10),
        event,
        "resource-a",
        Some(interest),
    )
    .with_owner_counts(XrayOwnerCounts::known(1, 0))
}

#[test]
fn open_receipt_attaches_relay_effects() {
    let receipts = correlate_receipts_with_wire_subscriptions(
        vec![receipt(XrayReceiptEventKind::Open, "interest-a")],
        &[
            XrayWireSubscriptionSnapshot::new("sub-a", "wss://relay.example", "live", 1, 3)
                .with_originating_interest_ids(["interest-a"]),
        ],
    );

    assert_eq!(receipts[0].relay_effects.len(), 1);
    assert_eq!(
        receipts[0].relay_effects[0].relay_url,
        "wss://relay.example"
    );
    assert_eq!(receipts[0].relay_effects[0].events_rx, 3);
    assert_eq!(receipts[0].outcome, XrayCommandOutcome::applied());
}

#[test]
fn close_receipt_marks_wire_retained_when_socket_stays_open() {
    let receipts = correlate_receipts_with_wire_subscriptions(
        vec![receipt(XrayReceiptEventKind::Close, "interest-a")],
        &[
            XrayWireSubscriptionSnapshot::new("sub-a", "wss://relay.example", "live", 1, 0)
                .with_originating_interest_ids(["interest-a"]),
        ],
    );

    assert_eq!(receipts[0].outcome, XrayCommandOutcome::retained());
    assert_eq!(
        receipts[0].relay_effects[0].outcome,
        XrayCommandOutcome::retained()
    );
}

#[test]
fn close_receipt_without_wire_row_marks_last_owner_applied() {
    let receipts = correlate_receipts_with_wire_subscriptions(
        vec![receipt(XrayReceiptEventKind::Close, "interest-a")],
        &[],
    );

    assert!(receipts[0].relay_effects.is_empty());
    assert_eq!(receipts[0].outcome, XrayCommandOutcome::applied());
}

#[test]
fn eose_with_zero_events_is_visible_on_relay_effect() {
    let receipts = correlate_receipts_with_wire_subscriptions(
        vec![receipt(XrayReceiptEventKind::Open, "interest-a")],
        &[
            XrayWireSubscriptionSnapshot::new("sub-a", "wss://relay.example", "live", 1, 0)
                .with_eose_observed(true)
                .with_originating_interest_ids(["interest-a"]),
        ],
    );

    assert_eq!(
        receipts[0].relay_effects[0].outcome,
        XrayCommandOutcome::unknown("eose_zero_events")
    );
    assert_eq!(
        receipts[0].outcome,
        XrayCommandOutcome::unknown("eose_zero_events")
    );
}

#[test]
fn unmatched_open_receipt_is_uncorrelated_not_pending() {
    let receipts = correlate_receipts_with_wire_subscriptions(
        vec![receipt(XrayReceiptEventKind::Open, "interest-a")],
        &[],
    );

    assert_eq!(
        receipts[0].outcome,
        XrayCommandOutcome::unknown("uncorrelated")
    );
}

#[test]
fn exact_wire_id_hint_can_still_join_rows() {
    let mut receipt = receipt(XrayReceiptEventKind::Open, "interest-a");
    receipt
        .interest
        .as_mut()
        .expect("test receipt has interest")
        .wire_id_hint = Some("sub-a".to_string());

    let receipts = correlate_receipts_with_wire_subscriptions(
        vec![receipt],
        &[XrayWireSubscriptionSnapshot::new(
            "sub-a",
            "wss://relay.example",
            "live",
            1,
            1,
        )],
    );

    assert_eq!(receipts[0].relay_effects.len(), 1);
    assert_eq!(receipts[0].outcome, XrayCommandOutcome::applied());
}
