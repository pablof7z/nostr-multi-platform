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

// ---- M14-1 / #2145: generated-builder wire round-trip (UlongWithPresenceFlag)
//
// The positive tests above feed the Rust typed `.encode()` through `start_bytes`.
// They do NOT prove that bytes shaped EXACTLY as the generated Swift/Kotlin
// `walletPayInvoice` builder emits (`crates/nmp-codegen/src/action_builders`)
// decode back correctly — and that emitter introduces a wire shape no other
// namespace uses: an `Option<u64>` encoded as TWO slots (the `amount_msats`
// scalar at slot 2 / vt 8 PLUS a `has_amount_msats` presence flag at slot 3 / vt
// 10), with BOTH slots OMITTED when the amount is absent. The subtle, load-
// bearing case is `Some(0)`: the scalar equals its FlatBuffers default and is
// elided on the wire, so ONLY the presence flag distinguishes it from `None`.
// This round-trip proves the emitter's two-slot layout preserves the
// None / Some(0) / Some(value) distinction the byte doorway relies on — the
// guard the codegen emitter unit tests cannot provide (no nmp-core dep there).

/// Build a `nmp.wallet.pay_invoice` `DispatchEnvelope` EXACTLY as the generated
/// `walletPayInvoice(correlationId:bolt11:amountMsats:)` builder does: the
/// `WalletPayInvoicePayload` (N47P; schema_version slot 0, bolt11 slot 1), and —
/// ONLY when `amount_msats` is `Some` — the `amount_msats` scalar (slot 2) plus
/// the `has_amount_msats` flag (slot 3). Then stamp it into the `NMPD` envelope.
fn build_pay_invoice_envelope(
    correlation_id: &str,
    bolt11: &str,
    amount_msats: Option<u64>,
) -> Vec<u8> {
    use flatbuffers::{FlatBufferBuilder, VOffsetT, WIPOffset};
    use nmp_core::dispatch_envelope::encode_dispatch_envelope;

    const N47P_IDENTIFIER: &str = "N47P";
    let payload = {
        let mut fbb = FlatBufferBuilder::new();
        let bolt11_off = fbb.create_string(bolt11);
        let start = fbb.start_table();
        fbb.push_slot::<u32>(4 as VOffsetT, 1, 0); // slot 0: schema_version = 1
        fbb.push_slot_always::<WIPOffset<&str>>(6 as VOffsetT, bolt11_off); // slot 1: bolt11
        if let Some(amount) = amount_msats {
            // Mirror the emitter: write both the scalar (def 0 — Some(0) elides
            // it) and the presence flag, ONLY inside the `Some` arm.
            fbb.push_slot::<u64>(8 as VOffsetT, amount, 0); // slot 2: amount_msats
            fbb.push_slot::<bool>(10 as VOffsetT, true, false); // slot 3: has_amount_msats
        }
        let root = fbb.end_table(start);
        fbb.finish(root, Some(N47P_IDENTIFIER));
        fbb.finished_data().to_vec()
    };
    encode_dispatch_envelope(correlation_id, PAY_INVOICE_NAMESPACE, 1, &payload)
}

/// `walletPayInvoice` builder bytes decode field-for-field to the expected
/// `WalletAction::PayInvoice` for `None`, `Some(0)`, and `Some(value)` — proving
/// the emitter's two-slot presence-flag layout preserves the distinction (in
/// particular `Some(0)` must NOT collapse to `None`, and `None` must NOT read as
/// `Some(0)`) — and dispatch through `start_bytes`. A wrong-namespace twin
/// proves the route is real.
#[test]
fn pay_invoice_builder_bytes_presence_flag_round_trip() {
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;

    let registry = registry_with_wallet_actions();

    for amount_msats in [None, Some(0u64), Some(21_000u64)] {
        let bolt11 = format!("lnbc100n1p0builder{amount_msats:?}");
        let bytes = build_pay_invoice_envelope("corr-pay", &bolt11, amount_msats);

        let decoded = decode_dispatch_envelope(&bytes).expect("builder envelope must decode (S2)");
        assert_eq!(decoded.action_namespace, PAY_INVOICE_NAMESPACE);

        let action = WalletAction::decode(&decoded.payload)
            .expect("the opaque payload must decode via WalletAction");
        match action {
            WalletAction::PayInvoice {
                bolt11: decoded_bolt11,
                amount_msats: decoded_amount,
            } => {
                assert_eq!(decoded_bolt11, bolt11, "bolt11 must round-trip");
                assert_eq!(
                    decoded_amount, amount_msats,
                    "the two-slot presence flag must preserve {amount_msats:?} exactly \
                     (Some(0) must stay Some(0); None must stay None)"
                );
            }
        }

        // POSITIVE: routed to the right namespace, payload decodes + start() OK.
        registry
            .start_bytes(
                &mut ActionContext::default(),
                NOW_MS,
                PAY_INVOICE_NAMESPACE,
                &decoded.payload,
            )
            .expect("walletPayInvoice builder bytes must dispatch + validate via start_bytes");

        // LOAD-BEARING twin: the SAME N47P bytes routed as connect mis-decode
        // (N47C identifier missing) and fail closed.
        let err = registry
            .start_bytes(
                &mut ActionContext::default(),
                NOW_MS,
                CONNECT_NAMESPACE,
                &decoded.payload,
            )
            .expect_err("an N47P payload routed as connect must be rejected");
        assert!(
            matches!(err, ActionRejection::Invalid(_)),
            "wrong-namespace dispatch must fail closed as Invalid, got {err:?}"
        );
    }
}

/// Build a `nmp.wallet.connect` `DispatchEnvelope` as the generated
/// `walletConnect(correlationId:uri:)` builder does (N47C; schema_version slot 0,
/// uri slot 1) and a `nmp.wallet.disconnect` envelope as `walletDisconnect`
/// does (N47D; schema_version-only).
fn build_connect_envelope(correlation_id: &str, uri: &str) -> Vec<u8> {
    use flatbuffers::{FlatBufferBuilder, VOffsetT, WIPOffset};
    use nmp_core::dispatch_envelope::encode_dispatch_envelope;

    let payload = {
        let mut fbb = FlatBufferBuilder::new();
        let uri_off = fbb.create_string(uri);
        let start = fbb.start_table();
        fbb.push_slot::<u32>(4 as VOffsetT, 1, 0); // slot 0: schema_version = 1
        fbb.push_slot_always::<WIPOffset<&str>>(6 as VOffsetT, uri_off); // slot 1: uri
        let root = fbb.end_table(start);
        fbb.finish(root, Some("N47C"));
        fbb.finished_data().to_vec()
    };
    encode_dispatch_envelope(correlation_id, CONNECT_NAMESPACE, 1, &payload)
}

fn build_disconnect_envelope(correlation_id: &str) -> Vec<u8> {
    use flatbuffers::{FlatBufferBuilder, VOffsetT};
    use nmp_core::dispatch_envelope::encode_dispatch_envelope;

    let payload = {
        let mut fbb = FlatBufferBuilder::new();
        let start = fbb.start_table();
        fbb.push_slot::<u32>(4 as VOffsetT, 1, 0); // slot 0: schema_version = 1
        let root = fbb.end_table(start);
        fbb.finish(root, Some("N47D"));
        fbb.finished_data().to_vec()
    };
    encode_dispatch_envelope(correlation_id, DISCONNECT_NAMESPACE, 1, &payload)
}

/// `walletConnect` builder bytes decode field-for-field to
/// `WalletConnectAction::Connect` and dispatch through `start_bytes`. The
/// wrong-namespace twin (routed as disconnect) proves the route is real.
#[test]
fn connect_builder_bytes_round_trip() {
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;

    let registry = registry_with_wallet_actions();
    let uri = "nostr+walletconnect://abc123?relay=wss://relay.example&secret=xyz";
    let bytes = build_connect_envelope("corr-conn", uri);

    let decoded = decode_dispatch_envelope(&bytes).expect("builder envelope must decode (S2)");
    assert_eq!(decoded.action_namespace, CONNECT_NAMESPACE);
    assert_eq!(
        WalletConnectAction::decode(&decoded.payload)
            .expect("payload must decode via WalletConnectAction"),
        WalletConnectAction::Connect { uri: uri.to_string() },
        "walletConnect builder bytes must decode field-for-field"
    );
    registry
        .start_bytes(
            &mut ActionContext::default(),
            NOW_MS,
            CONNECT_NAMESPACE,
            &decoded.payload,
        )
        .expect("walletConnect builder bytes must dispatch + validate via start_bytes");
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            NOW_MS,
            DISCONNECT_NAMESPACE,
            &decoded.payload,
        )
        .expect_err("an N47C payload routed as disconnect must be rejected");
    assert!(
        matches!(err, ActionRejection::Invalid(_)),
        "wrong-namespace dispatch must fail closed as Invalid, got {err:?}"
    );
}

/// `walletDisconnect` builder bytes (schema_version-only payload) decode to
/// `WalletDisconnectAction::Disconnect` and dispatch through `start_bytes`. The
/// wrong-namespace twin (routed as pay_invoice) proves the route is real.
#[test]
fn disconnect_builder_bytes_round_trip() {
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;

    let registry = registry_with_wallet_actions();
    let bytes = build_disconnect_envelope("corr-disc");

    let decoded = decode_dispatch_envelope(&bytes).expect("builder envelope must decode (S2)");
    assert_eq!(decoded.action_namespace, DISCONNECT_NAMESPACE);
    assert_eq!(
        WalletDisconnectAction::decode(&decoded.payload)
            .expect("payload must decode via WalletDisconnectAction"),
        WalletDisconnectAction::Disconnect,
        "walletDisconnect builder bytes must decode to Disconnect"
    );
    registry
        .start_bytes(
            &mut ActionContext::default(),
            NOW_MS,
            DISCONNECT_NAMESPACE,
            &decoded.payload,
        )
        .expect("walletDisconnect builder bytes must dispatch + validate via start_bytes");
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            NOW_MS,
            PAY_INVOICE_NAMESPACE,
            &decoded.payload,
        )
        .expect_err("an N47D payload routed as pay_invoice must be rejected");
    assert!(
        matches!(err, ActionRejection::Invalid(_)),
        "wrong-namespace dispatch must fail closed as Invalid, got {err:?}"
    );
}
