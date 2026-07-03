//! Unit tests for [`super::NwcWalletBackend`]. Split from `nwc.rs` itself
//! (AGENTS.md LOC discipline — adding the #2895 W2 `DepositQuote`/
//! `CompleteDeposit` split pushed the inline test module past the 500-LOC
//! hard cap).

use super::*;

fn backend() -> (NwcWalletBackend, WalletStatusSlot) {
    let runtime = nmp_nip47::new_wallet_runtime_handle();
    let status = nmp_nip47::new_wallet_status_slot();
    (NwcWalletBackend::new(runtime, status.clone()), status)
}

fn ctx() -> WalletBackendContext<'static> {
    WalletBackendContext {
        now_secs: 0,
        selected_backend: None,
        account_pubkey: None,
    }
}

fn status(token: &str, connection_state: Option<NwcConnectionState>) -> WalletStatus {
    WalletStatus {
        status: token.to_string(),
        relay_url: "wss://relay.example.com".to_string(),
        wallet_pubkey_hex: "a".repeat(64),
        balance_msats: Some(21_000),
        balance_sats: Some(21),
        is_ready: token == "ready",
        is_connected: token == "ready" || token == "connecting",
        connection_state,
    }
}

#[test]
fn advertises_only_pay_bolt11() {
    let (backend, _status) = backend();
    let caps = backend.capabilities();

    assert!(caps.pay_bolt11);
    assert!(!caps.create_cashu_wallet);
    assert!(!caps.publish_nutzap_info);
    assert!(!caps.send_nutzap);
    assert!(!caps.redeem_nutzap);
    assert!(!caps.deposit_cashu);
    assert!(!caps.melt_cashu);
    assert!(!caps.observe_nutzap_receipts);
}

#[test]
fn pay_bolt11_intent_emits_one_protocol_command() {
    let (backend, _status) = backend();

    let commands = backend.start_intent(
        ctx(),
        WalletIntent::PayBolt11 {
            bolt11: "lnbc100n1p0fake".to_string(),
            amount_msats: Some(1_000),
        },
        Some("corr-1".to_string()),
    );

    assert_eq!(commands.len(), 1);
    assert!(matches!(commands[0], ActorCommand::Protocol(_)));

    // `WalletPayInvoiceCommand` is type-erased behind `Box<dyn
    // ProtocolCommand>`; there is no `Any` downcast on that trait
    // (matching `nmp-nip47`'s own `WalletPayInvoiceModule` tests). Its
    // `Debug` impl is a supertrait bound, though, so this checks that
    // `bolt11`/`amount_msats`/`correlation_id` were actually threaded
    // through rather than dropped or swapped.
    let debug = format!("{:?}", commands[0]);
    assert!(debug.contains("lnbc100n1p0fake"));
    assert!(debug.contains("1000"));
    assert!(debug.contains("corr-1"));
}

#[test]
fn cashu_and_nutzap_intents_are_a_documented_no_op() {
    let (backend, _status) = backend();

    let unsupported = [
        WalletIntent::SelectBackend {
            backend_id: WalletBackendId::new(NWC_BACKEND_ID),
        },
        WalletIntent::CreateCashuWallet {
            mint: "https://mint.example.com".to_string(),
        },
        WalletIntent::RecoverCashuWallet,
        WalletIntent::PublishNutzapInfo,
        WalletIntent::SendNutzap {
            recipient_pubkey: "b".repeat(64),
            amount_sats: 21,
            target_event_id: None,
        },
        WalletIntent::RedeemNutzap {
            event_id: "c".repeat(64),
        },
        WalletIntent::DepositQuote {
            mint: "https://mint.example.com".to_string(),
            amount_sats: 21,
        },
        WalletIntent::CompleteDeposit {
            quote_id: "quote-1".to_string(),
        },
        WalletIntent::MeltCashu {
            bolt11: "lnbc100n1p0fake".to_string(),
        },
    ];

    for intent in unsupported {
        assert!(backend.start_intent(ctx(), intent, None).is_empty());
    }
}

#[test]
fn snapshot_reports_not_configured_before_any_status_is_written() {
    let (backend, _status) = backend();
    let snapshot = backend.snapshot(WalletProjectionScope::default());

    assert_eq!(
        snapshot.projection.readiness,
        WalletReadiness::NotConfigured
    );
    assert_eq!(
        snapshot
            .projection
            .active_backend_id
            .as_ref()
            .unwrap()
            .as_str(),
        NWC_BACKEND_ID
    );
    assert!(snapshot.projection.balances.is_empty());
}

#[test]
fn snapshot_tracks_connecting_ready_and_error_status_tokens() {
    let (backend, status_slot) = backend();

    *status_slot.lock().unwrap() = Some(status("connecting", None));
    assert_eq!(
        backend
            .snapshot(WalletProjectionScope::default())
            .projection
            .readiness,
        WalletReadiness::Activating
    );

    *status_slot.lock().unwrap() = Some(status("ready", None));
    assert_eq!(
        backend
            .snapshot(WalletProjectionScope::default())
            .projection
            .readiness,
        WalletReadiness::Ready
    );

    *status_slot.lock().unwrap() = Some(status("error", None));
    assert_eq!(
        backend
            .snapshot(WalletProjectionScope::default())
            .projection
            .readiness,
        WalletReadiness::Degraded
    );

    *status_slot.lock().unwrap() = Some(status("disconnected", None));
    assert_eq!(
        backend
            .snapshot(WalletProjectionScope::default())
            .projection
            .readiness,
        WalletReadiness::NotConfigured
    );
}

#[test]
fn heartbeat_connection_state_overrides_a_stale_ready_token() {
    let (backend, status_slot) = backend();

    *status_slot.lock().unwrap() = Some(status("ready", Some(NwcConnectionState::TransportLost)));
    assert_eq!(
        backend
            .snapshot(WalletProjectionScope::default())
            .projection
            .readiness,
        WalletReadiness::Degraded,
        "a live transport-lost signal must win over a stale ready token"
    );

    *status_slot.lock().unwrap() = Some(status("ready", Some(NwcConnectionState::Reconnecting)));
    assert_eq!(
        backend
            .snapshot(WalletProjectionScope::default())
            .projection
            .readiness,
        WalletReadiness::Activating
    );

    *status_slot.lock().unwrap() = Some(status("ready", Some(NwcConnectionState::Connected)));
    assert_eq!(
        backend
            .snapshot(WalletProjectionScope::default())
            .projection
            .readiness,
        WalletReadiness::Ready
    );
}

/// The heartbeat loop ticks as soon as a connection exists — before the
/// first kind:23195 response ever lands, i.e. while `status` is still
/// `"connecting"` (see `WalletConnection::status` in
/// `nmp-nip47/src/runtime/commands.rs`). A transport failure observed
/// during that window must still surface as `Degraded`/`Activating`, not
/// get stuck reporting `Activating` forever because the token-only match
/// only ever looked at `connection_state` under the `"ready"` arm.
#[test]
fn heartbeat_connection_state_overrides_a_still_connecting_token() {
    let (backend, status_slot) = backend();

    *status_slot.lock().unwrap() = Some(status(
        "connecting",
        Some(NwcConnectionState::TransportLost),
    ));
    assert_eq!(
        backend
            .snapshot(WalletProjectionScope::default())
            .projection
            .readiness,
        WalletReadiness::Degraded,
        "transport-lost must win even if the first response never arrived"
    );

    *status_slot.lock().unwrap() =
        Some(status("connecting", Some(NwcConnectionState::Reconnecting)));
    assert_eq!(
        backend
            .snapshot(WalletProjectionScope::default())
            .projection
            .readiness,
        WalletReadiness::Activating
    );
}

#[test]
fn poisoned_status_mutex_is_recovered_not_collapsed_to_not_configured() {
    let (backend, status_slot) = backend();
    *status_slot.lock().unwrap() = Some(status("ready", None));

    // Poison the mutex the same way a panicking writer would.
    let poison_slot = status_slot.clone();
    let _ = std::thread::spawn(move || {
        let _guard = poison_slot.lock().unwrap();
        panic!("simulated writer panic while holding the status lock");
    })
    .join();
    assert!(status_slot.is_poisoned());

    assert_eq!(
        backend
            .snapshot(WalletProjectionScope::default())
            .projection
            .readiness,
        WalletReadiness::Ready,
        "a poisoned lock must recover the last-written status, not report NotConfigured"
    );
}

#[test]
fn on_wallet_event_and_on_mint_result_are_no_ops() {
    let (backend, _status) = backend();

    let event = KernelEvent {
        id: "d".repeat(64),
        author: "e".repeat(64),
        kind: 23_195,
        created_at: 0,
        tags: Vec::new(),
        content: "encrypted-payload".to_string(),
        relay_provenance: Vec::new(),
    };
    assert!(backend.on_wallet_event(ctx(), &event).is_empty());

    let result = MintResult {
        operation_id: "op-1".to_string(),
        status: crate::backend::MintResultStatus::Settled,
    };
    assert!(backend.on_mint_result(ctx(), result).is_empty());
}
