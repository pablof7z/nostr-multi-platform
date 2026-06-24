//! Round-trip + fail-closed tests for the `nmp.publish` typed payload codec
//! (ADR-0064 / S3 #1751). Every fail-closed gate asserts the NEGATIVE.

use super::*;
use crate::publish::action::{PublishAction, PublishTarget};
use crate::substrate::{ActionPayload, ActionPayloadDecodeError};
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

fn fixture_signed_event() -> SignedEvent {
    SignedEvent {
        id: "a".repeat(64),
        sig: "b".repeat(128),
        unsigned: UnsignedEvent {
            pubkey: "c".repeat(64),
            kind: 1,
            tags: vec![
                vec!["e".to_string(), "d".repeat(64)],
                vec!["p".to_string(), "f".repeat(64)],
            ],
            content: "hello \"world\" — unicode ✓ and a \\ backslash".to_string(),
            created_at: 1_700_000_000,
        },
    }
}

fn signed_publish() -> PublishAction {
    PublishAction::Publish {
        handle: "pub-1".to_string(),
        event: fixture_signed_event(),
        target: PublishTarget::Auto,
    }
}

// ---- Pre-signed Publish: signature byte-exactness (the mandatory proof) ------

#[test]
fn presigned_publish_event_is_byte_identical_through_round_trip() {
    let original = signed_publish();
    // The canonical bytes the host signs over.
    let event = match &original {
        PublishAction::Publish { event, .. } => event.clone(),
        _ => unreachable!(),
    };
    let canonical_bytes = event.to_nip01_json().into_bytes();

    // encode → bytes → decode → re-encode.
    let bytes = original.encode();
    let decoded = PublishAction::decode(&bytes).expect("valid publish payload decodes");

    // The decoded SignedEvent is field-identical (id/sig/unsigned).
    match &decoded {
        PublishAction::Publish { event: decoded_event, handle, target } => {
            assert_eq!(handle, "pub-1");
            assert_eq!(*target, PublishTarget::Auto);
            assert_eq!(*decoded_event, event, "decoded SignedEvent must equal original");
            // The id and sig are byte-exact (the whole point).
            assert_eq!(decoded_event.id, "a".repeat(64));
            assert_eq!(decoded_event.sig, "b".repeat(128));
            // And re-serializing the decoded event yields byte-identical
            // canonical bytes — so the signature stays valid.
            assert_eq!(
                decoded_event.to_nip01_json().into_bytes(),
                canonical_bytes,
                "canonical NIP-01 bytes must round-trip byte-for-byte (sig exactness)"
            );
        }
        _ => panic!("expected Publish variant"),
    }

    // The opaque canonical_event vector inside the FlatBuffers buffer is the
    // verbatim canonical bytes — re-encoding the decoded action reproduces them.
    let bytes2 = decoded.encode();
    let re_decoded = PublishAction::decode(&bytes2).expect("re-decodes");
    match re_decoded {
        PublishAction::Publish { event: e2, .. } => {
            assert_eq!(e2.to_nip01_json().into_bytes(), canonical_bytes);
        }
        _ => panic!("expected Publish variant"),
    }
}

// ---- Round-trips for the other dispatchable variants ------------------------

#[test]
fn publish_raw_round_trips() {
    let action = PublishAction::PublishRaw {
        kind: 30023,
        tags: vec![vec!["d".to_string(), "slug".to_string()], vec!["title".to_string(), "T".to_string()]],
        content: "body".to_string(),
        target: PublishTarget::Explicit {
            relays: vec!["wss://relay.one".to_string(), "wss://relay.two".to_string()],
        },
        signer_pubkey: Some("e".repeat(64)),
    };
    let decoded = PublishAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn publish_raw_auto_target_and_no_signer_round_trips() {
    let action = PublishAction::PublishRaw {
        kind: 1,
        tags: vec![],
        content: "note".to_string(),
        target: PublishTarget::Auto,
        signer_pubkey: None,
    };
    let decoded = PublishAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn publish_profile_round_trips() {
    let mut fields = serde_json::Map::new();
    fields.insert("name".to_string(), serde_json::Value::String("Alice".to_string()));
    fields.insert("about".to_string(), serde_json::Value::String("nostr dev".to_string()));
    let action = PublishAction::PublishProfile { fields: fields.clone() };
    let decoded = PublishAction::decode(&action.encode()).expect("decodes");
    match decoded {
        PublishAction::PublishProfile { fields: got } => assert_eq!(got, fields),
        _ => panic!("expected PublishProfile"),
    }
}

// ---- Fail CLOSED: schema_version tripwire -----------------------------------

#[test]
fn wrong_schema_version_is_rejected_before_decode() {
    // Hand-build a buffer whose schema_version is bogus; the decode must trip
    // the version gate and NOT inspect the body.
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let handle = fbb.create_string("h");
    let canonical = fbb.create_vector(b"{}");
    let target = fb::PublishTarget::create(
        &mut fbb,
        &fb::PublishTargetArgs { explicit: false, relays: None },
    );
    let signed = fb::PublishSigned::create(
        &mut fbb,
        &fb::PublishSignedArgs {
            handle: Some(handle),
            canonical_event: Some(canonical),
            target: Some(target),
        },
    );
    let payload = fb::PublishPayload::create(
        &mut fbb,
        &fb::PublishPayloadArgs {
            schema_version: 999,
            body_type: fb::PublishPayloadBody::PublishSigned,
            body: Some(signed.as_union_value()),
        },
    );
    fb::finish_publish_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();

    let err = PublishAction::decode(&bytes).expect_err("bad schema_version must be rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch { found: 999, expected: SCHEMA_VERSION }
    );
}

#[test]
fn malformed_buffer_is_rejected() {
    assert!(matches!(
        PublishAction::decode(b"not a flatbuffer"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        PublishAction::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}

#[test]
fn schema_constants_are_stable() {
    assert_eq!(SCHEMA_ID, "nmp.publish");
    assert_eq!(SCHEMA_VERSION, 1);
    assert_eq!(FILE_IDENTIFIER, b"NPUB");
}
