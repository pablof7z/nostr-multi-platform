//! Registry-level trip tests for the relay-list typed FlatBuffers payload
//! doorway (ADR-0071 / #1756).
//!
//! These exercise the same path the byte transport (`DispatchEnvelope`) drives
//! in production: `ActionRegistry::start_bytes` decodes the typed payload,
//! gating `schema_version` FAIL-CLOSED BEFORE `start()` runs. Each namespace
//! gets a negative (bad-version trip) and a positive (well-formed round-trip).
//!
//! Codec round-trip tests (positive + per-field negative) live in
//! `src/wire/action_payload_tests.rs`; these sit one level up at the registry
//! boundary.

use nmp_core::__ffi_internal::ActionRegistry;
use nmp_core::substrate::{ActionContext, ActionPayload, ActionRegistrar, ActionRejection};
use nmp_router::publish_relay_list::{RelayListEntry, RelayMarker};
use nmp_router::{PublishRelayListAction, PublishRelayListInput};

fn assert_version_trip(err: ActionRejection) {
    match err {
        ActionRejection::Invalid(msg) => assert!(
            msg.contains("schema_version mismatch"),
            "rejection must name the version trip: {msg}"
        ),
        other => panic!("expected Invalid rejection, got {other:?}"),
    }
}

// --- publish_relay_list ------------------------------------------------------

#[test]
fn start_bytes_rejects_wrong_schema_version_for_publish_relay_list() {
    let mut registry = ActionRegistry::new();
    let _ = registry.register_action(PublishRelayListAction);

    let bad = build_bad_publish_payload();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip65.publish_relay_list",
            &bad,
        )
        .expect_err("a wrong schema_version must be rejected before start() (fail closed)");
    assert_version_trip(err);
}

#[test]
fn start_bytes_accepts_good_publish_relay_list_payload() {
    let mut registry = ActionRegistry::new();
    let _ = registry.register_action(PublishRelayListAction);

    let action = PublishRelayListInput {
        relays: vec![RelayListEntry {
            url: "wss://relay.example".to_string(),
            marker: RelayMarker::Both,
        }],
    };
    let id = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip65.publish_relay_list",
            &action.encode(),
        )
        .expect("well-formed relay list payload must be accepted");
    assert_eq!(id.len(), 32, "minted correlation_id must be 32 hex chars");
}

// ---- helpers: build bad-version FlatBuffers buffers -------------------------
//
// Built by hand (not by `encode`) so the buffer carries an out-of-range
// `schema_version` the registry's fail-closed gate must reject before `start`.

fn build_bad_publish_payload() -> Vec<u8> {
    use flatbuffers::FlatBufferBuilder;
    const IDENTIFIER: &str = "N65P";
    const VT_SCHEMA_VERSION: flatbuffers::VOffsetT = 4;
    const VT_RELAYS: flatbuffers::VOffsetT = 6;

    let mut fbb = FlatBufferBuilder::new();
    // An empty relays vector is fine — the gate trips on version before `start`
    // ever inspects the (empty) list.
    let relays = fbb.create_vector::<flatbuffers::WIPOffset<flatbuffers::Table>>(&[]);
    let start = fbb.start_table();
    fbb.push_slot::<u32>(VT_SCHEMA_VERSION, 999, 0);
    fbb.push_slot_always::<flatbuffers::WIPOffset<_>>(VT_RELAYS, relays);
    let root = fbb.end_table(start);
    fbb.finish(root, Some(IDENTIFIER));
    fbb.finished_data().to_vec()
}
