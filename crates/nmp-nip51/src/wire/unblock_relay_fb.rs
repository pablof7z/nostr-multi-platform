//! Typed FlatBuffers payload codec for `nmp.nip51.unblock_relay`.

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
#[path = "generated/unblock_relay_generated.rs"]
pub mod generated;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use generated::nmp::nip_51 as fb;

use crate::block_relay::UnblockRelayInput;

pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

impl ActionPayload for UnblockRelayInput {
    const SCHEMA_ID: &'static str = "nmp.nip51.unblock_relay";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let url = fbb.create_string(&self.url);
        let account_pubkey = fbb.create_string(&self.account_pubkey);
        let payload = fb::UnblockRelayPayload::create(
            &mut fbb,
            &fb::UnblockRelayPayloadArgs {
                schema_version: SCHEMA_VERSION,
                url: Some(url),
                account_pubkey: Some(account_pubkey),
            },
        );
        fb::finish_unblock_relay_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !fb::unblock_relay_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing NUBL file identifier"));
        }
        let root = fb::root_as_unblock_relay_payload(bytes)
            .map_err(|e| malformed(format!("not a valid UnblockRelayPayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(UnblockRelayInput {
            url: root.url().to_string(),
            account_pubkey: root.account_pubkey().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockRelayInput;

    const PUBKEY: &str = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";

    #[test]
    fn unblock_relay_round_trips() {
        let action = UnblockRelayInput {
            url: "wss://relay.example".to_string(),
            account_pubkey: PUBKEY.to_string(),
        };
        let decoded = UnblockRelayInput::decode(&action.encode()).expect("decodes");
        assert_eq!(decoded, action);
    }

    #[test]
    fn unblock_relay_wrong_schema_version_is_rejected() {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let url = fbb.create_string("wss://relay.example");
        let account_pubkey = fbb.create_string(PUBKEY);
        let payload = fb::UnblockRelayPayload::create(
            &mut fbb,
            &fb::UnblockRelayPayloadArgs {
                schema_version: 999,
                url: Some(url),
                account_pubkey: Some(account_pubkey),
            },
        );
        fb::finish_unblock_relay_payload_buffer(&mut fbb, payload);
        let bytes = fbb.finished_data().to_vec();
        let err = UnblockRelayInput::decode(&bytes).expect_err("bad version rejected");
        assert_eq!(
            err,
            ActionPayloadDecodeError::SchemaVersionMismatch {
                found: 999,
                expected: SCHEMA_VERSION
            }
        );
    }

    #[test]
    fn block_payload_does_not_decode_as_unblock() {
        let block = BlockRelayInput {
            url: "wss://relay.example".to_string(),
            account_pubkey: PUBKEY.to_string(),
        };
        let err = UnblockRelayInput::decode(&block.encode()).expect_err("cross-namespace rejected");
        assert!(
            matches!(err, ActionPayloadDecodeError::Malformed { .. }),
            "got {err:?}"
        );
    }
}
