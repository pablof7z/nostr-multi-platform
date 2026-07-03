//! Typed FlatBuffers payload codec for `nmp.wallet.select_backend`
//! ([`SelectBackendAction`]). See `super` (`wire.rs`) module docs.

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use super::select_backend_generated::nmp::wallet as sb_fb;
use super::{malformed, SCHEMA_VERSION};
use crate::action::SelectBackendAction;

impl ActionPayload for SelectBackendAction {
    const SCHEMA_ID: &'static str = "nmp.wallet.select_backend";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let backend_id = fbb.create_string(&self.backend_id);
        let payload = sb_fb::SelectBackendPayload::create(
            &mut fbb,
            &sb_fb::SelectBackendPayloadArgs {
                schema_version: SCHEMA_VERSION,
                backend_id: Some(backend_id),
            },
        );
        sb_fb::finish_select_backend_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !sb_fb::select_backend_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing NWSB file identifier"));
        }
        let root = sb_fb::root_as_select_backend_payload(bytes)
            .map_err(|e| malformed(format!("not a valid SelectBackendPayload buffer: {e}")))?;
        // Gate FIRST.
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(SelectBackendAction {
            backend_id: root.backend_id().to_string(),
        })
    }
}
