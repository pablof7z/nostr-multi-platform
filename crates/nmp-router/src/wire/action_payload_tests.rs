//! Round-trip + fail-closed tests for the relay-list action typed payload
//! codecs (ADR-0064 / #1756). Every fail-closed gate asserts the NEGATIVE.

use super::*;
use crate::publish_relay_list::{PublishRelayListInput, RelayListEntry, RelayMarker};
use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

/// Decode of a `PublishRelayListPayload` whose entry carries a `RelayMarker`
/// ordinal outside [0, 3] must return `Malformed`, NOT silently map to a
/// default known marker or panic.  This is the fail-closed gate for
/// forward-compat unknown discriminants (doctrine concern, #1834 nit).
///
/// Load-bearing proof: a decode that silently coerced an unknown ordinal to
/// `RelayMarker::Both` (ordinal 0) would return `Ok(…)` here and the
/// `expect_err` assertion would fail.
#[test]
fn publish_relay_list_unknown_marker_ordinal_is_malformed() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let url = fbb.create_string("wss://relay.example");
    // Ordinal 99 is outside the known set [0 (Both), 1 (Read), 2 (Write), 3 (Indexer)].
    let entry = relay_list_fb::RelayListEntry::create(
        &mut fbb,
        &relay_list_fb::RelayListEntryArgs {
            url: Some(url),
            marker: relay_list_fb::RelayMarker(99),
        },
    );
    let relays = fbb.create_vector(&[entry]);
    let payload = relay_list_fb::PublishRelayListPayload::create(
        &mut fbb,
        &relay_list_fb::PublishRelayListPayloadArgs {
            schema_version: SCHEMA_VERSION,
            relays: Some(relays),
        },
    );
    relay_list_fb::finish_publish_relay_list_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();

    let err = PublishRelayListInput::decode(&bytes)
        .expect_err("unknown RelayMarker ordinal must be rejected fail-closed");
    assert!(
        matches!(err, ActionPayloadDecodeError::Malformed { .. }),
        "expected Malformed, got {err:?}"
    );
}

// --- PublishRelayListInput round-trips ---------------------------------------

#[test]
fn publish_relay_list_round_trips_all_markers() {
    let action = PublishRelayListInput {
        relays: vec![
            RelayListEntry {
                url: "wss://both.example".to_string(),
                marker: RelayMarker::Both,
            },
            RelayListEntry {
                url: "wss://read.example".to_string(),
                marker: RelayMarker::Read,
            },
            RelayListEntry {
                url: "wss://write.example".to_string(),
                marker: RelayMarker::Write,
            },
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
        ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 999,
            expected: SCHEMA_VERSION
        }
    );
}
