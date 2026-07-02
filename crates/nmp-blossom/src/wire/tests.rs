//! Round-trip + fail-closed tests for the blossom upload typed payload codec
//! (ADR-0071 / S9 #1747). Every fail-closed gate asserts the NEGATIVE.

use super::*;
use crate::action::UploadInput;
use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

fn well_formed() -> UploadInput {
    UploadInput {
        file_path: "/tmp/avatar.png".to_string(),
        content_type: Some("image/png".to_string()),
        servers: vec!["https://blossom.example".to_string()],
        signer_pubkey: None,
    }
}

#[test]
fn upload_round_trips_with_all_optional_fields() {
    let action = UploadInput {
        file_path: "/var/mobile/docs/podcast.mp3".to_string(),
        content_type: Some("audio/mpeg".to_string()),
        servers: vec![
            "https://blossom.primal.net".to_string(),
            "https://blossom.band".to_string(),
        ],
        signer_pubkey: Some("deadbeef".repeat(8)),
    };
    let decoded = UploadInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn upload_round_trips_with_minimal_fields() {
    let action = UploadInput {
        file_path: "/tmp/avatar.png".to_string(),
        content_type: None,
        servers: vec![],
        signer_pubkey: None,
    };
    let decoded = UploadInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
    assert!(decoded.content_type.is_none());
    assert!(decoded.servers.is_empty());
    assert!(decoded.signer_pubkey.is_none());
}

#[test]
fn upload_wrong_schema_version_is_rejected() {
    // Hand-build an UploadPayload with a bogus schema_version.
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let file_path = fbb.create_string("/tmp/foo.png");
    let payload = upload_fb::UploadPayload::create(
        &mut fbb,
        &upload_fb::UploadPayloadArgs {
            schema_version: 999,
            file_path: Some(file_path),
            content_type: None,
            servers: None,
            signer_pubkey: None,
        },
    );
    upload_fb::finish_upload_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    let err = UploadInput::decode(&bytes).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 999,
            expected: SCHEMA_VERSION
        }
    );
}

#[test]
fn malformed_buffers_are_rejected() {
    assert!(matches!(
        UploadInput::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        UploadInput::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}

#[test]
fn wrong_identifier_is_rejected() {
    // Encode as a well-formed payload then corrupt the identifier bytes.
    let mut bytes = well_formed().encode();
    // The file identifier sits at bytes[4..8].
    bytes[4] = b'X';
    bytes[5] = b'X';
    bytes[6] = b'X';
    bytes[7] = b'X';
    assert!(matches!(
        UploadInput::decode(&bytes),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}
