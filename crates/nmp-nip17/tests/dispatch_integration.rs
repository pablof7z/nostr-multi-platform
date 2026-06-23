//! Registry-level trip tests for the nip17 typed FlatBuffers payload doorway
//! (ADR-0064 / S9 #1747).
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

/// ADR-0064 / S9 (#1747) — `nmp.nip17.send` with a bad `schema_version` MUST
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

/// ADR-0064 / S9 (#1747) — `nmp.nip17.publish_relay_list` with a bad
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
        .start_bytes(&mut ActionContext::default(), 1_700_000_000_000, "nmp.nip17.send", &payload)
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
