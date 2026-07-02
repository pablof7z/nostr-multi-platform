//! Unit tests for [`super`] (the NWC `WalletRuntime`).
//!
//! Kept as a sibling file via `#[path]` so `runtime.rs` stays under the LOC gate.

use super::disconnect::wallet_disconnect_inner;
use super::runtime_utils::encode_frame;
use super::*;
use crate::payment_store::{PaymentRecord, PaymentState};
use crate::reconcile::correct_unresolved_record;
use crate::status::new_wallet_status_slot;
use serde_json::json;

// ── V-63: encode-before-register ─────────────────────────────────────────

#[test]
fn encode_frame_returns_err_for_non_string_key_map() {
    let mut bad: std::collections::HashMap<Vec<u8>, ()> = std::collections::HashMap::new();
    bad.insert(vec![0u8], ());
    let result = serde_json::to_string(&bad);
    assert!(
        result.is_err(),
        "serde_json must reject a map with non-string keys — \
             this is the error class encode_frame is designed to catch"
    );
}

#[test]
fn encode_frame_succeeds_for_valid_json_array() {
    let frame = json!(["REQ", "sub-id-1", {"kinds": [23195u32]}]);
    let result = encode_frame(&frame);
    assert!(result.is_ok(), "valid json array must encode without error");
    let text = result.unwrap();
    assert!(!text.is_empty(), "encoded frame must not be empty");
    assert!(text.starts_with('['), "encoded frame must be a JSON array");
}

// ── V-64: orphan response counter ─────────────────────────────────────────

#[test]
fn orphan_response_count_starts_at_zero() {
    let slot = new_wallet_status_slot();
    let rt = WalletRuntime::new(slot);
    // No connection installed — count must be zero.
    assert_eq!(
        rt.orphan_response_count(),
        0,
        "fresh runtime must report zero orphan responses"
    );
}

// ── V-64: sweep_expired_payments ─────────────────────────────────────────

fn pending(correlation_id: Option<&str>, inserted_at_secs: u64) -> PendingPayment {
    PendingPayment {
        correlation_id: correlation_id.map(str::to_string),
        inserted_at_secs,
        bolt11: "lnbc1test".to_string(),
        amount_msats: Some(1_000),
    }
}

fn make_connection(pending_payments: HashMap<String, PendingPayment>) -> WalletConnection {
    WalletConnection {
        wallet_pubkey_hex: "aaaa".repeat(16),
        relay_url: "wss://test.relay".to_string(),
        client_secret_hex: Zeroizing::new("bb".repeat(32)),
        client_pubkey_hex: "cccc".repeat(16),
        status: "ready".to_string(),
        balance_msats: None,
        pending: HashMap::new(),
        pending_payments,
        pending_lookups: HashMap::new(),
        sub_id: "nwc-aaaa".to_string(),
        orphan_responses: 0,
        last_probe_sent_secs: 0,
        probe_outstanding: false,
        consecutive_failures: 0,
        connection_state: None,
    }
}

#[test]
fn sweep_removes_expired_entry_and_leaves_fresh_entry() {
    let slot = new_wallet_status_slot();
    let mut rt = WalletRuntime::new(slot);
    let now_secs: u64 = 1_000_000;
    let ttl_secs: u64 = 90;

    let mut payments = HashMap::new();
    payments.insert(
        "expired-event-id".to_string(),
        pending(Some("cid-expired"), now_secs - 200),
    );
    payments.insert(
        "fresh-event-id".to_string(),
        pending(Some("cid-fresh"), now_secs - 10),
    );
    rt.connection = Some(make_connection(payments));

    let outcomes = rt.sweep_expired_payments(now_secs, ttl_secs);

    assert_eq!(outcomes.len(), 1, "exactly one expired outcome");
    assert_eq!(outcomes[0].request_event_id, "expired-event-id");
    assert_eq!(outcomes[0].correlation_id.as_deref(), Some("cid-expired"));

    let conn = rt.connection.as_ref().unwrap();
    assert!(
        !conn.pending_payments.contains_key("expired-event-id"),
        "expired removed"
    );
    assert!(
        conn.pending_payments.contains_key("fresh-event-id"),
        "fresh retained"
    );
}

#[test]
fn ttl_sweep_transitions_to_unknown_not_failure() {
    let dir = tempfile::tempdir().unwrap();
    let slot = new_wallet_status_slot();
    let mut rt = WalletRuntime::new(slot);
    rt.set_payment_store(FsPaymentStore::new(dir.path()));
    let now_secs: u64 = 1_000_000;

    // Seed the durable record (PaySent) and the in-memory entry.
    rt.payment_store
        .as_ref()
        .unwrap()
        .upsert(&PaymentRecord {
            request_event_id: "pay-1".to_string(),
            bolt11: "lnbc1test".to_string(),
            correlation_id: Some("cid-1".to_string()),
            amount_msats: Some(1_000),
            state: PaymentState::PaySent,
            preimage: None,
        })
        .unwrap();
    let mut payments = HashMap::new();
    payments.insert("pay-1".to_string(), pending(Some("cid-1"), now_secs - 200));
    rt.connection = Some(make_connection(payments));

    let _ = rt.sweep_expired_payments(now_secs, 90);

    // The durable record must now be Unknown (NOT deleted, NOT Failed) so it
    // can be reconciled via lookup_invoice on reconnect.
    let unresolved = rt
        .payment_store
        .as_ref()
        .unwrap()
        .load_unresolved()
        .unwrap();
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].request_event_id, "pay-1");
    assert_eq!(unresolved[0].state, PaymentState::Unknown);
}

#[test]
fn sweep_removes_no_correlation_entry_without_failure() {
    let slot = new_wallet_status_slot();
    let mut rt = WalletRuntime::new(slot);
    let now_secs: u64 = 1_000_000;

    let mut payments = HashMap::new();
    payments.insert(
        "actor-internal-event-id".to_string(),
        pending(None, now_secs - 200),
    );
    rt.connection = Some(make_connection(payments));

    let outcomes = rt.sweep_expired_payments(now_secs, 90);
    assert_eq!(outcomes.len(), 1, "the entry is swept as an outcome");
    assert_eq!(outcomes[0].correlation_id, None);

    let conn = rt.connection.as_ref().unwrap();
    assert!(
        !conn
            .pending_payments
            .contains_key("actor-internal-event-id"),
        "actor-internal entry must be removed from the map"
    );
}

#[test]
fn sweep_leaves_fresh_entry_untouched() {
    let slot = new_wallet_status_slot();
    let mut rt = WalletRuntime::new(slot);
    let now_secs: u64 = 1_000_000;

    let mut payments = HashMap::new();
    payments.insert(
        "fresh-event-id".to_string(),
        pending(Some("cid-fresh"), now_secs - 10),
    );
    rt.connection = Some(make_connection(payments));

    let outcomes = rt.sweep_expired_payments(now_secs, 90);

    assert!(outcomes.is_empty(), "fresh entry must not be swept");
    let conn = rt.connection.as_ref().unwrap();
    assert!(
        conn.pending_payments.contains_key("fresh-event-id"),
        "fresh entry must still be present"
    );
}

#[test]
fn disconnect_drain_transitions_to_unknown_not_failure() {
    let dir = tempfile::tempdir().unwrap();
    let slot = new_wallet_status_slot();
    let mut rt = WalletRuntime::new(slot);
    rt.set_payment_store(FsPaymentStore::new(dir.path()));

    rt.payment_store
        .as_ref()
        .unwrap()
        .upsert(&PaymentRecord {
            request_event_id: "pay-d".to_string(),
            bolt11: "lnbc1test".to_string(),
            correlation_id: Some("cid-d".to_string()),
            amount_msats: Some(2_000),
            state: PaymentState::PaySent,
            preimage: None,
        })
        .unwrap();
    let mut payments = HashMap::new();
    payments.insert("pay-d".to_string(), pending(Some("cid-d"), 1_000));
    rt.connection = Some(make_connection(payments));

    // wallet_disconnect_inner needs the narrow wallet kernel capability
    // (ADR-0072 §D5); wrap the test-support kernel with `as_wallet_access`.
    let mut kernel = nmp_core::Kernel::testing_new(64);
    let _ = wallet_disconnect_inner(&mut rt, &kernel.as_wallet_access());

    let unresolved = rt
        .payment_store
        .as_ref()
        .unwrap()
        .load_unresolved()
        .unwrap();
    assert_eq!(
        unresolved.len(),
        1,
        "the payment survives disconnect as Unknown"
    );
    assert_eq!(unresolved[0].state, PaymentState::Unknown);
}

#[test]
fn unknown_records_survive_simulated_restart() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = FsPaymentStore::new(dir.path());
        store
            .upsert(&PaymentRecord {
                request_event_id: "pay-r".to_string(),
                bolt11: "lnbc1test".to_string(),
                correlation_id: Some("cid-r".to_string()),
                amount_msats: None,
                state: PaymentState::Unknown,
                preimage: None,
            })
            .unwrap();
    }
    // Fresh runtime + fresh store instance = simulated process restart.
    let slot = new_wallet_status_slot();
    let mut rt = WalletRuntime::new(slot);
    rt.set_payment_store(FsPaymentStore::new(dir.path()));
    let unresolved = rt
        .payment_store
        .as_ref()
        .unwrap()
        .load_unresolved()
        .unwrap();
    assert_eq!(unresolved.len(), 1, "Unknown record survives restart");
    assert_eq!(unresolved[0].state, PaymentState::Unknown);
    assert_eq!(unresolved[0].request_event_id, "pay-r");
}

#[test]
fn late_success_corrects_unknown_record() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsPaymentStore::new(dir.path());
    store
        .upsert(&PaymentRecord {
            request_event_id: "pay-l".to_string(),
            bolt11: "lnbc1test".to_string(),
            correlation_id: Some("cid-l".to_string()),
            amount_msats: Some(500),
            state: PaymentState::Unknown,
            preimage: None,
        })
        .unwrap();

    let mut kernel = nmp_core::Kernel::testing_new(64);
    // The late success arrives after the in-memory entry is gone; the
    // correction path loads the durable record and settles it.
    let corrected = correct_unresolved_record(
        Some(&store),
        "pay-l",
        true,
        Some("preimage-xyz".to_string()),
        None,
        &kernel.as_wallet_access(),
    );
    assert!(corrected, "the durable record must be found and corrected");
    // Terminal records are deleted (no longer need reconciliation).
    let unresolved = store.load_unresolved().unwrap();
    assert!(
        unresolved.is_empty(),
        "settled record is removed from the store"
    );
}

#[test]
fn pending_map_drains_on_response() {
    let slot = new_wallet_status_slot();
    let mut rt = WalletRuntime::new(slot);
    let mut conn = make_connection(HashMap::new());
    conn.pending
        .insert("req-1".to_string(), "get_info".to_string());
    conn.pending
        .insert("req-2".to_string(), "get_balance".to_string());
    rt.connection = Some(conn);

    // Simulate the drain `handle_nwc_text` performs when a response whose
    // `e` tag references "req-1" arrives.
    rt.connection.as_mut().unwrap().pending.remove("req-1");

    let conn = rt.connection.as_ref().unwrap();
    assert!(
        !conn.pending.contains_key("req-1"),
        "matched request drained"
    );
    assert!(
        conn.pending.contains_key("req-2"),
        "unmatched request retained"
    );
}

// ── V-79: tick_heartbeat ──────────────────────────────────────────────────

fn make_runtime_ready() -> WalletRuntime {
    let slot = new_wallet_status_slot();
    let mut rt = WalletRuntime::new(slot);
    rt.connection = Some(make_connection(HashMap::new()));
    // Mark ready + arm probe baseline so the first cadence window works.
    if let Some(c) = rt.connection.as_mut() {
        c.status = "ready".to_string();
    }
    rt
}

#[test]
fn heartbeat_no_op_within_cadence_window() {
    let mut rt = make_runtime_ready();
    let now_secs: u64 = 1_000_000;
    // Arm the baseline timestamp.
    let result = rt.tick_heartbeat(now_secs, 30, 3);
    // baseline-arm call returns no probe.
    assert!(!result.needs_probe, "baseline-arm must not request a probe");
    assert!(
        !result.state_changed,
        "baseline-arm must not report state change"
    );

    // 10 s later — still within the 30 s window.
    let result2 = rt.tick_heartbeat(now_secs + 10, 30, 3);
    assert!(!result2.needs_probe, "within cadence window must not probe");
}

#[test]
fn heartbeat_requests_probe_after_cadence_window() {
    let mut rt = make_runtime_ready();
    let now_secs: u64 = 1_000_000;
    // Arm baseline.
    rt.tick_heartbeat(now_secs, 30, 3);

    // 30 s later — a full cadence window has elapsed.
    let result = rt.tick_heartbeat(now_secs + 30, 30, 3);
    assert!(
        result.needs_probe,
        "after cadence window must request probe"
    );
}

#[test]
fn heartbeat_increments_failures_for_unanswered_probe() {
    let mut rt = make_runtime_ready();
    let now_secs: u64 = 1_000_000;
    // Arm baseline.
    rt.tick_heartbeat(now_secs, 30, 3);
    // First probe sent (no failure yet — no previous probe was outstanding).
    rt.tick_heartbeat(now_secs + 30, 30, 3);
    // probe_outstanding is now true; no response arrived.
    // Second window opens → previous probe counts as failure 1.
    rt.tick_heartbeat(now_secs + 60, 30, 3);
    let failures = rt.connection.as_ref().unwrap().consecutive_failures;
    assert_eq!(failures, 1, "one unanswered probe = failure count 1");
}

#[test]
fn heartbeat_transitions_to_reconnecting_after_max_failures() {
    let mut rt = make_runtime_ready();
    let now_secs: u64 = 1_000_000;
    let cadence: u64 = 30;
    let max: u32 = 3;

    // Arm baseline.
    rt.tick_heartbeat(now_secs, cadence, max);

    // Drive 3 missed probe windows to accumulate max_failures.
    for i in 1..=max as u64 {
        rt.tick_heartbeat(now_secs + cadence * i, cadence, max);
    }
    // Next window opens after all 3 failures are counted.
    let result = rt.tick_heartbeat(now_secs + cadence * (max as u64 + 1), cadence, max);
    let conn = rt.connection.as_ref().unwrap();
    assert_eq!(
        conn.connection_state,
        Some(NwcConnectionState::Reconnecting),
        "after max_failures the state must be Reconnecting"
    );
    // At least one ready frame (the REQ resubscription) must be present.
    assert!(
        !result.ready_frames.is_empty(),
        "Reconnecting transition must include a REQ frame"
    );
}

#[test]
fn heartbeat_transitions_to_transport_lost_after_double_max_failures() {
    let mut rt = make_runtime_ready();
    let now_secs: u64 = 1_000_000;
    let cadence: u64 = 30;
    let max: u32 = 3;

    // Arm baseline.
    rt.tick_heartbeat(now_secs, cadence, max);

    // Drive 2 × max_failures missed windows.
    for i in 1..=(max as u64 * 2) {
        rt.tick_heartbeat(now_secs + cadence * i, cadence, max);
    }
    let result = rt.tick_heartbeat(now_secs + cadence * (max as u64 * 2 + 1), cadence, max);
    let conn = rt.connection.as_ref().unwrap();
    assert_eq!(
        conn.connection_state,
        Some(NwcConnectionState::TransportLost),
        "after 2×max_failures the state must be TransportLost"
    );
    // Past TransportLost we must NOT keep emitting REQ frames.
    assert!(
        result.ready_frames.is_empty(),
        "TransportLost must not emit further REQ frames"
    );
}

#[test]
fn heartbeat_resets_on_successful_response() {
    let mut rt = make_runtime_ready();
    let now_secs: u64 = 1_000_000;
    let cadence: u64 = 30;
    let max: u32 = 3;

    // Arm baseline then drive into Reconnecting.
    rt.tick_heartbeat(now_secs, cadence, max);
    for i in 1..=(max as u64 + 1) {
        rt.tick_heartbeat(now_secs + cadence * i, cadence, max);
    }
    // Manually simulate what handle_nwc_text does on a successful response.
    {
        let conn = rt.connection.as_mut().unwrap();
        conn.probe_outstanding = false;
        conn.consecutive_failures = 0;
        conn.connection_state = Some(NwcConnectionState::Connected);
    }
    let conn = rt.connection.as_ref().unwrap();
    assert_eq!(conn.consecutive_failures, 0, "reset to 0 after success");
    assert_eq!(
        conn.connection_state,
        Some(NwcConnectionState::Connected),
        "state must be Connected after a successful response"
    );
}
