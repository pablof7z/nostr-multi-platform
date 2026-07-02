//! Registry-level trip tests for the nip57 typed FlatBuffers payload doorway
//! (ADR-0071 / S9 #1747).
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

/// ADR-0071 / S9 (#1747) — `nmp.nip57.zap` with a bad `schema_version` MUST
/// be rejected BEFORE `start()` runs, proving the fail-closed gate covers the
/// zap namespace at the registry level.
#[test]
fn start_bytes_rejects_wrong_schema_version_for_zap() {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::substrate::{ActionContext, ActionRejection};

    let mut registry = ActionRegistry::new();
    nmp_nip57::register(&mut registry, nmp_nip57::Config::default())
        .expect("nmp-nip57 registration must not collide");

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

/// ADR-0071 / S9 (#1747) — A valid `nmp.nip57.zap` FlatBuffers payload with
/// a legal `recipient_pubkey` and `amount_msats > 0` must be accepted by the
/// registry's `start_bytes` path, proving the full typed-decode → start() chain
/// works end-to-end.
#[test]
fn start_bytes_accepts_valid_zap_payload() {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::substrate::{ActionContext, ActionPayload};
    use nmp_nip57::ZapInput;

    let mut registry = ActionRegistry::new();
    nmp_nip57::register(&mut registry, nmp_nip57::Config::default())
        .expect("nmp-nip57 registration must not collide");

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

// ---- ADR-0071 §3 (#1783 / M14-1 PR2 #2145): generated `zap` builder round-trip
//
// The Swift/Kotlin/TS `zap` action-builder
// (`crates/nmp-codegen/src/action_builders/registry/table.rs`) hand-rolls the
// `ZapPayload` slot layout (schema_version at slot 0 / vt4, then recipient_pubkey,
// amount_msats:u64, lnurl?, relays:[string], target_event_id?, comment?), wrapped
// in the 4-slot `DispatchEnvelope` (NMPD). This proves bytes shaped THAT way
// decode via S2 (`decode_dispatch_envelope`) to `nmp.nip57.zap` and read back
// through `ZapInput::decode` field-for-field (u64 amount + optional presence).

/// Build a `nmp.nip57.zap` `DispatchEnvelope` exactly as the generated host
/// `zap` builder does: encode the `ZapPayload` (N57Z) at its declaration slots,
/// then stamp it into the open envelope (NMPD) at the canonical 4 slots.
/// Optional fields (`lnurl`, `target_event_id`, `comment`) are omitted from the
/// buffer when `None`; `relays` is always written as a vector (possibly empty).
fn build_zap_dispatch_envelope(
    correlation_id: &str,
    recipient_pubkey: &str,
    amount_msats: u64,
    lnurl: Option<&str>,
    relays: &[&str],
    target_event_id: Option<&str>,
    comment: Option<&str>,
) -> Vec<u8> {
    use flatbuffers::{FlatBufferBuilder, VOffsetT, WIPOffset};

    const ZAP_IDENTIFIER: &str = "N57Z";
    let payload = {
        let mut fbb = FlatBufferBuilder::new();
        let recipient_off = fbb.create_string(recipient_pubkey);
        let lnurl_off = lnurl.map(|s| fbb.create_string(s));
        let relay_offs: Vec<WIPOffset<&str>> =
            relays.iter().map(|s| fbb.create_string(s)).collect();
        let relays_off = fbb.create_vector(&relay_offs);
        let target_off = target_event_id.map(|s| fbb.create_string(s));
        let comment_off = comment.map(|s| fbb.create_string(s));
        let start = fbb.start_table();
        fbb.push_slot::<u32>(4 as VOffsetT, 1, 0); // slot 0: schema_version = 1
        fbb.push_slot_always::<WIPOffset<&str>>(6 as VOffsetT, recipient_off); // slot 1
        fbb.push_slot::<u64>(8 as VOffsetT, amount_msats, 0); // slot 2: amount_msats
        if let Some(off) = lnurl_off {
            fbb.push_slot_always::<WIPOffset<&str>>(10 as VOffsetT, off); // slot 3
        }
        fbb.push_slot_always::<WIPOffset<_>>(12 as VOffsetT, relays_off); // slot 4
        if let Some(off) = target_off {
            fbb.push_slot_always::<WIPOffset<&str>>(14 as VOffsetT, off); // slot 5
        }
        if let Some(off) = comment_off {
            fbb.push_slot_always::<WIPOffset<&str>>(16 as VOffsetT, off); // slot 6
        }
        let root = fbb.end_table(start);
        fbb.finish(root, Some(ZAP_IDENTIFIER));
        fbb.finished_data().to_vec()
    };
    nmp_core::dispatch_envelope::encode_dispatch_envelope(
        correlation_id,
        "nmp.nip57.zap",
        1,
        &payload,
    )
}

/// `zap` builder bytes decode via S2 to `nmp.nip57.zap` + correlation id, and
/// the opaque payload reads back through `ZapInput::decode` field-for-field —
/// the u64 `amount_msats` is preserved exactly and the optional `lnurl`,
/// `target_event_id`, `comment` round-trip as `Some`, with `relays` preserved.
#[test]
fn generated_zap_builder_bytes_round_trip_full() {
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;
    use nmp_core::substrate::ActionPayload;
    use nmp_nip57::ZapInput;

    let recipient = "a".repeat(64);
    let target = "e".repeat(64);
    let bytes = build_zap_dispatch_envelope(
        "corr-zap",
        &recipient,
        21_000_000,
        Some("alice@example.com"),
        &["wss://relay.one", "wss://relay.two"],
        Some(&target),
        Some("🤙 nice"),
    );

    let decoded = decode_dispatch_envelope(&bytes).expect("zap envelope must decode (S2)");
    assert_eq!(decoded.correlation_id, "corr-zap");
    assert_eq!(decoded.action_namespace, "nmp.nip57.zap");

    let action = ZapInput::decode(&decoded.payload)
        .expect("the opaque payload must decode via ZapInput::decode");
    assert_eq!(action.recipient_pubkey, recipient);
    assert_eq!(action.amount_msats, 21_000_000_u64);
    assert_eq!(action.lnurl.as_deref(), Some("alice@example.com"));
    assert_eq!(
        action.relays,
        vec!["wss://relay.one".to_string(), "wss://relay.two".to_string()]
    );
    assert_eq!(action.target_event_id.as_deref(), Some(target.as_str()));
    assert_eq!(action.comment.as_deref(), Some("🤙 nice"));
}

/// `zap` builder bytes with `lnurl: Some("")`, `target_event_id: Some("")`,
/// `comment: Some("")` decode to a `ZapInput` whose optional string fields are
/// `Some("")` — a present-but-empty string is distinct from an absent field and
/// must NOT be collapsed to `None`.
#[test]
fn generated_zap_builder_bytes_empty_optionals_round_trip() {
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;
    use nmp_core::substrate::ActionPayload;
    use nmp_nip57::ZapInput;

    let recipient = "c".repeat(64);
    let bytes = build_zap_dispatch_envelope(
        "corr-zap-empty-opts",
        &recipient,
        500,
        Some(""), // lnurl: Some("")
        &[],
        Some(""), // target_event_id: Some("")
        Some(""), // comment: Some("")
    );

    let decoded = decode_dispatch_envelope(&bytes)
        .expect("zap envelope with empty optionals must decode (S2)");
    assert_eq!(decoded.action_namespace, "nmp.nip57.zap");

    let action = ZapInput::decode(&decoded.payload)
        .expect("payload with empty optionals must decode via ZapInput::decode");
    assert_eq!(action.recipient_pubkey, recipient);
    assert_eq!(
        action.lnurl.as_deref(),
        Some(""),
        "present-empty lnurl must round-trip as Some(\"\"), not collapse to None"
    );
    assert_eq!(
        action.target_event_id.as_deref(),
        Some(""),
        "present-empty target_event_id must round-trip as Some(\"\"), not collapse to None"
    );
    assert_eq!(
        action.comment.as_deref(),
        Some(""),
        "present-empty comment must round-trip as Some(\"\"), not collapse to None"
    );
}

/// `zap` builder bytes with all optionals omitted (a profile zap, no target /
/// lnurl / comment, empty relays) decode to a `ZapInput` whose optional fields
/// are `None` and whose `relays` is empty — proving FlatBuffers field-absence
/// round-trips as `None`/empty (not a silent default), and the bytes dispatch
/// end-to-end through `start_bytes`.
#[test]
fn generated_zap_builder_bytes_minimal_dispatch_through_start_bytes() {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;
    use nmp_core::substrate::{ActionContext, ActionPayload, ActionRejection};
    use nmp_nip57::ZapInput;

    let recipient = "b".repeat(64);
    let bytes =
        build_zap_dispatch_envelope("corr-zap-min", &recipient, 1_000, None, &[], None, None);

    let decoded = decode_dispatch_envelope(&bytes).expect("zap envelope must decode (S2)");
    assert_eq!(decoded.action_namespace, "nmp.nip57.zap");
    let action = ZapInput::decode(&decoded.payload).expect("minimal zap payload must decode");
    assert_eq!(action.recipient_pubkey, recipient);
    assert_eq!(action.amount_msats, 1_000_u64);
    assert_eq!(action.lnurl, None);
    assert!(action.relays.is_empty());
    assert_eq!(action.target_event_id, None);
    assert_eq!(action.comment, None);

    let mut registry = ActionRegistry::new();
    nmp_nip57::register(&mut registry, nmp_nip57::Config::default())
        .expect("nmp-nip57 registration must not collide");
    // POSITIVE: routed to nmp.nip57.zap, payload decodes + start() OK.
    registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            &decoded.action_namespace,
            &decoded.payload,
        )
        .expect("zap builder bytes must dispatch + validate via start_bytes");
    // LOAD-BEARING: the same bytes under a wrong namespace must fail closed,
    // proving the positive above is not vacuous.
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip17.send",
            &decoded.payload,
        )
        .expect_err("a ZapPayload routed as nmp.nip17.send must be rejected");
    assert!(
        matches!(err, ActionRejection::Invalid(_)),
        "wrong-namespace dispatch must fail closed as Invalid, got {err:?}"
    );
}
