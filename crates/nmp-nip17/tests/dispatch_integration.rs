//! Registry-level trip tests for the nip17 typed FlatBuffers payload doorway
//! (ADR-0071 / S9 #1747).
//!
//! These are NEGATIVE tests only: they prove the fail-closed `schema_version`
//! gate in `ActionRegistry::start_bytes` rejects bad payloads BEFORE `start()`
//! runs, for every nip17 namespace migrated in S9.
//!
//! Codec round-trip tests (positive + per-field negative) live in
//! `src/wire/action_payload_tests.rs`. These tests sit one level up — at the
//! registry boundary — so they exercise the same path the byte transport (S2
//! `DispatchEnvelope`) drives in production.

// ---- S9 gap tests: bad-version trip for every migrated nip17 namespace ------

/// Build a fresh `ActionRegistry` with the two nip17 action modules wired.
///
/// The full `register_actions` entry point also installs a `DmRelayCache` and
/// `Kind10050Parser` which require additional traits (`DmInboxRelayRegistrar` +
/// `IngestParserRegistrar`) that `ActionRegistry` does not implement. The
/// registry-level payload tests only need the two action modules; the extra
/// seams are irrelevant here.
fn registry_with_nip17() -> nmp_core::__ffi_internal::ActionRegistry {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::substrate::ActionRegistrar;
    let mut registry = ActionRegistry::new();
    let _ = registry.register_action(nmp_nip17::action::SendDmAction);
    let _ = registry.register_action(nmp_nip17::dm_relay_list::PublishDmRelayListAction);
    registry
}

/// ADR-0071 / S9 (#1747) — `nmp.nip17.send` with a bad `schema_version` MUST
/// be rejected BEFORE `start()` runs, proving the fail-closed gate covers the
/// send namespace at the registry level.
#[test]
fn start_bytes_rejects_wrong_schema_version_for_send() {
    use nmp_core::substrate::{ActionContext, ActionRejection};

    let mut registry = registry_with_nip17();
    let bad_version = build_bad_version_send_payload();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip17.send",
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

/// ADR-0071 / S9 (#1747) — `nmp.nip17.publish_relay_list` with a bad
/// `schema_version` MUST be rejected BEFORE `start()` runs.
#[test]
fn start_bytes_rejects_wrong_schema_version_for_publish_relay_list() {
    use nmp_core::substrate::{ActionContext, ActionRejection};

    let mut registry = registry_with_nip17();
    let bad_version = build_bad_version_relay_list_payload();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip17.publish_relay_list",
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

// ---- positive round-trip tests ----------------------------------------------

/// Typed encode → `start_bytes` for `nmp.nip17.send` (positive path).
#[test]
fn start_bytes_accepts_good_send_payload() {
    use nmp_core::substrate::{ActionContext, ActionPayload};

    let mut registry = registry_with_nip17();
    let action = nmp_nip17::SendDmInput {
        recipient_pubkey: "a".repeat(64),
        content: "hello".to_string(),
        reply_to: None,
    };
    let payload = action.encode();
    let id = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip17.send",
            &payload,
        )
        .expect("well-formed send payload must be accepted");
    assert_eq!(id.len(), 32, "minted correlation_id must be 32 hex chars");
}

/// Typed encode → `start_bytes` for `nmp.nip17.publish_relay_list` (positive).
#[test]
fn start_bytes_accepts_good_publish_relay_list_payload() {
    use nmp_core::substrate::{ActionContext, ActionPayload};

    let mut registry = registry_with_nip17();
    let action = nmp_nip17::PublishDmRelayListInput {
        relays: vec!["wss://relay.example".to_string()],
    };
    let payload = action.encode();
    let id = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip17.publish_relay_list",
            &payload,
        )
        .expect("well-formed relay list payload must be accepted");
    assert_eq!(id.len(), 32, "minted correlation_id must be 32 hex chars");
}

// ---- helpers: build bad-version FlatBuffers buffers -------------------------

/// A finished `SendDmPayload` (file identifier `N17S`) with `schema_version = 999`.
/// The fail-closed gate must reject it before `start` runs.
fn build_bad_version_send_payload() -> Vec<u8> {
    // Mirror the SendDmPayload vtable slots from send_generated.rs:
    //   VT_SCHEMA_VERSION = 4, VT_RECIPIENT_PUBKEY = 6, VT_CONTENT = 8
    use flatbuffers::FlatBufferBuilder;

    const SEND_IDENTIFIER: &str = "N17S";
    const VT_SCHEMA_VERSION: flatbuffers::VOffsetT = 4;
    const VT_RECIPIENT_PUBKEY: flatbuffers::VOffsetT = 6;
    const VT_CONTENT: flatbuffers::VOffsetT = 8;

    let mut fbb = FlatBufferBuilder::new();
    let recipient = fbb.create_string(&"a".repeat(64));
    let content = fbb.create_string("hello");

    let start = fbb.start_table();
    // schema_version = 999 (the tripwire value)
    fbb.push_slot::<u32>(VT_SCHEMA_VERSION, 999, 0);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_RECIPIENT_PUBKEY, recipient);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_CONTENT, content);
    let root = fbb.end_table(start);
    fbb.finish(root, Some(SEND_IDENTIFIER));
    fbb.finished_data().to_vec()
}

// ---- M14-1 / #2145: generated-builder wire round-trip (StrVec) --------------
//
// The positive tests above feed the Rust typed `.encode()` through `start_bytes`.
// The test below proves that bytes shaped EXACTLY as the generated Swift/Kotlin
// `publishDmRelayList` builder emits (`crates/nmp-codegen/src/action_builders`)
// — a `relays:[string]` vector at slot 1 of the `N17R` payload, wrapped in the
// `NMPD` envelope — decode back field-for-field and dispatch through the
// registry. This is the authoritative wire guard the codegen emitter unit tests
// cannot provide (that crate has no nmp-core dep).

/// Build a `nmp.nip17.publish_relay_list` `DispatchEnvelope` EXACTLY as the
/// generated `publishDmRelayList(correlationId:relays:)` builder does (N17R;
/// schema_version slot 0, relays string-vector slot 1), stamped into `NMPD`.
fn build_dm_relay_list_envelope(correlation_id: &str, relays: &[&str]) -> Vec<u8> {
    use flatbuffers::{FlatBufferBuilder, VOffsetT, WIPOffset};
    use nmp_core::dispatch_envelope::encode_dispatch_envelope;

    let payload = {
        let mut fbb = FlatBufferBuilder::new();
        let relay_offsets: Vec<WIPOffset<&str>> =
            relays.iter().map(|r| fbb.create_string(r)).collect();
        let relays_vec = fbb.create_vector(&relay_offsets);
        let start = fbb.start_table();
        fbb.push_slot::<u32>(4 as VOffsetT, 1, 0); // slot 0: schema_version = 1
        fbb.push_slot_always::<WIPOffset<_>>(6 as VOffsetT, relays_vec); // slot 1: relays
        let root = fbb.end_table(start);
        fbb.finish(root, Some("N17R"));
        fbb.finished_data().to_vec()
    };
    encode_dispatch_envelope(correlation_id, "nmp.nip17.publish_relay_list", 1, &payload)
}

/// `publishDmRelayList` builder bytes decode field-for-field to
/// `PublishDmRelayListInput` and dispatch through `start_bytes` to
/// `nmp.nip17.publish_relay_list`. The wrong-namespace twin (routed as
/// `nmp.nip17.send`) proves the route is real.
#[test]
fn dm_relay_list_builder_bytes_round_trip() {
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;
    use nmp_core::substrate::{ActionContext, ActionPayload, ActionRejection};

    let registry = registry_with_nip17();
    let relays = ["wss://relay.one", "wss://relay.two"];
    let bytes = build_dm_relay_list_envelope("corr-n17r", &relays);

    let decoded = decode_dispatch_envelope(&bytes).expect("builder envelope must decode (S2)");
    assert_eq!(decoded.action_namespace, "nmp.nip17.publish_relay_list");
    assert_eq!(
        nmp_nip17::PublishDmRelayListInput::decode(&decoded.payload)
            .expect("payload must decode via PublishDmRelayListInput"),
        nmp_nip17::PublishDmRelayListInput {
            relays: relays.iter().map(|r| r.to_string()).collect(),
        },
        "publishDmRelayList builder bytes must decode field-for-field"
    );
    registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            &decoded.action_namespace,
            &decoded.payload,
        )
        .expect("publishDmRelayList builder bytes must dispatch + validate via start_bytes");
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip17.send",
            &decoded.payload,
        )
        .expect_err("an N17R payload routed as send must be rejected");
    assert!(
        matches!(err, ActionRejection::Invalid(_)),
        "wrong-namespace dispatch must fail closed as Invalid, got {err:?}"
    );
}

/// A finished `PublishDmRelayListPayload` (file identifier `N17R`) with
/// `schema_version = 999`. The fail-closed gate must reject it before `start`.
fn build_bad_version_relay_list_payload() -> Vec<u8> {
    // Mirror the PublishDmRelayListPayload vtable slots from
    // dm_relay_list_action_generated.rs:
    //   VT_SCHEMA_VERSION = 4, VT_RELAYS = 6
    use flatbuffers::FlatBufferBuilder;

    const RELAY_LIST_IDENTIFIER: &str = "N17R";
    const VT_SCHEMA_VERSION: flatbuffers::VOffsetT = 4;
    const VT_RELAYS: flatbuffers::VOffsetT = 6;

    let mut fbb = FlatBufferBuilder::new();
    let r = fbb.create_string("wss://relay.example");
    let relays = fbb.create_vector(&[r]);

    let start = fbb.start_table();
    fbb.push_slot::<u32>(VT_SCHEMA_VERSION, 999, 0);
    fbb.push_slot_always::<flatbuffers::WIPOffset<_>>(VT_RELAYS, relays);
    let root = fbb.end_table(start);
    fbb.finish(root, Some(RELAY_LIST_IDENTIFIER));
    fbb.finished_data().to_vec()
}

// ---- ADR-0071 §3 (#1783 / M14-1 PR2 #2145): generated `sendDm` builder round-trip
//
// The Swift/Kotlin/TS `sendDm` action-builder
// (`crates/nmp-codegen/src/action_builders/registry/table.rs`) hand-rolls the
// `SendDmPayload` slot layout (schema_version at slot 0 / vt4, then
// recipient_pubkey, content, reply_to?), wrapped in the 4-slot `DispatchEnvelope`
// (NMPD). This proves bytes shaped THAT way decode via S2
// (`decode_dispatch_envelope`) to `nmp.nip17.send` and read back through
// `SendDmInput::decode` field-for-field, with `reply_to` presence preserved.

/// Build a `nmp.nip17.send` `DispatchEnvelope` exactly as the generated host
/// `sendDm` builder does: encode the `SendDmPayload` (N17S) at its declaration
/// slots, then stamp it into the open envelope (NMPD). `reply_to` is omitted
/// from the buffer when `None`.
fn build_send_dm_dispatch_envelope(
    correlation_id: &str,
    recipient_pubkey: &str,
    content: &str,
    reply_to: Option<&str>,
) -> Vec<u8> {
    use flatbuffers::{FlatBufferBuilder, VOffsetT, WIPOffset};

    const SEND_IDENTIFIER: &str = "N17S";
    let payload = {
        let mut fbb = FlatBufferBuilder::new();
        let recipient_off = fbb.create_string(recipient_pubkey);
        let content_off = fbb.create_string(content);
        let reply_off = reply_to.map(|s| fbb.create_string(s));
        let start = fbb.start_table();
        fbb.push_slot::<u32>(4 as VOffsetT, 1, 0); // slot 0: schema_version = 1
        fbb.push_slot_always::<WIPOffset<&str>>(6 as VOffsetT, recipient_off); // slot 1
        fbb.push_slot_always::<WIPOffset<&str>>(8 as VOffsetT, content_off); // slot 2
        if let Some(off) = reply_off {
            fbb.push_slot_always::<WIPOffset<&str>>(10 as VOffsetT, off); // slot 3
        }
        let root = fbb.end_table(start);
        fbb.finish(root, Some(SEND_IDENTIFIER));
        fbb.finished_data().to_vec()
    };
    nmp_core::dispatch_envelope::encode_dispatch_envelope(
        correlation_id,
        "nmp.nip17.send",
        1,
        &payload,
    )
}

/// `sendDm` builder bytes (with a `reply_to`) decode via S2 to `nmp.nip17.send`
/// + correlation id, and read back through `SendDmInput::decode` field-for-field
/// — including the optional `reply_to` as `Some`.
#[test]
fn generated_send_dm_builder_bytes_round_trip_with_reply() {
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;
    use nmp_core::substrate::ActionPayload;
    use nmp_nip17::action::SendDmInput;

    let recipient = "a".repeat(64);
    let reply = "e".repeat(64);
    let bytes = build_send_dm_dispatch_envelope("corr-dm", &recipient, "gm fren", Some(&reply));

    let decoded = decode_dispatch_envelope(&bytes).expect("send envelope must decode (S2)");
    assert_eq!(decoded.correlation_id, "corr-dm");
    assert_eq!(decoded.action_namespace, "nmp.nip17.send");

    let action = SendDmInput::decode(&decoded.payload)
        .expect("the opaque payload must decode via SendDmInput::decode");
    assert_eq!(action.recipient_pubkey, recipient);
    assert_eq!(action.content, "gm fren");
    assert_eq!(action.reply_to.as_deref(), Some(reply.as_str()));
}

/// `sendDm` builder bytes with `reply_to: Some("")` decode to a `SendDmInput`
/// whose `reply_to` is `Some("")` — a present-but-empty string is a distinct
/// state from an absent field and must NOT be collapsed to `None`.
#[test]
fn generated_send_dm_builder_bytes_empty_reply_to_round_trip() {
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;
    use nmp_core::substrate::ActionPayload;
    use nmp_nip17::action::SendDmInput;

    let recipient = "c".repeat(64);
    let bytes = build_send_dm_dispatch_envelope("corr-dm-empty-reply", &recipient, "hi", Some(""));

    let decoded = decode_dispatch_envelope(&bytes)
        .expect("send envelope with empty reply_to must decode (S2)");
    assert_eq!(decoded.action_namespace, "nmp.nip17.send");

    let action = SendDmInput::decode(&decoded.payload)
        .expect("payload with empty reply_to must decode via SendDmInput::decode");
    assert_eq!(action.recipient_pubkey, recipient);
    assert_eq!(action.content, "hi");
    assert_eq!(
        action.reply_to.as_deref(),
        Some(""),
        "present-empty reply_to must round-trip as Some(\"\"), not collapse to None"
    );
}

/// `sendDm` builder bytes with `reply_to` omitted decode to a `SendDmInput`
/// whose `reply_to` is `None` (field-absence round-trips as `None`), and the
/// bytes dispatch end-to-end through `start_bytes`. The wrong-namespace twin
/// proves the route is real.
#[test]
fn generated_send_dm_builder_bytes_no_reply_dispatch_through_start_bytes() {
    use nmp_core::dispatch_envelope::decode_dispatch_envelope;
    use nmp_core::substrate::{ActionContext, ActionPayload, ActionRejection};
    use nmp_nip17::action::SendDmInput;

    let recipient = "b".repeat(64);
    let bytes = build_send_dm_dispatch_envelope("corr-dm-min", &recipient, "hello", None);

    let decoded = decode_dispatch_envelope(&bytes).expect("send envelope must decode (S2)");
    assert_eq!(decoded.action_namespace, "nmp.nip17.send");
    let action = SendDmInput::decode(&decoded.payload).expect("minimal DM payload must decode");
    assert_eq!(action.recipient_pubkey, recipient);
    assert_eq!(action.content, "hello");
    assert_eq!(action.reply_to, None);

    let mut registry = registry_with_nip17();
    // POSITIVE: routed to nmp.nip17.send, payload decodes + start() OK.
    registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            &decoded.action_namespace,
            &decoded.payload,
        )
        .expect("sendDm builder bytes must dispatch + validate via start_bytes");
    // LOAD-BEARING: the same bytes under a wrong namespace must fail closed.
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip17.publish_relay_list",
            &decoded.payload,
        )
        .expect_err("a SendDmPayload routed as publish_relay_list must be rejected");
    assert!(
        matches!(err, ActionRejection::Invalid(_)),
        "wrong-namespace dispatch must fail closed as Invalid, got {err:?}"
    );
}
