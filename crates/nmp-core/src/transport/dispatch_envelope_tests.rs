//! Round-trip + fail-closed trip tests for the write-command byte transport
//! (ADR-0071 / S2 #1750). Every fail-closed gate has a test that asserts the
//! NEGATIVE — the bad case is REJECTED, never silently accepted.

use super::{
    decode_dispatch_envelope, encode_dispatch_envelope, DispatchDecodeError,
    DISPATCH_ENVELOPE_FILE_IDENTIFIER, DISPATCH_ENVELOPE_SCHEMA_VERSION,
    MAX_DISPATCH_ENVELOPE_BYTES,
};

// ---- Round-trip (acceptance: encode → bytes → decode) -----------------------

#[test]
fn round_trips_through_bytes() {
    let payload = b"\x00\x01\x02opaque-flatbuffers-root\xff";
    let bytes = encode_dispatch_envelope(
        "corr-123",
        "nmp.publish",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        payload,
    );
    // The finished buffer carries our magic at offset 4.
    assert_eq!(&bytes[4..8], DISPATCH_ENVELOPE_FILE_IDENTIFIER);

    let decoded = decode_dispatch_envelope(&bytes).expect("valid envelope decodes");
    assert_eq!(decoded.correlation_id, "corr-123");
    assert_eq!(decoded.action_namespace, "nmp.publish");
    // Payload carried VERBATIM — opaque bytes preserved exactly, never inspected.
    assert_eq!(decoded.payload, payload);
}

#[test]
fn payload_stays_opaque_arbitrary_bytes() {
    // The transport must carry ANY byte sequence (not just valid FlatBuffers):
    // S3 owns the typed decode. An empty payload is also valid at this layer.
    for payload in [&b""[..], &b"\xde\xad\xbe\xef"[..], &[0u8; 4096][..]] {
        let bytes = encode_dispatch_envelope(
            "c",
            "nmp.nip25.react",
            DISPATCH_ENVELOPE_SCHEMA_VERSION,
            payload,
        );
        let decoded = decode_dispatch_envelope(&bytes).expect("decodes");
        assert_eq!(decoded.payload, payload);
    }
}

// ---- Gate: schema_version tripwire (fail CLOSED) ----------------------------

#[test]
fn schema_version_mismatch_is_rejected() {
    let bad_version = DISPATCH_ENVELOPE_SCHEMA_VERSION + 7;
    let bytes = encode_dispatch_envelope("c", "nmp.publish", bad_version, b"payload");
    let err = decode_dispatch_envelope(&bytes).expect_err("unknown version must be rejected");
    assert_eq!(
        err,
        DispatchDecodeError::SchemaVersionMismatch {
            found: bad_version,
            expected: DISPATCH_ENVELOPE_SCHEMA_VERSION,
        },
        "schema_version mismatch must fail closed, not silently accept or downcast"
    );
}

#[test]
fn schema_version_zero_is_rejected() {
    // A default/absent version (0) is NOT the recognised version; reject it.
    let bytes = encode_dispatch_envelope("c", "nmp.publish", 0, b"payload");
    assert!(matches!(
        decode_dispatch_envelope(&bytes),
        Err(DispatchDecodeError::SchemaVersionMismatch { found: 0, .. })
    ));
}

// ---- Gate: file identifier (fail CLOSED) ------------------------------------

#[test]
fn wrong_file_identifier_is_rejected() {
    // Encode a valid envelope, then corrupt the 4-byte magic at offset 4.
    let mut bytes =
        encode_dispatch_envelope("c", "nmp.publish", DISPATCH_ENVELOPE_SCHEMA_VERSION, b"p");
    bytes[4..8].copy_from_slice(b"NMPU"); // the READ-direction magic
    let err = decode_dispatch_envelope(&bytes).expect_err("wrong root magic must be rejected");
    assert_eq!(
        err,
        DispatchDecodeError::BadFileIdentifier { found: *b"NMPU" },
        "a wrong-root buffer must fail closed; the RAW discriminant is reported"
    );
}

#[test]
fn empty_buffer_is_rejected() {
    let err = decode_dispatch_envelope(&[]).expect_err("empty buffer must be rejected");
    assert!(matches!(err, DispatchDecodeError::BadFileIdentifier { .. }));
}

// ---- Gate: oversize bound (fail CLOSED) -------------------------------------

#[test]
fn oversize_buffer_is_rejected() {
    let oversize = vec![0u8; MAX_DISPATCH_ENVELOPE_BYTES + 1];
    let err = decode_dispatch_envelope(&oversize).expect_err("oversize must be rejected");
    assert_eq!(
        err,
        DispatchDecodeError::Oversize {
            len: MAX_DISPATCH_ENVELOPE_BYTES + 1,
            max: MAX_DISPATCH_ENVELOPE_BYTES,
        },
        "an oversize buffer must fail closed BEFORE any FlatBuffers traversal"
    );
}

#[test]
fn at_limit_is_not_oversize() {
    // A buffer exactly at the bound is NOT rejected by the size gate (it fails
    // the later identifier/verify gates instead — proving the bound is `>`, not
    // `>=`, so a legitimate max-size command is admitted to decoding).
    let at_limit = vec![0u8; MAX_DISPATCH_ENVELOPE_BYTES];
    assert!(!matches!(
        decode_dispatch_envelope(&at_limit),
        Err(DispatchDecodeError::Oversize { .. })
    ));
}

// ---- Gate: required routing fields (fail CLOSED) ----------------------------

#[test]
fn empty_namespace_is_rejected() {
    let bytes = encode_dispatch_envelope("c", "", DISPATCH_ENVELOPE_SCHEMA_VERSION, b"p");
    assert_eq!(
        decode_dispatch_envelope(&bytes),
        Err(DispatchDecodeError::MissingNamespace),
        "a blank routing key cannot be dispatched — fail closed"
    );
}

#[test]
fn empty_correlation_id_is_rejected() {
    let bytes = encode_dispatch_envelope("", "nmp.publish", DISPATCH_ENVELOPE_SCHEMA_VERSION, b"p");
    assert_eq!(
        decode_dispatch_envelope(&bytes),
        Err(DispatchDecodeError::MissingCorrelationId),
        "an operation with no identity cannot be dispatched — fail closed"
    );
}

// ---- Gate: malformed buffer (fail CLOSED) -----------------------------------

#[test]
fn truncated_buffer_is_rejected() {
    let bytes = encode_dispatch_envelope(
        "c",
        "nmp.publish",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        b"payload",
    );
    // Keep the magic intact but truncate the body so verification fails.
    let truncated = &bytes[..bytes.len().min(12)];
    let err = decode_dispatch_envelope(truncated).expect_err("truncated buffer must be rejected");
    // Either the magic survived and verify fails (Malformed), or truncation
    // also clipped the magic (BadFileIdentifier). Both are fail-closed rejects;
    // neither is a successful decode.
    assert!(matches!(
        err,
        DispatchDecodeError::Malformed | DispatchDecodeError::BadFileIdentifier { .. }
    ));
}
