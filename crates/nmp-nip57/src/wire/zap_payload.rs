//! Typed FlatBuffers payload codec for the `nmp.nip57.zap` action
//! (ADR-0064 / S9 #1747).
//!
//! This is the WRITE-direction typed payload carried as the OPAQUE
//! `DispatchEnvelope.payload`. The registry adapter decodes it through
//! [`ActionPayload::decode`] here — the single typed-decode site — running the
//! fail-closed `schema_version` gate BEFORE `start()`.
//!
//! Honours D6: decode returns a data-shaped [`ActionPayloadDecodeError`] on any
//! malformed input; no panics on the decode path.

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "generated/zap_generated.rs"]
pub mod zap_generated;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};
use zap_generated::nmp::nip_57 as fb;

use crate::action::ZapInput;

/// Wire schema version for the nip57 zap action payload. Bump on any breaking
/// change to `zap.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed { reason: reason.into() }
}

impl ActionPayload for ZapInput {
    const SCHEMA_ID: &'static str = "nmp.nip57.zap";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();

        let recipient_pubkey = fbb.create_string(&self.recipient_pubkey);
        // Optional fields: write the string when present, omit the field when None.
        // Presence is preserved on decode; `start()` is responsible for domain
        // validation (e.g. rejecting an empty lnurl).
        let lnurl = self
            .lnurl
            .as_deref()
            .map(|s| fbb.create_string(s));
        let relay_offsets: Vec<_> = self
            .relays
            .iter()
            .map(|r| fbb.create_string(r))
            .collect();
        let relays = if relay_offsets.is_empty() {
            None
        } else {
            Some(fbb.create_vector(&relay_offsets))
        };
        let target_event_id = self
            .target_event_id
            .as_deref()
            .map(|s| fbb.create_string(s));
        let comment = self.comment.as_deref().map(|s| fbb.create_string(s));

        let payload = fb::ZapPayload::create(
            &mut fbb,
            &fb::ZapPayloadArgs {
                schema_version: SCHEMA_VERSION,
                recipient_pubkey: Some(recipient_pubkey),
                amount_msats: self.amount_msats,
                lnurl,
                relays,
                target_event_id,
                comment,
            },
        );
        fb::finish_zap_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !fb::zap_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N57Z file identifier"));
        }
        let root = fb::root_as_zap_payload(bytes)
            .map_err(|e| malformed(format!("not a valid ZapPayload buffer: {e}")))?;

        // Gate schema_version FIRST (fail-closed: reject before start() runs).
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }

        // `recipient_pubkey` is `required` in the schema — the verifier guarantees
        // it is present; we still surface a Malformed rather than panic.
        let recipient_pubkey = root.recipient_pubkey().to_string();

        // Optional string fields: preserve FlatBuffers field presence verbatim.
        // `None` when the field is absent (not written); `Some(s)` — including
        // `Some("")` — when it IS present. Preserving presence lets `start()`
        // apply the domain validation (e.g. rejecting an explicitly-empty `lnurl`)
        // without the decode layer silently masking invalid inputs.
        let lnurl = root.lnurl().map(str::to_string);
        let relays: Vec<String> = root
            .relays()
            .map(|v| v.iter().map(str::to_string).collect())
            .unwrap_or_default();
        let target_event_id = root.target_event_id().map(str::to_string);
        let comment = root.comment().map(str::to_string);

        Ok(ZapInput {
            recipient_pubkey,
            amount_msats: root.amount_msats(),
            lnurl,
            relays,
            target_event_id,
            comment,
        })
    }
}

#[cfg(test)]
#[path = "zap_payload_tests.rs"]
mod tests;
