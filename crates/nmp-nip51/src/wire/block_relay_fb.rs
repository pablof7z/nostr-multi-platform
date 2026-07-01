//! Typed FlatBuffers payload codec for `nmp.nip51.block_relay`.

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
#[path = "generated/block_relay_generated.rs"]
pub mod generated;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use generated::nmp::nip_51 as fb;

use crate::block_relay::BlockRelayInput;

pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

impl ActionPayload for BlockRelayInput {
    const SCHEMA_ID: &'static str = "nmp.nip51.block_relay";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let url = fbb.create_string(&self.url);
        let account_pubkey = fbb.create_string(&self.account_pubkey);
        let payload = fb::BlockRelayPayload::create(
            &mut fbb,
            &fb::BlockRelayPayloadArgs {
                schema_version: SCHEMA_VERSION,
                url: Some(url),
                account_pubkey: Some(account_pubkey),
            },
        );
        fb::finish_block_relay_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !fb::block_relay_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing NBLK file identifier"));
        }
        let root = fb::root_as_block_relay_payload(bytes)
            .map_err(|e| malformed(format!("not a valid BlockRelayPayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(BlockRelayInput {
            url: root.url().to_string(),
            account_pubkey: root.account_pubkey().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUBKEY: &str = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";

    #[test]
    fn block_relay_round_trips() {
        let action = BlockRelayInput {
            url: "wss://relay.example".to_string(),
            account_pubkey: PUBKEY.to_string(),
        };
        let decoded = BlockRelayInput::decode(&action.encode()).expect("decodes");
        assert_eq!(decoded, action);
    }

    #[test]
    fn block_relay_wrong_schema_version_is_rejected() {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let url = fbb.create_string("wss://relay.example");
        let account_pubkey = fbb.create_string(PUBKEY);
        let payload = fb::BlockRelayPayload::create(
            &mut fbb,
            &fb::BlockRelayPayloadArgs {
                schema_version: 999,
                url: Some(url),
                account_pubkey: Some(account_pubkey),
            },
        );
        fb::finish_block_relay_payload_buffer(&mut fbb, payload);
        let bytes = fbb.finished_data().to_vec();
        let err = BlockRelayInput::decode(&bytes).expect_err("bad version rejected");
        assert_eq!(
            err,
            ActionPayloadDecodeError::SchemaVersionMismatch {
                found: 999,
                expected: SCHEMA_VERSION
            }
        );
    }

    #[test]
    fn block_relay_missing_identifier_is_malformed() {
        let err = BlockRelayInput::decode(b"not flatbuffers").expect_err("garbage rejected");
        assert!(
            matches!(err, ActionPayloadDecodeError::Malformed { .. }),
            "got {err:?}"
        );
    }
}
