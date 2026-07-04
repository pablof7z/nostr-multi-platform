//! #2916 — identity-change reset for the NWC `WalletRuntime`.
//!
//! An NWC Lightning connection is Nostr-account-scoped (the owner's settled
//! decision, mirroring Cashu): a Nostr account switch must drop the previous
//! account's [`WalletConnection`] and clear the shared status slot back to
//! "no wallet connected", so the merged `"wallet"` projection does not carry a
//! previous account's connection/status into the newly active one.
//!
//! Kept as a `#[path]` sibling of `runtime.rs`/`runtime_tests.rs` so both test
//! modules stay under the file-size gate's 500-LOC source ceiling.

use super::*;

use crate::payment_store::{FsPaymentStore, PaymentRecord, PaymentState};
use crate::status::{new_wallet_status_slot, WalletStatus};

/// Build a live NWC connection for a fictional "account A" wallet.
fn account_a_connection() -> WalletConnection {
    WalletConnection {
        wallet_pubkey_hex: "aaaa".repeat(16),
        relay_url: "wss://test.relay".to_string(),
        client_secret_hex: Zeroizing::new("bb".repeat(32)),
        client_pubkey_hex: "cccc".repeat(16),
        status: "ready".to_string(),
        balance_msats: Some(21_000),
        pending: HashMap::new(),
        pending_payments: HashMap::new(),
        pending_lookups: HashMap::new(),
        sub_id: "nwc-aaaa".to_string(),
        orphan_responses: 0,
        last_probe_sent_secs: 0,
        probe_outstanding: false,
        consecutive_failures: 0,
        connection_state: Some(NwcConnectionState::Connected),
    }
}

#[test]
fn reset_clears_connection_and_status_slot() {
    let slot = new_wallet_status_slot();
    let mut rt = WalletRuntime::new(slot.clone());

    // Account A: a live NWC connection plus the projected "ready" status the
    // shells would render.
    rt.connection = Some(account_a_connection());
    *slot.lock().unwrap() = Some(WalletStatus {
        status: "ready".to_string(),
        relay_url: "wss://test.relay".to_string(),
        wallet_pubkey_hex: "aaaa".repeat(16),
        balance_msats: Some(21_000),
        balance_sats: Some(21),
        is_ready: true,
        is_connected: true,
        connection_state: Some(NwcConnectionState::Connected),
    });
    assert!(rt.is_nwc_relay("wss://test.relay"), "account A is connected");

    // Nostr account switch → reset.
    rt.reset();

    assert!(
        rt.connection.is_none(),
        "reset must drop the previous account's WalletConnection"
    );
    assert!(
        !rt.is_nwc_relay("wss://test.relay"),
        "no relay is the NWC relay after reset"
    );
    assert!(
        slot.lock().unwrap().is_none(),
        "reset must clear the shared status slot back to no-wallet-connected"
    );
}

#[test]
fn reset_keeps_the_durable_payment_store_handle() {
    // Mirrors Cashu keeping its WAL store across reset: the durable per-payment
    // records are the source of truth for lookup_invoice reconciliation and
    // must survive an account switch, even though the in-memory connection does
    // not.
    let dir = tempfile::tempdir().unwrap();
    let slot = new_wallet_status_slot();
    let mut rt = WalletRuntime::new(slot);
    rt.set_payment_store(FsPaymentStore::new(dir.path()));
    rt.payment_store
        .as_ref()
        .unwrap()
        .upsert(&PaymentRecord {
            request_event_id: "pay-keep".to_string(),
            bolt11: "lnbc1test".to_string(),
            correlation_id: Some("cid-keep".to_string()),
            amount_msats: None,
            state: PaymentState::Unknown,
            preimage: None,
        })
        .unwrap();
    rt.connection = Some(account_a_connection());

    rt.reset();

    let unresolved = rt
        .payment_store
        .as_ref()
        .expect("payment store handle survives reset")
        .load_unresolved()
        .unwrap();
    assert_eq!(
        unresolved.len(),
        1,
        "durable Unknown record must survive the account-switch reset"
    );
}
