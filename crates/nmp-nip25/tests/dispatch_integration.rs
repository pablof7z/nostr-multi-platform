//! Registry-level trip tests for the nip25 typed FlatBuffers payload doorway
//! (ADR-0064 / S3 #1751).
//!
//! These are NEGATIVE tests only: they prove the fail-closed `schema_version`
//! gate in `ActionRegistry::start_bytes` rejects bad payloads BEFORE `start()`
//! runs, for every nip25 namespace that was migrated in S3.
//!
//! Codec round-trip tests (positive + per-field negative) live in
//! `src/wire/tests.rs`. These tests sit one level up — at the registry boundary
//! — so they exercise the same path the byte transport (S2 `DispatchEnvelope`)
//! drives in production.

// ---- S3 gap tests: bad-version trip for every migrated nip25 namespace -------

/// ADR-0064 / S3 (#1751) — `nmp.nip25.react` with a bad `schema_version` MUST
/// be rejected BEFORE `start()` runs, proving the fail-closed gate covers the
/// react namespace at the registry level.
#[test]
fn start_bytes_rejects_wrong_schema_version_for_react() {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::substrate::{ActionContext, ActionRejection};

    let mut registry = ActionRegistry::new();
    nmp_nip25::register_actions(&mut registry);

    let bad_version = build_bad_version_react_payload();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip25.react",
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

/// ADR-0064 / S3 (#1751) — `nmp.nip25.unreact` with a bad `schema_version`
/// MUST be rejected BEFORE `start()` runs.
#[test]
fn start_bytes_rejects_wrong_schema_version_for_unreact() {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::substrate::{ActionContext, ActionRejection};

    let mut registry = ActionRegistry::new();
    nmp_nip25::register_actions(&mut registry);

    let bad_version = build_bad_version_unreact_payload();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip25.unreact",
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

// ---- helpers: build bad-version FlatBuffers buffers --------------------------

/// A finished `ReactPayload` (file identifier `N25R`) with `schema_version = 999`.
/// The fail-closed gate must reject it before `start` runs.
fn build_bad_version_react_payload() -> Vec<u8> {
    // Inline the generated FlatBuffers API so this integration test does not
    // need to expose the private `wire` module from nmp-nip25. The wire layout
    // (identifier, vtable offsets) is fixed by the committed .fbs schema.
    use flatbuffers::FlatBufferBuilder;

    // Mirror the ReactPayload vtable slots from react_generated.rs:
    //   VT_SCHEMA_VERSION = 4, VT_TARGET_EVENT_ID = 6, VT_REACTION = 8
    const REACT_IDENTIFIER: &str = "N25R";
    const VT_SCHEMA_VERSION: flatbuffers::VOffsetT = 4;
    const VT_TARGET_EVENT_ID: flatbuffers::VOffsetT = 6;
    const VT_REACTION: flatbuffers::VOffsetT = 8;

    let mut fbb = FlatBufferBuilder::new();
    let target = fbb.create_string(&"a".repeat(64));
    let reaction = fbb.create_string("+");

    let start = fbb.start_table();
    // schema_version = 999 (the tripwire value)
    fbb.push_slot::<u32>(VT_SCHEMA_VERSION, 999, 0);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_TARGET_EVENT_ID, target);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_REACTION, reaction);
    let root = fbb.end_table(start);
    fbb.finish(root, Some(REACT_IDENTIFIER));
    fbb.finished_data().to_vec()
}

/// A finished `UnreactPayload` (file identifier `N25U`) with `schema_version = 999`.
fn build_bad_version_unreact_payload() -> Vec<u8> {
    use flatbuffers::FlatBufferBuilder;

    // Mirror the UnreactPayload vtable slots from unreact_generated.rs:
    //   VT_SCHEMA_VERSION = 4, VT_REACTION_EVENT_ID = 6, VT_REASON = 8
    const UNREACT_IDENTIFIER: &str = "N25U";
    const VT_SCHEMA_VERSION: flatbuffers::VOffsetT = 4;
    const VT_REACTION_EVENT_ID: flatbuffers::VOffsetT = 6;
    const VT_REASON: flatbuffers::VOffsetT = 8;

    let mut fbb = FlatBufferBuilder::new();
    let id = fbb.create_string(&"e".repeat(64));
    let reason = fbb.create_string("");

    let start = fbb.start_table();
    fbb.push_slot::<u32>(VT_SCHEMA_VERSION, 999, 0);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_REACTION_EVENT_ID, id);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_REASON, reason);
    let root = fbb.end_table(start);
    fbb.finish(root, Some(UNREACT_IDENTIFIER));
    fbb.finished_data().to_vec()
}

// ---- ADR-0064 §3 (#1783): generated-builder wire round-trip -----------------
//
// The Swift/Kotlin action-builders (`crates/nmp-codegen/src/action_builders`)
// hand-roll the SAME FlatBuffers slot layout this helper builds: the payload
// table (`schema_version` at slot 0 / vtable 4, then the data fields), wrapped
// in a 4-slot `DispatchEnvelope` (`correlation_id`, `action_namespace`,
// `schema_version`, `payload`) finished with the `NMPD` identifier. This test
// proves bytes shaped THAT way decode via S2 (`decode_dispatch_envelope`) to the
// right namespace + an opaque payload that the per-crate `ReactAction::decode`
// reads back field-for-field. If a future emitter change broke the slot order or
// the envelope shape, this round-trip would fail — the authoritative wire guard
// the emitter unit tests in nmp-codegen can't provide (that crate has no
// nmp-core dep).

/// Build a `nmp.nip25.react` `DispatchEnvelope` exactly as the generated host
/// builders do: encode the `ReactPayload` (N25R), then stamp it into the open
/// envelope (NMPD) at the canonical 4 slots.
fn build_react_dispatch_envelope(
    correlation_id: &str,
    target_event_id: &str,
    reaction: &str,
) -> Vec<u8> {
    use flatbuffers::{FlatBufferBuilder, VOffsetT, WIPOffset};

    // --- payload (mirrors the generated builder's payload encode) -----------
    const REACT_IDENTIFIER: &str = "N25R";
    let payload = {
        let mut fbb = FlatBufferBuilder::new();
        let target = fbb.create_string(target_event_id);
        let reaction_off = fbb.create_string(reaction);
        let start = fbb.start_table();
        fbb.push_slot::<u32>(4, 1, 0); // slot 0: schema_version = 1
        fbb.push_slot_always::<WIPOffset<&str>>(6, target); // slot 1
        fbb.push_slot_always::<WIPOffset<&str>>(8, reaction_off); // slot 2
        let root = fbb.end_table(start);
        fbb.finish(root, Some(REACT_IDENTIFIER));
        fbb.finished_data().to_vec()
    };

    // --- envelope (mirrors the generated `encodeDispatchEnvelope`) ----------
    const ENVELOPE_IDENTIFIER: &str = "NMPD";
    let mut fbb = FlatBufferBuilder::new();
    let correlation = fbb.create_string(correlation_id);
    let namespace = fbb.create_string("nmp.nip25.react");
    let payload_vec = fbb.create_vector(&payload);
    let start = fbb.start_table();
    fbb.push_slot_always::<WIPOffset<&str>>(4 as VOffsetT, correlation); // slot 0
    fbb.push_slot_always::<WIPOffset<&str>>(6 as VOffsetT, namespace); // slot 1
    fbb.push_slot::<u32>(8 as VOffsetT, 1, 0); // slot 2: schema_version = 1
    fbb.push_slot_always::<WIPOffset<flatbuffers::Vector<u8>>>(10 as VOffsetT, payload_vec); // slot 3
    let root = fbb.end_table(start);
    fbb.finish(root, Some(ENVELOPE_IDENTIFIER));
    fbb.finished_data().to_vec()
}

/// A builder-shaped `nmp.nip25.react` envelope decodes via S2 to the right
/// namespace + correlation id, and its opaque payload reads back through
/// `ReactAction::decode` field-for-field — i.e. the generated Swift/Kotlin wire
/// layout is consumable by the native byte doorway.
#[test]
fn generated_react_builder_bytes_round_trip() {
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;
    use nmp_core::substrate::ActionPayload;
    use nmp_nip25::ReactAction;

    let event_id = "a".repeat(64);
    let bytes = build_react_dispatch_envelope("corr-123", &event_id, "+");

    let decoded = decode_dispatch_envelope(&bytes).expect("builder envelope must decode (S2)");
    assert_eq!(decoded.correlation_id, "corr-123");
    assert_eq!(decoded.action_namespace, "nmp.nip25.react");

    let action = ReactAction::decode(&decoded.payload)
        .expect("the opaque payload must decode via the per-crate ActionPayload");
    assert_eq!(action.target_event_id, event_id);
    assert_eq!(action.reaction, "+");
    assert_eq!(action.target_author_pubkey, None);
}
