//! Registry-level trip tests for the typed FlatBuffers payload doorway
//! (ADR-0071 / S3 #1751, builders #1783).
//!
//! Two kinds of test live here, both at the `ActionRegistry::start_bytes`
//! registry boundary — the same path the byte transport (S2 `DispatchEnvelope`)
//! drives in production:
//!
//! * NEGATIVE: the fail-closed `schema_version` gate rejects bad payloads BEFORE
//!   `start()` runs, for every nip25 namespace migrated in S3.
//! * POSITIVE / load-bearing (#1783): bytes shaped EXACTLY as the generated
//!   Swift/Kotlin action-builders emit (`crates/nmp-codegen/src/action_builders`)
//!   are routed END TO END through `ActionRegistry::start_bytes` and accepted —
//!   for `nmp.nip25.react` (a flat table) AND `nmp.publish` (a UNION body). These
//!   are the authoritative guard the emitter unit tests in nmp-codegen cannot
//!   provide (that crate has no nmp-core dep): each asserts the bytes dispatch to
//!   the RIGHT namespace, and a wrong-namespace twin proves the assertion bites.
//!
//! Codec round-trip tests (positive + per-field negative) live in
//! `src/wire/tests.rs`.

// ---- S3 gap tests: bad-version trip for every migrated nip25 namespace -------

/// ADR-0071 / S3 (#1751) — `nmp.nip25.react` with a bad `schema_version` MUST
/// be rejected BEFORE `start()` runs, proving the fail-closed gate covers the
/// react namespace at the registry level.
#[test]
fn start_bytes_rejects_wrong_schema_version_for_react() {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::substrate::{ActionContext, ActionRejection};

    let mut registry = ActionRegistry::new();
    nmp_core::substrate::ProtocolDescriptor::register_actions(
        &nmp_nip25::Nip25Descriptor,
        &mut registry,
    );

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

/// ADR-0071 / S3 (#1751) — `nmp.nip25.unreact` with a bad `schema_version`
/// MUST be rejected BEFORE `start()` runs.
#[test]
fn start_bytes_rejects_wrong_schema_version_for_unreact() {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::substrate::{ActionContext, ActionRejection};

    let mut registry = ActionRegistry::new();
    nmp_core::substrate::ProtocolDescriptor::register_actions(
        &nmp_nip25::Nip25Descriptor,
        &mut registry,
    );

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

// ---- ADR-0071 §3 (#1783): generated-builder wire round-trip -----------------
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

// ---- #1783: LOAD-BEARING start_bytes round-trip (react + publish) ------------
//
// The test above stops at `decode_dispatch_envelope` + a direct
// `ReactAction::decode`; it does NOT route the builder bytes through the actual
// registry doorway. The tests below close that gap: they feed builder-shaped
// bytes through `ActionRegistry::start_bytes` END TO END (the production path:
// S2 envelope decode → adapter typed decode + fail-closed gate → `start()`).
// They are load-bearing — the wrong-namespace twins prove a malformed/misrouted
// builder would be rejected, so a passing positive is a real signal.

/// react builder bytes dispatch through `start_bytes` to the registered
/// `nmp.nip25.react` module and are ACCEPTED (start() validated the decoded
/// payload). The wrong-namespace twin proves the route is real: the SAME bytes
/// sent under `nmp.nip25.unreact` are rejected (the unreact decoder can't read a
/// ReactPayload), so a passing positive means the slot order + namespace stamp
/// the emitter produced are correct.
#[test]
fn react_builder_bytes_dispatch_through_start_bytes() {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;
    use nmp_core::substrate::ActionContext;

    let mut registry = ActionRegistry::new();
    nmp_core::substrate::ProtocolDescriptor::register_actions(
        &nmp_nip25::Nip25Descriptor,
        &mut registry,
    );

    let event_id = "a".repeat(64);
    let bytes = build_react_dispatch_envelope("corr-react", &event_id, "+");
    let decoded = decode_dispatch_envelope(&bytes).expect("builder envelope must decode (S2)");
    assert_eq!(decoded.action_namespace, "nmp.nip25.react");

    // POSITIVE: routed to the right namespace, the payload decodes + start() OK.
    registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            &decoded.action_namespace,
            &decoded.payload,
        )
        .expect("react builder bytes must dispatch + validate via start_bytes");

    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip25.unreact",
            &decoded.payload,
        )
        .expect_err("a ReactPayload routed as unreact must be rejected");
    assert!(
        matches!(err, nmp_core::substrate::ActionRejection::Invalid(_)),
        "wrong-namespace dispatch must fail closed as Invalid, got {err:?}"
    );
}

fn build_publish_raw_envelope_and_action(
    correlation_id: &str,
    kind: u32,
    tags: &[Vec<String>],
    content: &str,
    relays: Option<Vec<String>>,
    signer_pubkey: Option<String>,
) -> (Vec<u8>, nmp_core::publish::PublishAction) {
    use flatbuffers::{FlatBufferBuilder, VOffsetT, WIPOffset};
    use nmp_core::dispatch_envelope::encode_dispatch_envelope;
    use nmp_core::publish::{
        PublishAction, PublishRouteClass, PublishSigner, PublishSignerProvenance, PublishTarget,
    };
    use nmp_core::substrate::ActionPayload;

    const PUBLISH_SCHEMA_VERSION: u32 = <PublishAction as ActionPayload>::SCHEMA_VERSION;
    const BODY_PUBLISH_RAW: u8 = 2; // union discriminant (decl order, NONE=0)
    let payload = {
        let mut fbb = FlatBufferBuilder::new();
        let tag_rows: Vec<WIPOffset<_>> = tags
            .iter()
            .map(|row| {
                let vals: Vec<WIPOffset<&str>> = row.iter().map(|s| fbb.create_string(s)).collect();
                let vals_vec = fbb.create_vector(&vals);
                let start = fbb.start_table();
                fbb.push_slot_always::<WIPOffset<_>>(4 as VOffsetT, vals_vec);
                fbb.end_table(start)
            })
            .collect();
        let tags_vec = fbb.create_vector(&tag_rows);
        let content_off = fbb.create_string(content);
        let signer = signer_pubkey.as_ref().map(|pubkey| {
            let pubkey = fbb.create_string(pubkey);
            let provenance = fbb.create_string(PublishSignerProvenance::AppManaged.wire_token());
            let start = fbb.start_table();
            fbb.push_slot::<u8>(4 as VOffsetT, 1, 0); // PublishSignerMode::Registered
            fbb.push_slot_always::<WIPOffset<&str>>(6 as VOffsetT, pubkey);
            fbb.push_slot_always::<WIPOffset<&str>>(8 as VOffsetT, provenance);
            fbb.end_table(start)
        });
        let target = {
            let relay_list = relays.clone().unwrap_or_default();
            let explicit = !relay_list.is_empty();
            let relay_offs: Vec<WIPOffset<&str>> =
                relay_list.iter().map(|s| fbb.create_string(s)).collect();
            let relays_vec = fbb.create_vector(&relay_offs);
            let route_class = fbb.create_string(PublishRouteClass::ManualOverride.wire_token());
            let start = fbb.start_table();
            fbb.push_slot::<bool>(4 as VOffsetT, explicit, false);
            fbb.push_slot_always::<WIPOffset<_>>(6 as VOffsetT, relays_vec);
            fbb.push_slot_always::<WIPOffset<&str>>(8 as VOffsetT, route_class);
            fbb.end_table(start)
        };
        let start = fbb.start_table();
        fbb.push_slot::<u32>(4 as VOffsetT, kind, 0);
        fbb.push_slot_always::<WIPOffset<_>>(6 as VOffsetT, tags_vec);
        fbb.push_slot_always::<WIPOffset<&str>>(8 as VOffsetT, content_off);
        fbb.push_slot_always::<WIPOffset<_>>(10 as VOffsetT, target);
        if let Some(off) = signer {
            fbb.push_slot_always::<WIPOffset<flatbuffers::TableFinishedWIPOffset>>(
                12 as VOffsetT,
                off,
            );
        }
        let body = fbb.end_table(start);
        let start = fbb.start_table();
        fbb.push_slot::<u32>(4 as VOffsetT, PUBLISH_SCHEMA_VERSION, 0);
        fbb.push_slot::<u8>(6 as VOffsetT, BODY_PUBLISH_RAW, 0);
        fbb.push_slot_always::<WIPOffset<flatbuffers::TableFinishedWIPOffset>>(8 as VOffsetT, body); // slot 2: body (union value = table offset)
        let root = fbb.end_table(start);
        fbb.finish(root, Some("NPUB"));
        fbb.finished_data().to_vec()
    };
    let envelope = encode_dispatch_envelope(correlation_id, "nmp.publish", 1, &payload);

    let target = match relays {
        Some(r) if !r.is_empty() => PublishTarget::explicit(r, PublishRouteClass::ManualOverride),
        _ => PublishTarget::Auto,
    };
    let expected = PublishAction::PublishRaw {
        kind,
        tags: tags.to_vec(),
        content: content.to_string(),
        target,
        signer: signer_pubkey
            .map(|pubkey| PublishSigner::registered(pubkey, PublishSignerProvenance::AppManaged))
            .unwrap_or_default(),
    };
    (envelope, expected)
}

#[test]
fn publish_raw_builder_bytes_dispatch_through_start_bytes() {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;
    use nmp_core::publish::PublishAction;
    use nmp_core::substrate::{ActionContext, ActionPayload, ActionRejection};

    let mut registry = ActionRegistry::new();
    registry
        .register(nmp_core::publish::PublishModule)
        .expect("register publish module");

    let (bytes, expected) = build_publish_raw_envelope_and_action(
        "corr-pub-raw",
        30023,
        &[
            vec!["d".to_string(), "slug".to_string()],
            vec!["title".to_string(), "T".to_string()],
        ],
        "body",
        Some(vec![
            "wss://relay.one".to_string(),
            "wss://relay.two".to_string(),
        ]),
        Some("e".repeat(64)),
    );

    let decoded = decode_dispatch_envelope(&bytes).expect("publish envelope must decode (S2)");
    assert_eq!(decoded.action_namespace, "nmp.publish");
    assert_eq!(
        PublishAction::decode(&decoded.payload).expect("builder bytes must decode"),
        expected,
        "publishRaw builder bytes must decode field-for-field"
    );

    registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            &decoded.action_namespace,
            &decoded.payload,
        )
        .expect("publishRaw builder bytes must dispatch + validate via start_bytes");

    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip25.react",
            &decoded.payload,
        )
        .expect_err("a publish payload routed as react must be rejected");
    assert!(
        matches!(err, ActionRejection::Invalid(_)),
        "wrong-namespace dispatch must fail closed as Invalid, got {err:?}"
    );
}

#[test]
fn publish_profile_builder_bytes_dispatch_through_start_bytes() {
    use flatbuffers::{FlatBufferBuilder, VOffsetT, WIPOffset};
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::dispatch_envelope::{decode_dispatch_envelope, encode_dispatch_envelope};
    use nmp_core::publish::PublishAction;
    use nmp_core::substrate::{ActionContext, ActionPayload, ActionRejection};
    const PUBLISH_SCHEMA_VERSION: u32 = <PublishAction as ActionPayload>::SCHEMA_VERSION;
    const BODY_PUBLISH_PROFILE: u8 = 1;
    let fields = vec![("name".to_string(), "Alice".to_string())];
    let payload = {
        let mut fbb = FlatBufferBuilder::new();
        let field_rows: Vec<WIPOffset<_>> = fields
            .iter()
            .map(|(k, v)| {
                let key = fbb.create_string(k);
                let value = fbb.create_string(v);
                let start = fbb.start_table();
                fbb.push_slot_always::<WIPOffset<&str>>(4 as VOffsetT, key);
                fbb.push_slot_always::<WIPOffset<&str>>(6 as VOffsetT, value);
                fbb.end_table(start)
            })
            .collect();
        let fields_vec = fbb.create_vector(&field_rows);
        let start = fbb.start_table();
        fbb.push_slot_always::<WIPOffset<_>>(4 as VOffsetT, fields_vec);
        let body = fbb.end_table(start);
        let start = fbb.start_table();
        fbb.push_slot::<u32>(4 as VOffsetT, PUBLISH_SCHEMA_VERSION, 0);
        fbb.push_slot::<u8>(6 as VOffsetT, BODY_PUBLISH_PROFILE, 0);
        fbb.push_slot_always::<WIPOffset<flatbuffers::TableFinishedWIPOffset>>(8 as VOffsetT, body); // slot 2: body (union value = table offset)
        let root = fbb.end_table(start);
        fbb.finish(root, Some("NPUB"));
        fbb.finished_data().to_vec()
    };
    let bytes = encode_dispatch_envelope("corr-pub-prof", "nmp.publish", 1, &payload);
    let mut map = serde_json::Map::new();
    for (k, v) in &fields {
        map.insert(k.clone(), serde_json::Value::String(v.clone()));
    }
    let expected = PublishAction::PublishProfile { fields: map };
    let mut registry = ActionRegistry::new();
    registry
        .register(nmp_core::publish::PublishModule)
        .expect("register publish module");
    let decoded = decode_dispatch_envelope(&bytes).expect("publish envelope must decode (S2)");
    assert_eq!(decoded.action_namespace, "nmp.publish");
    assert_eq!(
        PublishAction::decode(&decoded.payload).expect("builder bytes must decode"),
        expected,
        "publishProfile builder bytes must decode field-for-field"
    );
    registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            &decoded.action_namespace,
            &decoded.payload,
        )
        .expect("publishProfile builder bytes must dispatch + validate via start_bytes");
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip25.react",
            &decoded.payload,
        )
        .expect_err("publishProfile payload routed as nmp.nip25.react must be rejected");
    assert!(
        matches!(err, ActionRejection::Invalid(_)),
        "wrong-namespace dispatch must fail closed as Invalid, got {err:?}"
    );
}
