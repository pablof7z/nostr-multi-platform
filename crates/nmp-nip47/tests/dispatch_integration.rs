//! Registry-level trip tests for the three wallet typed FlatBuffers payload
//! doorways (ADR-0064 / #1756): `nmp.wallet.connect`, `nmp.wallet.disconnect`,
//! and `nmp.wallet.pay_invoice`.
//!
//! These tests prove the fail-closed `schema_version` gate in
//! `ActionRegistry::start_bytes` rejects bad payloads BEFORE `start()` runs, and
//! that a well-formed payload round-trips through the registry boundary — the
//! same path the byte transport (S2 `DispatchEnvelope`) drives in production.
//! They are load-bearing for the Cut-B producer-typing gap: before this change
//! the three modules were reachable only through the JSON doorway.
//!
//! Codec round-trip tests (positive + per-field negative, incl. the
//! `amount_msats` presence flag) live in `src/wire/action_payload_tests.rs`.
//! These tests sit one level up, at the registry boundary.

use nmp_core::__ffi_internal::ActionRegistry;
use nmp_core::substrate::{ActionContext, ActionPayload, ActionRegistrar, ActionRejection};
use nmp_nip47::{
    new_wallet_runtime_handle, WalletAction, WalletConnectAction, WalletConnectModule,
    WalletDisconnectAction, WalletDisconnectModule, WalletPayInvoiceModule,
};

const CONNECT_NAMESPACE: &str = "nmp.wallet.connect";
const DISCONNECT_NAMESPACE: &str = "nmp.wallet.disconnect";
const PAY_INVOICE_NAMESPACE: &str = "nmp.wallet.pay_invoice";

const NOW_MS: u64 = 1_700_000_000_000;

/// Register all three wallet action modules into a fresh registry, each bound to
/// an independent (empty) per-app `WalletRuntimeHandle` — the same shape
/// `register_wallet` installs, minus the interceptor/projection wiring those
/// `start_bytes` paths never touch.
fn registry_with_wallet_actions() -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    let _ = registry.register_action(WalletConnectModule::new(new_wallet_runtime_handle()));
    let _ = registry.register_action(WalletDisconnectModule::new(new_wallet_runtime_handle()));
    let _ = registry.register_action(WalletPayInvoiceModule::new(new_wallet_runtime_handle()));
    registry
}

fn assert_version_mismatch(err: ActionRejection) {
    match err {
        ActionRejection::Invalid(msg) => assert!(
            msg.contains("schema_version mismatch"),
            "rejection must name the version trip: {msg}"
        ),
        other => panic!("expected Invalid rejection, got {other:?}"),
    }
}

// --- nmp.wallet.connect ------------------------------------------------------

#[test]
fn start_bytes_rejects_wrong_schema_version_for_connect() {
    let registry = registry_with_wallet_actions();
    let bad = build_bad_version_connect_payload();
    let err = registry
        .start_bytes(&mut ActionContext::default(), NOW_MS, CONNECT_NAMESPACE, &bad)
        .expect_err("a wrong schema_version must be rejected before start() (fail closed)");
    assert_version_mismatch(err);
}

#[test]
fn start_bytes_accepts_well_formed_connect() {
    let registry = registry_with_wallet_actions();
    let action = WalletConnectAction::Connect {
        uri: "nostr+walletconnect://abc123?relay=wss://relay.example&secret=xyz".to_string(),
    };
    let bytes = action.encode();
    registry
        .start_bytes(
            &mut ActionContext::default(),
            NOW_MS,
            CONNECT_NAMESPACE,
            &bytes,
        )
        .expect("a well-formed, correct-version connect payload must be accepted");
}

// --- nmp.wallet.disconnect ---------------------------------------------------

#[test]
fn start_bytes_rejects_wrong_schema_version_for_disconnect() {
    let registry = registry_with_wallet_actions();
    let bad = build_bad_version_disconnect_payload();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            NOW_MS,
            DISCONNECT_NAMESPACE,
            &bad,
        )
        .expect_err("a wrong schema_version must be rejected before start() (fail closed)");
    assert_version_mismatch(err);
}

#[test]
fn start_bytes_accepts_well_formed_disconnect() {
    let registry = registry_with_wallet_actions();
    let bytes = WalletDisconnectAction::Disconnect.encode();
    registry
        .start_bytes(
            &mut ActionContext::default(),
            NOW_MS,
            DISCONNECT_NAMESPACE,
            &bytes,
        )
        .expect("a well-formed disconnect payload must be accepted");
}

// --- nmp.wallet.pay_invoice --------------------------------------------------

#[test]
fn start_bytes_rejects_wrong_schema_version_for_pay_invoice() {
    let registry = registry_with_wallet_actions();
    let bad = build_bad_version_pay_invoice_payload();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            NOW_MS,
            PAY_INVOICE_NAMESPACE,
            &bad,
        )
        .expect_err("a wrong schema_version must be rejected before start() (fail closed)");
    assert_version_mismatch(err);
}

#[test]
fn start_bytes_accepts_well_formed_pay_invoice() {
    let registry = registry_with_wallet_actions();
    for amount_msats in [None, Some(0u64), Some(21_000u64)] {
        let action = WalletAction::PayInvoice {
            bolt11: format!("lnbc100n1p0trip{amount_msats:?}"),
            amount_msats,
        };
        let bytes = action.encode();
        registry
            .start_bytes(
                &mut ActionContext::default(),
                NOW_MS,
                PAY_INVOICE_NAMESPACE,
                &bytes,
            )
            .expect("a well-formed, correct-version pay_invoice payload must be accepted");
    }
}

// --- unregistered namespace --------------------------------------------------

#[test]
fn unregistered_namespace_is_rejected() {
    let registry = registry_with_wallet_actions();
    let bytes = WalletDisconnectAction::Disconnect.encode();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            NOW_MS,
            "nmp.wallet.not_a_real_namespace",
            &bytes,
        )
        .expect_err("an unregistered namespace must be rejected");
    assert!(
        matches!(err, ActionRejection::Invalid(_)),
        "expected Invalid rejection for unregistered namespace, got {err:?}"
    );
}

/// A finished `WalletConnectPayload` (file identifier `N47C`) carrying
/// `schema_version = 999`. The fail-closed gate must reject it before `start()`.
fn build_bad_version_connect_payload() -> Vec<u8> {
    use flatbuffers::FlatBufferBuilder;

    // WalletConnectPayload vtable slots: VT_SCHEMA_VERSION = 4, VT_URI = 6.
    const N47C_IDENTIFIER: &str = "N47C";
    const VT_SCHEMA_VERSION: flatbuffers::VOffsetT = 4;
    const VT_URI: flatbuffers::VOffsetT = 6;

    let mut fbb = FlatBufferBuilder::new();
    let uri = fbb.create_string("nostr+walletconnect://abc");
    let payload_start = fbb.start_table();
    fbb.push_slot::<u32>(VT_SCHEMA_VERSION, 999, 0);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_URI, uri);
    let root = fbb.end_table(payload_start);
    fbb.finish(root, Some(N47C_IDENTIFIER));
    fbb.finished_data().to_vec()
}

/// A finished `WalletDisconnectPayload` (file identifier `N47D`) carrying
/// `schema_version = 999`. The fail-closed gate must reject it before `start()`.
fn build_bad_version_disconnect_payload() -> Vec<u8> {
    use flatbuffers::FlatBufferBuilder;

    // WalletDisconnectPayload vtable slots: VT_SCHEMA_VERSION = 4 (only field).
    const N47D_IDENTIFIER: &str = "N47D";
    const VT_SCHEMA_VERSION: flatbuffers::VOffsetT = 4;

    let mut fbb = FlatBufferBuilder::new();
    let payload_start = fbb.start_table();
    fbb.push_slot::<u32>(VT_SCHEMA_VERSION, 999, 0);
    let root = fbb.end_table(payload_start);
    fbb.finish(root, Some(N47D_IDENTIFIER));
    fbb.finished_data().to_vec()
}

/// A finished `WalletPayInvoicePayload` (file identifier `N47P`) carrying
/// `schema_version = 999`. The fail-closed gate must reject it before `start()`.
/// Hand-built so this integration test does not need the private `wire` module.
fn build_bad_version_pay_invoice_payload() -> Vec<u8> {
    use flatbuffers::FlatBufferBuilder;

    // WalletPayInvoicePayload vtable slots:
    //   VT_SCHEMA_VERSION = 4, VT_BOLT11 = 6, VT_AMOUNT_MSATS = 8,
    //   VT_HAS_AMOUNT_MSATS = 10.
    const N47P_IDENTIFIER: &str = "N47P";
    const VT_SCHEMA_VERSION: flatbuffers::VOffsetT = 4;
    const VT_BOLT11: flatbuffers::VOffsetT = 6;

    let mut fbb = FlatBufferBuilder::new();
    let bolt11 = fbb.create_string("lnbc100n1p0badversion");

    let payload_start = fbb.start_table();
    fbb.push_slot::<u32>(VT_SCHEMA_VERSION, 999, 0);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_BOLT11, bolt11);
    let root = fbb.end_table(payload_start);
    fbb.finish(root, Some(N47P_IDENTIFIER));
    fbb.finished_data().to_vec()
}
