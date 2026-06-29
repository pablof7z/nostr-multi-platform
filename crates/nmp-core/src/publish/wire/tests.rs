//! Round-trip + fail-closed tests for the `nmp.publish` typed payload codec
//! (ADR-0064 / S3 #1751). Every fail-closed gate asserts the NEGATIVE.

use super::*;
use crate::publish::action::{
    PublishAction, PublishRouteClass, PublishSigner, PublishSignerProvenance, PublishTarget,
};
use crate::substrate::{ActionPayload, ActionPayloadDecodeError};

#[test]
fn presigned_publish_is_not_a_dispatchable_wire_payload() {
    let action = PublishAction::Publish {
        handle: "pub-1".to_string(),
        event: nmp_signer_iface::SignedEvent {
            id: "a".repeat(64),
            sig: "b".repeat(128),
            unsigned: nmp_signer_iface::UnsignedEvent {
                pubkey: "c".repeat(64),
                kind: 1,
                tags: vec![],
                content: "verbatim".to_string(),
                created_at: 1_700_000_000,
            },
        },
        target: PublishTarget::explicit(
            vec!["wss://relay.example".to_string()],
            PublishRouteClass::ImportedOrPresigned,
        ),
    };

    let err = PublishAction::decode(&action.encode())
        .expect_err("pre-signed Publish must not serialize as app dispatch");
    assert!(
        matches!(err, ActionPayloadDecodeError::Malformed { .. }),
        "pre-signed publish should fail closed at decode; got {err:?}"
    );
}

// ---- Round-trips for the other dispatchable variants ------------------------

#[test]
fn publish_raw_round_trips() {
    let action = PublishAction::PublishRaw {
        kind: 30023,
        tags: vec![
            vec!["d".to_string(), "slug".to_string()],
            vec!["title".to_string(), "T".to_string()],
        ],
        content: "body".to_string(),
        target: PublishTarget::explicit(
            vec!["wss://relay.one".to_string(), "wss://relay.two".to_string()],
            PublishRouteClass::Diagnostic,
        ),
        signer: PublishSigner::registered("e".repeat(64), PublishSignerProvenance::AppManaged),
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
        signer: Default::default(),
    };
    let decoded = PublishAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

fn publish_raw_payload_with_route_class(route_class: Option<&str>) -> Vec<u8> {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let relay = fbb.create_string("wss://relay.example");
    let relays = fbb.create_vector(&[relay]);
    let route_class = route_class.map(|token| fbb.create_string(token));
    let target = fb::PublishTarget::create(
        &mut fbb,
        &fb::PublishTargetArgs {
            explicit: true,
            relays: Some(relays),
            route_class,
        },
    );
    let content = fbb.create_string("body");
    let raw = fb::PublishRaw::create(
        &mut fbb,
        &fb::PublishRawArgs {
            kind: 1,
            tags: None,
            content: Some(content),
            target: Some(target),
            signer: None,
        },
    );
    let payload = fb::PublishPayload::create(
        &mut fbb,
        &fb::PublishPayloadArgs {
            schema_version: SCHEMA_VERSION,
            body_type: fb::PublishPayloadBody::PublishRaw,
            body: Some(raw.as_union_value()),
        },
    );
    fb::finish_publish_payload_buffer(&mut fbb, payload);
    fbb.finished_data().to_vec()
}

#[test]
fn explicit_target_without_route_class_is_rejected() {
    let err = PublishAction::decode(&publish_raw_payload_with_route_class(None))
        .expect_err("explicit target without route_class must be malformed");
    assert!(
        matches!(&err, ActionPayloadDecodeError::Malformed { reason } if reason.contains("route_class")),
        "error must mention route_class; got {err:?}"
    );
}

#[test]
fn explicit_target_with_unknown_route_class_is_rejected() {
    let err = PublishAction::decode(&publish_raw_payload_with_route_class(Some("mystery")))
        .expect_err("unknown explicit route_class must be malformed");
    assert!(
        matches!(&err, ActionPayloadDecodeError::Malformed { reason } if reason.contains("mystery")),
        "error must mention the unknown route_class; got {err:?}"
    );
}

#[test]
fn publish_profile_round_trips() {
    let mut fields = serde_json::Map::new();
    fields.insert(
        "name".to_string(),
        serde_json::Value::String("Alice".to_string()),
    );
    fields.insert(
        "about".to_string(),
        serde_json::Value::String("nostr dev".to_string()),
    );
    let action = PublishAction::PublishProfile {
        fields: fields.clone(),
    };
    let decoded = PublishAction::decode(&action.encode()).expect("decodes");
    match decoded {
        PublishAction::PublishProfile { fields: got } => assert_eq!(got, fields),
        _ => panic!("expected PublishProfile"),
    }
}

#[test]
fn publish_reply_round_trips() {
    let action = PublishAction::PublishReply {
        content: "reply body".to_string(),
        reply_to_event_id: "d".repeat(64),
        target: PublishTarget::explicit(
            vec!["wss://relay.example".to_string()],
            PublishRouteClass::GroupHostPin,
        ),
        signer: PublishSigner::registered("e".repeat(64), PublishSignerProvenance::AppManaged),
    };
    let decoded = PublishAction::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

// ---- Fail CLOSED: schema_version tripwire -----------------------------------

#[test]
fn wrong_schema_version_is_rejected_before_decode() {
    // Hand-build a buffer whose schema_version is bogus; the decode must trip
    // the version gate and NOT inspect the body.
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let content = fbb.create_string("x");
    let target = fb::PublishTarget::create(
        &mut fbb,
        &fb::PublishTargetArgs {
            explicit: false,
            relays: None,
            route_class: None,
        },
    );
    let raw = fb::PublishRaw::create(
        &mut fbb,
        &fb::PublishRawArgs {
            kind: 1,
            tags: None,
            content: Some(content),
            target: Some(target),
            signer: None,
        },
    );
    let payload = fb::PublishPayload::create(
        &mut fbb,
        &fb::PublishPayloadArgs {
            schema_version: 999,
            body_type: fb::PublishPayloadBody::PublishRaw,
            body: Some(raw.as_union_value()),
        },
    );
    fb::finish_publish_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();

    let err = PublishAction::decode(&bytes).expect_err("bad schema_version must be rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 999,
            expected: SCHEMA_VERSION
        }
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
    assert_eq!(SCHEMA_VERSION, 4);
    assert_eq!(FILE_IDENTIFIER, b"NPUB");
}
