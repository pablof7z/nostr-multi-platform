//! Registry-level trip tests for the nip57 typed FlatBuffers payload doorway
//! (ADR-0064 / S9 #1747).
//!
//! These tests prove the fail-closed `schema_version` gate in
//! `ActionRegistry::start_bytes` rejects bad payloads BEFORE `start()` runs,
//! and that a valid payload round-trips through the registry path.
//!
//! Codec round-trip tests (positive + per-field negative) live in
//! `src/wire/zap_payload_tests.rs`. These tests sit one level up — at the
//! registry boundary — so they exercise the same path the byte transport
//! (S2 `DispatchEnvelope`) drives in production.

// ---- S9 gap tests: bad-version trip for nmp.nip57.zap ----------------------

/// ADR-0064 / S9 (#1747) — `nmp.nip57.zap` with a bad `schema_version` MUST
/// be rejected BEFORE `start()` runs, proving the fail-closed gate covers the
/// zap namespace at the registry level.
#[test]
fn start_bytes_rejects_wrong_schema_version_for_zap() {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::substrate::{ActionContext, ActionRejection};

    let mut registry = ActionRegistry::new();
    nmp_nip57::register_actions(&mut registry);

    let bad_version = build_bad_version_zap_payload();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip57.zap",
            &bad_version,
        )
        .expect_err("a wrong schema_version must be rejected before start() (fail closed)");
    match err {
        ActionRejection::Invalid(msg) => assert!(
            msg.contains("schema_version mismatch"),
            "rejection must name the version trip: {msg}"
        ),
        other => panic!("expected Invalid rejection, got {other:?}"),
    }
}

/// ADR-0064 / S9 (#1747) — A valid `nmp.nip57.zap` FlatBuffers payload with
/// a legal `recipient_pubkey` and `amount_msats > 0` must be accepted by the
/// registry's `start_bytes` path, proving the full typed-decode → start() chain
/// works end-to-end.
#[test]
fn start_bytes_accepts_valid_zap_payload() {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::substrate::{ActionContext, ActionPayload};
    use nmp_nip57::ZapInput;

    let mut registry = ActionRegistry::new();
    nmp_nip57::register_actions(&mut registry);

    let action = ZapInput {
        recipient_pubkey: "a".repeat(64),
        amount_msats: 21_000,
        lnurl: None,
        relays: vec![],
        target_event_id: None,
        comment: None,
    };
    let encoded = action.encode();
    registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip57.zap",
            &encoded,
        )
        .expect("valid zap payload must be accepted");
}

// ---- helpers: build bad-version FlatBuffers buffers -------------------------

/// A finished `ZapPayload` (file identifier `N57Z`) with `schema_version = 999`.
/// The fail-closed gate must reject it before `start` runs.
fn build_bad_version_zap_payload() -> Vec<u8> {
    // Inline the generated FlatBuffers API so this integration test does not
    // need to expose the private `wire` module from nmp-nip57. The wire layout
    // (identifier, vtable offsets) is fixed by the committed .fbs schema.
    use flatbuffers::FlatBufferBuilder;

    // Mirror the ZapPayload vtable slots from zap_generated.rs:
    //   VT_SCHEMA_VERSION = 4, VT_RECIPIENT_PUBKEY = 6, VT_AMOUNT_MSATS = 8
    const ZAP_IDENTIFIER: &str = "N57Z";
    const VT_SCHEMA_VERSION: flatbuffers::VOffsetT = 4;
    const VT_RECIPIENT_PUBKEY: flatbuffers::VOffsetT = 6;
    const VT_AMOUNT_MSATS: flatbuffers::VOffsetT = 8;

    let mut fbb = FlatBufferBuilder::new();
    let recipient_pubkey = fbb.create_string(&"a".repeat(64));

    let start = fbb.start_table();
    // schema_version = 999 (the tripwire value)
    fbb.push_slot::<u32>(VT_SCHEMA_VERSION, 999, 0);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_RECIPIENT_PUBKEY, recipient_pubkey);
    fbb.push_slot::<u64>(VT_AMOUNT_MSATS, 21_000, 0);
    let root = fbb.end_table(start);
    fbb.finish(root, Some(ZAP_IDENTIFIER));
    fbb.finished_data().to_vec()
}
