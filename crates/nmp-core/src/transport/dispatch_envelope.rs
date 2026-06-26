//! ADR-0064 / S2 (#1750) — the open write-command byte transport.
//!
//! One generic *byte* doorway carries a [`DispatchEnvelope`] across both
//! boundaries (native FFI `ptr,len` and the wasm `DispatchBytes { bytes }`).
//! This module is the **inbound decode path**: it takes the raw bytes, decodes
//! the envelope, runs the **fail-closed** gates (file-identifier, schema-version
//! tripwire, oversize bound, namespace presence), and returns the routable
//! [`DecodedDispatch`] — `(correlation_id, action_namespace, payload)` — for the
//! one `ActionModule`-registry doorway to route on.
//!
//! ## Opaque payload (do NOT peek)
//!
//! `payload` stays an opaque byte slice. The per-crate typed payload decode is
//! the `ActionModule`'s job (S3 / #1751), not the transport's. This module
//! NEVER interprets `payload`; it only bounds its size and carries it verbatim.
//!
//! ## Fail CLOSED
//!
//! Unknown `schema_version`, a wrong/absent file identifier, an oversize buffer,
//! or a missing `action_namespace`/`payload` are **rejected** (data-shaped
//! [`DispatchDecodeError`], D6 — no panic crosses the boundary). The decoder
//! NEVER guesses: a tripwire mismatch is a reject, not a multi-version decode.

use crate::transport::write_wire::DispatchEnvelope;

/// The single recognised envelope schema version (ADR-0064 §1 tripwire). An
/// envelope carrying any other value is rejected — this is a fail-closed
/// tripwire, not a version-negotiation field. Bumping it is a deliberate,
/// lockstep change across every host builder + the registry decode.
pub const DISPATCH_ENVELOPE_SCHEMA_VERSION: u32 = 1;

/// The write envelope's FlatBuffers file identifier (`root_type DispatchEnvelope`
/// in `dispatch_envelope.fbs`). Distinct from `UpdateFrame`'s `NMPU` so the byte
/// doorway fails closed on a wrong-root buffer. The `&str` form is the canonical
/// constant; [`DISPATCH_ENVELOPE_FILE_IDENTIFIER`] is its byte view for the
/// raw-offset compare.
pub const DISPATCH_ENVELOPE_FILE_IDENTIFIER_STR: &str = "NMPD";

/// Byte view of [`DISPATCH_ENVELOPE_FILE_IDENTIFIER_STR`] for the raw 4-byte
/// compare at buffer offset 4 (before any FlatBuffers traversal).
pub const DISPATCH_ENVELOPE_FILE_IDENTIFIER: &[u8; 4] = b"NMPD";

/// Hard upper bound on an inbound write envelope (bytes). A single command
/// payload (a note, a reaction, a follow) is small; anything larger is a
/// malformed or hostile buffer and is rejected before any FlatBuffers traversal.
/// 1 MiB is generous for a Nostr command yet bounds the worst case.
pub const MAX_DISPATCH_ENVELOPE_BYTES: usize = 1024 * 1024;

/// A decoded, gate-passed write envelope. `payload` is **opaque** — the bytes
/// are carried verbatim to the namespace-matched `ActionModule`; the transport
/// never interprets them (S3 owns the typed decode).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedDispatch {
    /// Host-supplied operation identity (ADR-0064 §4).
    pub correlation_id: String,
    /// Open-registry routing key — never hand-written by app code.
    pub action_namespace: String,
    /// Opaque per-crate FlatBuffers root. NOT interpreted here.
    pub payload: Vec<u8>,
}

/// Fail-closed rejection reasons. Errors are **data** (D6); none of these is a
/// panic or a `Result` that crosses the FFI/worker boundary as an exception.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DispatchDecodeError {
    /// The buffer exceeds [`MAX_DISPATCH_ENVELOPE_BYTES`] (or is too short to
    /// even carry a file identifier). Rejected before traversal.
    Oversize { len: usize, max: usize },
    /// The buffer is not a `DispatchEnvelope` root (missing/wrong file
    /// identifier or a malformed FlatBuffers table). The RAW discriminant —
    /// the bytes present where the identifier should be — is preserved.
    BadFileIdentifier { found: [u8; 4] },
    /// FlatBuffers verification failed (truncated/corrupt buffer).
    Malformed,
    /// `schema_version` is not [`DISPATCH_ENVELOPE_SCHEMA_VERSION`]. Fail-closed
    /// tripwire: the RAW version is reported, the envelope is NOT decoded.
    SchemaVersionMismatch { found: u32, expected: u32 },
    /// `action_namespace` is absent or empty — the transport cannot route it.
    MissingNamespace,
    /// `correlation_id` is absent or empty — the operation has no identity.
    MissingCorrelationId,
}

impl core::fmt::Display for DispatchDecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Oversize { len, max } => {
                write!(f, "dispatch envelope oversize: {len} bytes (max {max})")
            }
            Self::BadFileIdentifier { found } => write!(
                f,
                "dispatch envelope bad file identifier: {:?} (expected {:?})",
                found, DISPATCH_ENVELOPE_FILE_IDENTIFIER
            ),
            Self::Malformed => write!(f, "dispatch envelope malformed (verification failed)"),
            Self::SchemaVersionMismatch { found, expected } => write!(
                f,
                "dispatch envelope schema_version mismatch: {found} (expected {expected})"
            ),
            Self::MissingNamespace => write!(f, "dispatch envelope missing action_namespace"),
            Self::MissingCorrelationId => write!(f, "dispatch envelope missing correlation_id"),
        }
    }
}

/// Decode raw inbound bytes into a [`DecodedDispatch`], running every
/// fail-closed gate in order. The order is load-bearing: size → identifier →
/// verification → version tripwire → required fields. Each gate REJECTS on the
/// bad case; none falls through to a best-effort decode.
pub fn decode_dispatch_envelope(bytes: &[u8]) -> Result<DecodedDispatch, DispatchDecodeError> {
    // Gate 1 — oversize bound. Reject before any FlatBuffers traversal so a
    // hostile length can never drive an allocation or a deep verify.
    if bytes.len() > MAX_DISPATCH_ENVELOPE_BYTES {
        return Err(DispatchDecodeError::Oversize {
            len: bytes.len(),
            max: MAX_DISPATCH_ENVELOPE_BYTES,
        });
    }
    // A valid FlatBuffers root with a file identifier is at least 8 bytes
    // (root offset + the 4-byte identifier at offset 4). Anything shorter cannot
    // carry our magic — surface the RAW discriminant for the fail-closed log.
    if bytes.len() < 8 {
        let mut found = [0u8; 4];
        let n = bytes.len().min(4);
        found[..n].copy_from_slice(&bytes[..n]);
        return Err(DispatchDecodeError::BadFileIdentifier { found });
    }

    // Gate 2 — file identifier. Read the RAW 4 bytes at offset 4 and compare to
    // our magic before trusting the buffer is our root.
    let found = [bytes[4], bytes[5], bytes[6], bytes[7]];
    if &found != DISPATCH_ENVELOPE_FILE_IDENTIFIER {
        return Err(DispatchDecodeError::BadFileIdentifier { found });
    }

    // Gate 3 — FlatBuffers verification. `root_with_opts` runs the size-prefixed
    // / bounds verifier; a truncated or corrupt buffer is rejected as data, not
    // a panic.
    let envelope =
        flatbuffers::root::<DispatchEnvelope>(bytes).map_err(|_| DispatchDecodeError::Malformed)?;

    // Gate 4 — schema_version tripwire. Read the RAW value and reject any
    // version we do not recognise. We do NOT attempt to decode an unknown
    // version — fail closed (ADR-0064 §1).
    let found_version = envelope.schema_version();
    if found_version != DISPATCH_ENVELOPE_SCHEMA_VERSION {
        return Err(DispatchDecodeError::SchemaVersionMismatch {
            found: found_version,
            expected: DISPATCH_ENVELOPE_SCHEMA_VERSION,
        });
    }

    // Gate 5 — required routing fields. `correlation_id`/`action_namespace`/
    // `payload` are `(required)` in the schema, so the verifier already proved
    // presence; we additionally reject EMPTY namespace/correlation_id (a present
    // but blank routing key cannot be dispatched).
    let action_namespace = envelope.action_namespace();
    if action_namespace.is_empty() {
        return Err(DispatchDecodeError::MissingNamespace);
    }
    let correlation_id = envelope.correlation_id();
    if correlation_id.is_empty() {
        return Err(DispatchDecodeError::MissingCorrelationId);
    }

    // Payload stays OPAQUE: copy the bytes out verbatim, never inspected here.
    let payload = envelope.payload().bytes().to_vec();

    Ok(DecodedDispatch {
        correlation_id: correlation_id.to_string(),
        action_namespace: action_namespace.to_string(),
        payload,
    })
}

/// Encode a [`DispatchEnvelope`] to finished, file-identified bytes. The
/// production app-facing path is the **generated typed builders** (ADR-0064 §3);
/// this constructor is the kernel-side primitive they (and round-trip tests)
/// build on. `payload` is carried verbatim — the caller owns its typed encoding.
#[must_use]
pub fn encode_dispatch_envelope(
    correlation_id: &str,
    action_namespace: &str,
    schema_version: u32,
    payload: &[u8],
) -> Vec<u8> {
    let mut builder = flatbuffers::FlatBufferBuilder::new();
    let correlation = builder.create_string(correlation_id);
    let namespace = builder.create_string(action_namespace);
    let payload_vec = builder.create_vector(payload);
    let envelope = DispatchEnvelope::create(
        &mut builder,
        &crate::transport::write_wire::DispatchEnvelopeArgs {
            correlation_id: Some(correlation),
            action_namespace: Some(namespace),
            schema_version,
            payload: Some(payload_vec),
        },
    );
    builder.finish(envelope, Some(DISPATCH_ENVELOPE_FILE_IDENTIFIER_STR));
    builder.finished_data().to_vec()
}

#[cfg(test)]
#[path = "dispatch_envelope_tests.rs"]
mod tests;
