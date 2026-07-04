//! Registry-level trip tests for the ten `nmp.wallet.*` typed FlatBuffers
//! payload doorways this crate owns (#2920, epic #2864; `set_mints` added by
//! #2997, `cross_mint_transfer` by #3003): `select_backend`, the Cashu
//! `create`/`recover`/`set_mints`/`cross_mint_transfer`/`deposit_quote`/
//! `complete_deposit` family (split into `dispatch_integration_cashu.rs`,
//! AGENTS.md LOC discipline), and the nutzap `publish_info`/`send`/`redeem`
//! family (this file).
//!
//! Before this test existed, none of these eight `ActionModule`s implemented
//! `ActionPayload`/overrode `decode_payload`, so `ActionRegistry::start_bytes`/
//! `execute_bytes` — the SAME seam `nmp_native_runtime::dispatch_action_bytes_typed`
//! calls into, the sole production dispatch path a Rust, UniFFI, or wasm host
//! reaches — rejected every one of them as "not typed-capable". These tests
//! prove each namespace is now reachable BY NAME through that byte doorway: a
//! well-formed payload decodes, validates, and executes; a malformed payload
//! fails closed before `start()` ever runs.
//!
//! Codec round-trip tests (positive + per-field negative) live in
//! `src/wire/action_payload_tests.rs`. These tests sit one level up, at the
//! registry boundary — the same level `nmp-nip47`'s own
//! `tests/dispatch_integration.rs` exercises for its three wallet actions.
//!
//! The two builder-shaped-bytes tests at the bottom of this file
//! (`nutzap_send_builder_bytes_...` / `cashu_deposit_quote_builder_bytes_...`)
//! additionally hand-build bytes EXACTLY as the generated
//! `walletNutzapSend`/`walletCashuDepositQuote` Swift/Kotlin/TS builders would
//! (`crates/nmp-codegen/src/action_builders/registry/wallet.rs`'s field order),
//! rather than going through this crate's own `.encode()` — mirroring
//! `nmp-nip47`'s `pay_invoice_builder_bytes_...`/`connect_builder_bytes_...`
//! tests. This is the guard that would catch a FUTURE field-order drift
//! between the generated-builder registry and the `.fbs` schema (a mismatch
//! there would still compile and pass every OTHER test in this crate, since
//! they all round-trip through this crate's own matched encode/decode pair).

use std::cell::RefCell;
use std::sync::Arc;

use nmp_core::__ffi_internal::ActionRegistry;
use nmp_core::actor::ActorCommand;
use nmp_core::slots::new_active_account_slot;
use nmp_core::substrate::{ActionContext, ActionPayload, ActionRegistrar, ActionRejection};

use nmp_wallet::{
    CashuCompleteDepositAction, CashuCreateAction, CashuCrossMintTransferAction,
    CashuDepositQuoteAction, CashuRecoverAction, CashuSetMintsAction, CashuWalletBackend,
    NutzapPublishInfoAction, NutzapRedeemAction, NutzapSendAction, SelectBackendAction,
    WalletBackendSelector, CASHU_BACKEND_ID,
};

const SELECT_BACKEND: &str = nmp_wallet::ACTION_SELECT_BACKEND;
const CASHU_CREATE: &str = nmp_wallet::ACTION_CASHU_CREATE;
const CASHU_RECOVER: &str = nmp_wallet::ACTION_CASHU_RECOVER;
const CASHU_SET_MINTS: &str = nmp_wallet::ACTION_CASHU_SET_MINTS;
const CASHU_CROSS_MINT_TRANSFER: &str = nmp_wallet::ACTION_CASHU_CROSS_MINT_TRANSFER;
const CASHU_DEPOSIT_QUOTE: &str = nmp_wallet::ACTION_CASHU_DEPOSIT_QUOTE;
const CASHU_COMPLETE_DEPOSIT: &str = nmp_wallet::ACTION_CASHU_COMPLETE_DEPOSIT;
const NUTZAP_PUBLISH_INFO: &str = nmp_wallet::ACTION_NUTZAP_PUBLISH_INFO;
const NUTZAP_SEND: &str = nmp_wallet::ACTION_NUTZAP_SEND;
const NUTZAP_REDEEM: &str = nmp_wallet::ACTION_NUTZAP_REDEEM;

const NOW_MS: u64 = 1_700_000_000_000;

/// Register all eight wallet action modules into a fresh registry, mirroring
/// the composition `nmp_wallet::register` performs: a `WalletBackendSelector`
/// carrying a real `CashuWalletBackend` (so `require_capable_backend` accepts
/// the six capability-gated actions the same way production composition
/// does), plus a fresh active-account slot.
fn registry_with_wallet_actions() -> ActionRegistry {
    let selector: Arc<WalletBackendSelector> =
        Arc::new(WalletBackendSelector::new(vec![Arc::new(
            CashuWalletBackend::new(),
        )]));
    let active_pubkey = new_active_account_slot();

    let mut registry = ActionRegistry::new();
    let _ = registry.register_action(nmp_wallet::action::SelectBackendModule::new(Arc::clone(
        &selector,
    )));
    let _ = registry.register_action(nmp_wallet::action::CashuCreateModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ));
    let _ = registry.register_action(nmp_wallet::action::CashuRecoverModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ));
    let _ = registry.register_action(nmp_wallet::action::CashuSetMintsModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ));
    let _ = registry.register_action(nmp_wallet::action::CashuCrossMintTransferModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ));
    let _ = registry.register_action(nmp_wallet::action::CashuDepositQuoteModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ));
    let _ = registry.register_action(nmp_wallet::action::CashuCompleteDepositModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ));
    let _ = registry.register_action(nmp_wallet::action::NutzapPublishInfoModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ));
    let _ = registry.register_action(nmp_wallet::action::NutzapSendModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ));
    let _ = registry.register_action(nmp_wallet::action::NutzapRedeemModule::new(
        Arc::clone(&selector),
        active_pubkey,
    ));
    registry
}

/// Run `start_bytes` then `execute_bytes` for `namespace`/`bytes`, collecting
/// every `ActorCommand` the module enqueues, and assert both steps succeed —
/// the full happy path `dispatch_action_bytes_typed` drives in production.
fn dispatch_ok(registry: &ActionRegistry, namespace: &str, bytes: &[u8]) -> Vec<ActorCommand> {
    let correlation_id = registry
        .start_bytes(&mut ActionContext::default(), NOW_MS, namespace, bytes)
        .unwrap_or_else(|err| {
            panic!("{namespace} start_bytes must accept a well-formed payload: {err:?}")
        });
    let sent = RefCell::new(Vec::new());
    registry
        .execute_bytes(
            &ActionContext::default(),
            namespace,
            bytes,
            &correlation_id,
            &|cmd| sent.borrow_mut().push(cmd),
        )
        .unwrap_or_else(|err| panic!("{namespace} execute_bytes must succeed: {err:?}"));
    sent.into_inner()
}

// The `cashu.*` family's own dispatch tests (AGENTS.md LOC discipline) —
// shares `registry_with_wallet_actions`/`dispatch_ok`/the namespace consts
// above via `use super::*;`. Lives in the `dispatch_integration/` SUBDIRECTORY
// (not a sibling `tests/*.rs` file) so Cargo does not also auto-discover it
// as its own independent, helper-less integration test binary.
#[path = "dispatch_integration/cashu.rs"]
mod dispatch_integration_cashu;

// --- select_backend -----------------------------------------------------------

#[test]
fn select_backend_dispatches_by_name() {
    let registry = registry_with_wallet_actions();
    let bytes = SelectBackendAction {
        backend_id: CASHU_BACKEND_ID.to_string(),
    }
    .encode();
    // select_backend never enqueues an ActorCommand (it flips selector state
    // synchronously in execute()) — the assertion is simply that both steps
    // succeed.
    let _ = dispatch_ok(&registry, SELECT_BACKEND, &bytes);
}

// --- nutzap.publish_info -----------------------------------------------------------

#[test]
fn nutzap_publish_info_dispatches_by_name() {
    let registry = registry_with_wallet_actions();
    let bytes = NutzapPublishInfoAction {}.encode();
    let sent = dispatch_ok(&registry, NUTZAP_PUBLISH_INFO, &bytes);
    assert!(
        !sent.is_empty(),
        "nutzap.publish_info must enqueue at least one ActorCommand"
    );
}

// --- nutzap.send --------------------------------------------------------------------

#[test]
fn nutzap_send_dispatches_by_name() {
    let registry = registry_with_wallet_actions();
    let bytes = NutzapSendAction {
        recipient_pubkey: "a".repeat(64),
        amount_sats: 100,
        target_event_id: None,
    }
    .encode();
    let sent = dispatch_ok(&registry, NUTZAP_SEND, &bytes);
    assert!(
        !sent.is_empty(),
        "nutzap.send must enqueue at least one ActorCommand"
    );
}

// --- nutzap.redeem ------------------------------------------------------------------

#[test]
fn nutzap_redeem_dispatches_by_name() {
    let registry = registry_with_wallet_actions();
    let bytes = NutzapRedeemAction {
        event_id: "b".repeat(64),
    }
    .encode();
    let sent = dispatch_ok(&registry, NUTZAP_REDEEM, &bytes);
    assert!(
        !sent.is_empty(),
        "nutzap.redeem must enqueue at least one ActorCommand"
    );
}

// --- fail-closed: malformed payload, every namespace ---------------------------------

#[test]
fn malformed_payload_is_rejected_before_start_for_every_namespace() {
    let registry = registry_with_wallet_actions();
    for namespace in [
        SELECT_BACKEND,
        CASHU_CREATE,
        CASHU_RECOVER,
        CASHU_SET_MINTS,
        CASHU_DEPOSIT_QUOTE,
        CASHU_COMPLETE_DEPOSIT,
        NUTZAP_PUBLISH_INFO,
        NUTZAP_SEND,
        NUTZAP_REDEEM,
    ] {
        let err = registry
            .start_bytes(
                &mut ActionContext::default(),
                NOW_MS,
                namespace,
                b"not a flatbuffer",
            )
            .expect_err(&format!("{namespace} must reject a malformed payload"));
        assert!(
            matches!(err, ActionRejection::Invalid(_)),
            "{namespace}: malformed payload must fail closed as Invalid, got {err:?}"
        );
    }
}

// --- unregistered namespace ------------------------------------------------------------

#[test]
fn unregistered_namespace_is_rejected() {
    let registry = registry_with_wallet_actions();
    let bytes = NutzapRedeemAction {
        event_id: "c".repeat(64),
    }
    .encode();
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

// ---- generated-builder-shaped wire round-trip (M14-1 / #2145 pattern) --------------
//
// The positive tests above feed the Rust typed `.encode()` through `start_bytes`.
// They do NOT prove that bytes shaped EXACTLY as the generated Swift/Kotlin/TS
// `walletNutzapSend`/`walletCashuDepositQuote` builders emit
// (`crates/nmp-codegen/src/action_builders/registry/wallet.rs`) decode back
// correctly — a field-order drift between that registry and the `.fbs` schema
// would still pass every OTHER test in this crate (they all round-trip through
// this crate's own matched encode/decode pair). These two hand-build the wire
// bytes at the vtable-slot level, mirroring `nmp-nip47`'s own
// `pay_invoice_builder_bytes_...`/`connect_builder_bytes_...` tests.

/// Build a `nmp.wallet.nutzap.send` `DispatchEnvelope` EXACTLY as the generated
/// `walletNutzapSend(correlationId:recipientPubkey:amountSats:targetEventId:)`
/// builder does: the `NutzapSendPayload` (NWNS; schema_version slot 0,
/// recipient_pubkey slot 1, amount_sats slot 2), and — ONLY when
/// `target_event_id` is `Some` — the optional string at slot 3 (omitted
/// entirely, not written as offset 0, when absent).
fn build_nutzap_send_envelope(
    correlation_id: &str,
    recipient_pubkey: &str,
    amount_sats: u64,
    target_event_id: Option<&str>,
) -> Vec<u8> {
    use flatbuffers::{FlatBufferBuilder, VOffsetT, WIPOffset};
    use nmp_core::dispatch_envelope::encode_dispatch_envelope;

    const NWNS_IDENTIFIER: &str = "NWNS";
    let payload = {
        let mut fbb = FlatBufferBuilder::new();
        let recipient_off = fbb.create_string(recipient_pubkey);
        let target_off = target_event_id.map(|s| fbb.create_string(s));
        let start = fbb.start_table();
        fbb.push_slot::<u32>(4 as VOffsetT, 1, 0); // slot 0: schema_version = 1
        fbb.push_slot_always::<WIPOffset<&str>>(6 as VOffsetT, recipient_off); // slot 1: recipient_pubkey
        fbb.push_slot::<u64>(8 as VOffsetT, amount_sats, 0); // slot 2: amount_sats
        if let Some(target_off) = target_off {
            fbb.push_slot_always::<WIPOffset<&str>>(10 as VOffsetT, target_off);
            // slot 3: target_event_id
        }
        let root = fbb.end_table(start);
        fbb.finish(root, Some(NWNS_IDENTIFIER));
        fbb.finished_data().to_vec()
    };
    encode_dispatch_envelope(correlation_id, NUTZAP_SEND, 1, &payload)
}

/// `walletNutzapSend` builder bytes decode field-for-field to the expected
/// `NutzapSendAction` for both an absent and a present `target_event_id`, and
/// dispatch through `start_bytes`/`execute_bytes`.
#[test]
fn nutzap_send_builder_bytes_round_trip() {
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;

    let registry = registry_with_wallet_actions();
    let recipient = "a".repeat(64);

    for target_event_id in [None, Some("b".repeat(64))] {
        let bytes = build_nutzap_send_envelope(
            "corr-nutzap-send",
            &recipient,
            100,
            target_event_id.as_deref(),
        );
        let decoded = decode_dispatch_envelope(&bytes).expect("builder envelope must decode (S2)");
        assert_eq!(decoded.action_namespace, NUTZAP_SEND);
        assert_eq!(
            NutzapSendAction::decode(&decoded.payload).expect("payload must decode"),
            NutzapSendAction {
                recipient_pubkey: recipient.clone(),
                amount_sats: 100,
                target_event_id: target_event_id.clone(),
            },
            "walletNutzapSend builder bytes must decode field-for-field \
             (target_event_id: {target_event_id:?})"
        );
        let _ = dispatch_ok(&registry, NUTZAP_SEND, &decoded.payload);
    }
}

/// Build a `nmp.wallet.cashu.deposit_quote` `DispatchEnvelope` EXACTLY as the
/// generated `walletCashuDepositQuote(correlationId:mint:amountSats:)` builder
/// does: the `CashuDepositQuotePayload` (NWDQ; schema_version slot 0, mint
/// slot 1, amount_sats slot 2) — both `mint` and `amount_sats` are required.
fn build_cashu_deposit_quote_envelope(
    correlation_id: &str,
    mint: &str,
    amount_sats: u64,
) -> Vec<u8> {
    use flatbuffers::{FlatBufferBuilder, VOffsetT, WIPOffset};
    use nmp_core::dispatch_envelope::encode_dispatch_envelope;

    const NWDQ_IDENTIFIER: &str = "NWDQ";
    let payload = {
        let mut fbb = FlatBufferBuilder::new();
        let mint_off = fbb.create_string(mint);
        let start = fbb.start_table();
        fbb.push_slot::<u32>(4 as VOffsetT, 1, 0); // slot 0: schema_version = 1
        fbb.push_slot_always::<WIPOffset<&str>>(6 as VOffsetT, mint_off); // slot 1: mint
        fbb.push_slot::<u64>(8 as VOffsetT, amount_sats, 0); // slot 2: amount_sats
        let root = fbb.end_table(start);
        fbb.finish(root, Some(NWDQ_IDENTIFIER));
        fbb.finished_data().to_vec()
    };
    encode_dispatch_envelope(correlation_id, CASHU_DEPOSIT_QUOTE, 1, &payload)
}

/// `walletCashuDepositQuote` builder bytes decode field-for-field and dispatch
/// through `start_bytes`/`execute_bytes`.
#[test]
fn cashu_deposit_quote_builder_bytes_round_trip() {
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;

    let registry = registry_with_wallet_actions();
    let mint = "https://mint.example.com".to_string();
    let bytes = build_cashu_deposit_quote_envelope("corr-deposit-quote", &mint, 21_000);

    let decoded = decode_dispatch_envelope(&bytes).expect("builder envelope must decode (S2)");
    assert_eq!(decoded.action_namespace, CASHU_DEPOSIT_QUOTE);
    assert_eq!(
        CashuDepositQuoteAction::decode(&decoded.payload).expect("payload must decode"),
        CashuDepositQuoteAction {
            mint: mint.clone(),
            amount_sats: 21_000,
        },
        "walletCashuDepositQuote builder bytes must decode field-for-field"
    );
    let _ = dispatch_ok(&registry, CASHU_DEPOSIT_QUOTE, &decoded.payload);
}
