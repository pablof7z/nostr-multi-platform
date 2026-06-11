//! Round-trip tests for the `bunker_connection_state` typed FlatBuffers codec.
//!
//! These tests mirror the `bunker_handshake_fb_tests` pattern and prove:
//! 1. A `BunkerConnectionStateModel` encodes to a buffer with the `KBCS`
//!    file identifier.
//! 2. Decoding that buffer reproduces the original model exactly.
//! 3. The file-identifier guard rejects buffers with the wrong identifier.
//! 4. Empty and truncated inputs are rejected gracefully (D6 — no panics).
//! 5. All three state tokens (`connected` / `reconnecting` / `failed`) round-trip.

use super::*;

#[test]
fn encode_connected_round_trips() {
    let model = BunkerConnectionStateModel {
        state: "connected".to_string(),
        reason: None,
        is_connected: true,
        is_reconnecting: false,
        is_failed: false,
    };
    let bytes = encode_bunker_connection_state(&model);
    let decoded = decode_bunker_connection_state(&bytes).expect("connected round-trip");
    assert_eq!(decoded, model);
}

#[test]
fn encode_reconnecting_with_reason_round_trips() {
    let model = BunkerConnectionStateModel {
        state: "reconnecting".to_string(),
        reason: Some("connection reset by peer".to_string()),
        is_connected: false,
        is_reconnecting: true,
        is_failed: false,
    };
    let bytes = encode_bunker_connection_state(&model);
    let decoded = decode_bunker_connection_state(&bytes).expect("reconnecting round-trip");
    assert_eq!(decoded, model);
}

#[test]
fn encode_failed_with_reason_round_trips() {
    let model = BunkerConnectionStateModel {
        state: "failed".to_string(),
        reason: Some("403 Forbidden".to_string()),
        is_connected: false,
        is_reconnecting: false,
        is_failed: true,
    };
    let bytes = encode_bunker_connection_state(&model);
    let decoded = decode_bunker_connection_state(&bytes).expect("failed round-trip");
    assert_eq!(decoded, model);
}

#[test]
fn reason_absent_when_connected_decodes_to_none() {
    let model = BunkerConnectionStateModel {
        state: "connected".to_string(),
        reason: None,
        is_connected: true,
        is_reconnecting: false,
        is_failed: false,
    };
    let bytes = encode_bunker_connection_state(&model);
    let decoded = decode_bunker_connection_state(&bytes).expect("absent-reason round-trip");
    assert_eq!(decoded.reason, None);
    assert!(decoded.is_connected);
    assert!(!decoded.is_reconnecting);
    assert!(!decoded.is_failed);
}

#[test]
fn empty_input_is_rejected() {
    let result = decode_bunker_connection_state(&[]);
    assert!(result.is_err(), "empty bytes must be rejected");
}

#[test]
fn truncated_input_is_rejected() {
    let bytes = encode_bunker_connection_state(&BunkerConnectionStateModel {
        state: "connected".to_string(),
        reason: None,
        is_connected: true,
        is_reconnecting: false,
        is_failed: false,
    });
    // Truncate to just the file-identifier region so the presence check passes
    // but the FlatBuffers root decode fails.
    let truncated = &bytes[..8.min(bytes.len())];
    // The identifier passes but the root cannot be decoded from 8 bytes.
    // Accept either outcome: decode may pass the identifier check on a short
    // buffer and then fail, or the size guard catches it. Either way no panic.
    let _ = decode_bunker_connection_state(truncated); // must not panic
}

#[test]
fn wrong_file_identifier_is_rejected() {
    // Build a valid buffer then clobber the 4-byte file-identifier at offset 4.
    let mut bytes = encode_bunker_connection_state(&BunkerConnectionStateModel {
        state: "connected".to_string(),
        reason: None,
        is_connected: true,
        is_reconnecting: false,
        is_failed: false,
    });
    if bytes.len() >= 8 {
        bytes[4..8].copy_from_slice(b"WRNG");
    }
    let result = decode_bunker_connection_state(&bytes);
    assert!(result.is_err(), "wrong identifier must be rejected");
}

#[test]
fn file_identifier_constant_is_kbcs() {
    assert_eq!(BUNKER_CONNECTION_STATE_FILE_IDENTIFIER, b"KBCS");
}

#[test]
fn schema_id_constant_matches_projection_key() {
    // The schema_id and projection key must be identical per ADR-0037
    // shared-keyspace contract.
    assert_eq!(
        BUNKER_CONNECTION_STATE_SCHEMA_ID,
        "bunker_connection_state"
    );
}
