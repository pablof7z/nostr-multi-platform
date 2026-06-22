//! Typed FlatBuffers payload codec for the blossom upload action
//! (ADR-0064 / S9 #1747): `nmp.blossom.upload` ([`UploadInput`]).
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
#[path = "wire/generated/upload_generated.rs"]
pub mod upload_generated;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};
use upload_generated::nmp::blossom as upload_fb;

use crate::action::UploadInput;

/// Wire schema version for the blossom upload payload. Bump on any breaking
/// change to `upload.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

// --- UploadInput -------------------------------------------------------------

impl ActionPayload for UploadInput {
    const SCHEMA_ID: &'static str = "nmp.blossom.upload";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let file_path = fbb.create_string(&self.file_path);
        let content_type = self.content_type.as_ref().map(|s| fbb.create_string(s));
        let servers = if self.servers.is_empty() {
            None
        } else {
            let server_offsets: Vec<_> =
                self.servers.iter().map(|s| fbb.create_string(s)).collect();
            Some(fbb.create_vector(&server_offsets))
        };
        let signer_pubkey = self.signer_pubkey.as_ref().map(|s| fbb.create_string(s));
        let payload = upload_fb::UploadPayload::create(
            &mut fbb,
            &upload_fb::UploadPayloadArgs {
                schema_version: SCHEMA_VERSION,
                file_path: Some(file_path),
                content_type,
                servers,
                signer_pubkey,
            },
        );
        upload_fb::finish_upload_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !upload_fb::upload_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing BUPL file identifier"));
        }
        let root = upload_fb::root_as_upload_payload(bytes)
            .map_err(|e| malformed(format!("not a valid UploadPayload buffer: {e}")))?;
        // Gate FIRST.
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        let servers = root
            .servers()
            .map(|v| v.iter().map(str::to_string).collect())
            .unwrap_or_default();
        Ok(UploadInput {
            file_path: root.file_path().to_string(),
            content_type: root.content_type().map(str::to_string),
            servers,
            signer_pubkey: root.signer_pubkey().map(str::to_string),
        })
    }
}

#[cfg(test)]
#[path = "wire/tests.rs"]
mod tests;
