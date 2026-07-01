//! Registry-level tests for the NIP-51 blocked-relay typed action payloads.

use std::sync::Arc;

use nmp_core::__ffi_internal::ActionRegistry;
use nmp_core::substrate::{ActionContext, ActionPayload, ActionRegistrar, ActionRejection};
use nmp_nip51::{
    BlockRelayAction, BlockRelayInput, InMemoryBlockedRelayCache, UnblockRelayAction,
    UnblockRelayInput,
};

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

#[test]
fn start_bytes_rejects_wrong_schema_version_for_block_relay() {
    let cache = Arc::new(InMemoryBlockedRelayCache::new());
    let mut registry = ActionRegistry::new();
    let _ = registry.register_action(BlockRelayAction::new(cache));

    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip51.block_relay",
            &build_bad_block_payload(),
        )
        .expect_err("a wrong schema_version must be rejected before start()");
    assert_version_trip(err);
}

#[test]
fn start_bytes_accepts_good_block_relay_payload() {
    let cache = Arc::new(InMemoryBlockedRelayCache::new());
    let mut registry = ActionRegistry::new();
    let _ = registry.register_action(BlockRelayAction::new(cache));

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

#[test]
fn start_bytes_rejects_wrong_schema_version_for_unblock_relay() {
    let cache = Arc::new(InMemoryBlockedRelayCache::new());
    let mut registry = ActionRegistry::new();
    let _ = registry.register_action(UnblockRelayAction::new(cache));

    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip51.unblock_relay",
            &build_bad_unblock_payload(),
        )
        .expect_err("a wrong schema_version must be rejected before start()");
    assert_version_trip(err);
}

#[test]
fn start_bytes_accepts_good_unblock_relay_payload() {
    let cache = Arc::new(InMemoryBlockedRelayCache::new());
    cache.upsert(PUBKEY.to_string(), vec!["wss://relay.example".to_string()]);
    let mut registry = ActionRegistry::new();
    let _ = registry.register_action(UnblockRelayAction::new(cache));

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

fn build_bad_block_payload() -> Vec<u8> {
    build_bad_url_pubkey_payload("NBLK")
}

fn build_bad_unblock_payload() -> Vec<u8> {
    build_bad_url_pubkey_payload("NUBL")
}

fn build_bad_url_pubkey_payload(identifier: &str) -> Vec<u8> {
    use flatbuffers::FlatBufferBuilder;
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
    fbb.finish(root, Some(identifier));
    fbb.finished_data().to_vec()
}
