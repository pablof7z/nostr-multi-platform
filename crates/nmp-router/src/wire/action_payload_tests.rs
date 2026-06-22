//! Round-trip + fail-closed tests for the relay-list action typed payload
//! codecs (ADR-0064 / #1756). Every fail-closed gate asserts the NEGATIVE.

use super::*;
use crate::block_relay::{BlockRelayInput, UnblockRelayInput};
use crate::publish_relay_list::{PublishRelayListInput, RelayListEntry, RelayMarker};
use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

const PUBKEY: &str = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";

// --- BlockRelayInput round-trips ---------------------------------------------

#[test]
fn block_relay_round_trips() {
    let action = BlockRelayInput {
        url: "wss://relay.example".to_string(),
        account_pubkey: PUBKEY.to_string(),
    };
    let decoded = BlockRelayInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn block_relay_wrong_schema_version_is_rejected() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let url = fbb.create_string("wss://relay.example");
    let account_pubkey = fbb.create_string(PUBKEY);
    let payload = block_fb::BlockRelayPayload::create(
        &mut fbb,
        &block_fb::BlockRelayPayloadArgs {
            schema_version: 999,
            url: Some(url),
            account_pubkey: Some(account_pubkey),
        },
    );
    block_fb::finish_block_relay_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    let err = BlockRelayInput::decode(&bytes).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch { found: 999, expected: SCHEMA_VERSION }
    );
}

#[test]
fn block_relay_missing_identifier_is_malformed() {
    let err = BlockRelayInput::decode(b"not flatbuffers").expect_err("garbage rejected");
    assert!(matches!(err, ActionPayloadDecodeError::Malformed { .. }), "got {err:?}");
}

// --- UnblockRelayInput round-trips -------------------------------------------

#[test]
fn unblock_relay_round_trips() {
    let action = UnblockRelayInput {
        url: "wss://relay.example".to_string(),
        account_pubkey: PUBKEY.to_string(),
    };
    let decoded = UnblockRelayInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn unblock_relay_wrong_schema_version_is_rejected() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let url = fbb.create_string("wss://relay.example");
    let account_pubkey = fbb.create_string(PUBKEY);
    let payload = unblock_fb::UnblockRelayPayload::create(
        &mut fbb,
        &unblock_fb::UnblockRelayPayloadArgs {
            schema_version: 999,
            url: Some(url),
            account_pubkey: Some(account_pubkey),
        },
    );
    unblock_fb::finish_unblock_relay_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    let err = UnblockRelayInput::decode(&bytes).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch { found: 999, expected: SCHEMA_VERSION }
    );
}

/// A `BlockRelayPayload` (`NBLK`) must NOT decode as an `UnblockRelayPayload`
/// (`NUBL`) — distinct file identifiers keep the two namespaces apart.
#[test]
fn block_payload_does_not_decode_as_unblock() {
    let block = BlockRelayInput {
        url: "wss://relay.example".to_string(),
        account_pubkey: PUBKEY.to_string(),
    };
    let bytes = block.encode();
    let err = UnblockRelayInput::decode(&bytes).expect_err("cross-namespace decode rejected");
    assert!(matches!(err, ActionPayloadDecodeError::Malformed { .. }), "got {err:?}");
}

// --- PublishRelayListInput round-trips ---------------------------------------

#[test]
fn publish_relay_list_round_trips_all_markers() {
    let action = PublishRelayListInput {
        relays: vec![
            RelayListEntry { url: "wss://both.example".to_string(), marker: RelayMarker::Both },
            RelayListEntry { url: "wss://read.example".to_string(), marker: RelayMarker::Read },
            RelayListEntry { url: "wss://write.example".to_string(), marker: RelayMarker::Write },
            RelayListEntry {
                url: "wss://idx.example".to_string(),
                marker: RelayMarker::Indexer,
            },
        ],
    };
    let decoded = PublishRelayListInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn publish_relay_list_empty_round_trips() {
    let action = PublishRelayListInput { relays: vec![] };
    let decoded = PublishRelayListInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn publish_relay_list_wrong_schema_version_is_rejected() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let url = fbb.create_string("wss://relay.example");
    let entry = relay_list_fb::RelayListEntry::create(
        &mut fbb,
        &relay_list_fb::RelayListEntryArgs {
            url: Some(url),
            marker: relay_list_fb::RelayMarker::Both,
        },
    );
    let relays = fbb.create_vector(&[entry]);
    let payload = relay_list_fb::PublishRelayListPayload::create(
        &mut fbb,
        &relay_list_fb::PublishRelayListPayloadArgs {
            schema_version: 999,
            relays: Some(relays),
        },
    );
    relay_list_fb::finish_publish_relay_list_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    let err = PublishRelayListInput::decode(&bytes).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch { found: 999, expected: SCHEMA_VERSION }
    );
}
