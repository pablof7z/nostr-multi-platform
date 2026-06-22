//! Registry-level trip tests for the relay-list typed FlatBuffers payload
//! doorway (ADR-0064 / #1756).
//!
//! These exercise the same path the byte transport (`DispatchEnvelope`) drives
//! in production: `ActionRegistry::start_bytes` decodes the typed payload,
//! gating `schema_version` FAIL-CLOSED BEFORE `start()` runs. Each namespace
//! gets a negative (bad-version trip) and a positive (well-formed round-trip).
//!
//! Codec round-trip tests (positive + per-field negative) live in
//! `src/wire/action_payload_tests.rs`; these sit one level up at the registry
//! boundary.

use std::sync::Arc;

use nmp_core::__ffi_internal::ActionRegistry;
use nmp_core::substrate::{ActionContext, ActionPayload, ActionRejection, ActionRegistrar};
use nmp_router::{
    BlockRelayAction, BlockRelayInput, InMemoryBlockedRelayCache, PublishRelayListAction,
    PublishRelayListInput, UnblockRelayAction, UnblockRelayInput,
};
use nmp_router::publish_relay_list::{RelayListEntry, RelayMarker};

const PUBKEY: &str = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";

fn assert_version_trip(err: ActionRejection) {
    match err {
        ActionRejection::Invalid(msg) => assert!(
            msg.contains("schema_version mismatch"),
            "rejection must name the version trip: {msg}"
        ),
        other => panic!("expected Invalid rejection, got {other:?}"),
    }
}

// --- block_relay -------------------------------------------------------------

#[test]
fn start_bytes_rejects_wrong_schema_version_for_block_relay() {
    let cache = Arc::new(InMemoryBlockedRelayCache::new());
    let mut registry = ActionRegistry::new();
    registry.register_action(BlockRelayAction::new(cache));

    // Encode a good payload, then corrupt its schema_version slot via a hand
    // build with version 999.
    let bad = build_bad_block_payload();
    let err = registry
        .start_bytes(&mut ActionContext::default(), 1_700_000_000_000, "nmp.nip51.block_relay", &bad)
        .expect_err("a wrong schema_version must be rejected before start() (fail closed)");
    assert_version_trip(err);
}

#[test]
fn start_bytes_accepts_good_block_relay_payload() {
    let cache = Arc::new(InMemoryBlockedRelayCache::new());
    let mut registry = ActionRegistry::new();
    registry.register_action(BlockRelayAction::new(cache));

    let action = BlockRelayInput {
        url: "wss://relay.example".to_string(),
        account_pubkey: PUBKEY.to_string(),
    };
    let id = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip51.block_relay",
            &action.encode(),
        )
        .expect("well-formed block payload must be accepted");
    assert_eq!(id.len(), 32, "minted correlation_id must be 32 hex chars");
}

// --- unblock_relay -----------------------------------------------------------

#[test]
fn start_bytes_rejects_wrong_schema_version_for_unblock_relay() {
    let cache = Arc::new(InMemoryBlockedRelayCache::new());
    let mut registry = ActionRegistry::new();
    registry.register_action(UnblockRelayAction::new(cache));

    let bad = build_bad_unblock_payload();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip51.unblock_relay",
            &bad,
        )
        .expect_err("a wrong schema_version must be rejected before start() (fail closed)");
    assert_version_trip(err);
}

#[test]
fn start_bytes_accepts_good_unblock_relay_payload() {
    // `start` rejects unblocking a relay that is not currently blocked, so
    // pre-populate the cache with the relay we are about to unblock.
    let cache = Arc::new(InMemoryBlockedRelayCache::new());
    cache.upsert(PUBKEY.to_string(), vec!["wss://relay.example".to_string()]);
    let mut registry = ActionRegistry::new();
    registry.register_action(UnblockRelayAction::new(cache));

    let action = UnblockRelayInput {
        url: "wss://relay.example".to_string(),
        account_pubkey: PUBKEY.to_string(),
    };
    let id = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip51.unblock_relay",
            &action.encode(),
        )
        .expect("well-formed unblock payload must be accepted");
    assert_eq!(id.len(), 32, "minted correlation_id must be 32 hex chars");
}

// --- publish_relay_list ------------------------------------------------------

#[test]
fn start_bytes_rejects_wrong_schema_version_for_publish_relay_list() {
    let mut registry = ActionRegistry::new();
    registry.register_action(PublishRelayListAction);

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
    registry.register_action(PublishRelayListAction);

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
// Built by hand (not by `encode`) so the buffers carry an out-of-range
// `schema_version` the registry's fail-closed gate must reject before `start`.

fn build_bad_block_payload() -> Vec<u8> {
    use flatbuffers::FlatBufferBuilder;
    const IDENTIFIER: &str = "NBLK";
    const VT_SCHEMA_VERSION: flatbuffers::VOffsetT = 4;
    const VT_URL: flatbuffers::VOffsetT = 6;
    const VT_ACCOUNT_PUBKEY: flatbuffers::VOffsetT = 8;

    let mut fbb = FlatBufferBuilder::new();
    let url = fbb.create_string("wss://relay.example");
    let pubkey = fbb.create_string(PUBKEY);
    let start = fbb.start_table();
    fbb.push_slot::<u32>(VT_SCHEMA_VERSION, 999, 0);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_URL, url);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_ACCOUNT_PUBKEY, pubkey);
    let root = fbb.end_table(start);
    fbb.finish(root, Some(IDENTIFIER));
    fbb.finished_data().to_vec()
}

fn build_bad_unblock_payload() -> Vec<u8> {
    use flatbuffers::FlatBufferBuilder;
    const IDENTIFIER: &str = "NUBL";
    const VT_SCHEMA_VERSION: flatbuffers::VOffsetT = 4;
    const VT_URL: flatbuffers::VOffsetT = 6;
    const VT_ACCOUNT_PUBKEY: flatbuffers::VOffsetT = 8;

    let mut fbb = FlatBufferBuilder::new();
    let url = fbb.create_string("wss://relay.example");
    let pubkey = fbb.create_string(PUBKEY);
    let start = fbb.start_table();
    fbb.push_slot::<u32>(VT_SCHEMA_VERSION, 999, 0);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_URL, url);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_ACCOUNT_PUBKEY, pubkey);
    let root = fbb.end_table(start);
    fbb.finish(root, Some(IDENTIFIER));
    fbb.finished_data().to_vec()
}

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
