//! Typed FlatBuffers payload codec for `nmp.browse_relay`.

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
#[path = "wire/generated/browse_relay_generated.rs"]
mod browse_relay_generated;

use crate::browse::{BrowseLifecycle, BrowseRelayAction};
use crate::substrate::{ActionPayload, ActionPayloadDecodeError};

use browse_relay_generated::nmp::core as browse_fb;

const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

fn gate_schema_version(found: u32) -> Result<(), ActionPayloadDecodeError> {
    if found != SCHEMA_VERSION {
        return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
            found,
            expected: SCHEMA_VERSION,
        });
    }
    Ok(())
}

fn lifecycle_to_wire(value: &BrowseLifecycle) -> browse_fb::BrowseRelayLifecycle {
    match value {
        BrowseLifecycle::Tailing => browse_fb::BrowseRelayLifecycle::Tailing,
        BrowseLifecycle::OneShot => browse_fb::BrowseRelayLifecycle::OneShot,
    }
}

fn lifecycle_from_wire(
    value: browse_fb::BrowseRelayLifecycle,
) -> Result<BrowseLifecycle, ActionPayloadDecodeError> {
    match value {
        browse_fb::BrowseRelayLifecycle::Tailing => Ok(BrowseLifecycle::Tailing),
        browse_fb::BrowseRelayLifecycle::OneShot => Ok(BrowseLifecycle::OneShot),
        unknown => Err(malformed(format!(
            "unknown BrowseRelayLifecycle ordinal: {}",
            unknown.0
        ))),
    }
}

impl ActionPayload for BrowseRelayAction {
    const SCHEMA_ID: &'static str = "nmp.browse_relay";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let (op, relay_url, kinds, lifecycle, interest_id) = match self {
            BrowseRelayAction::Open {
                relay_url,
                kinds,
                lifecycle,
                interest_id,
            } => (
                browse_fb::BrowseRelayOp::Open,
                Some(fbb.create_string(relay_url)),
                Some(fbb.create_vector(kinds)),
                lifecycle_to_wire(lifecycle),
                *interest_id,
            ),
            BrowseRelayAction::Close { interest_id } => (
                browse_fb::BrowseRelayOp::Close,
                None,
                None,
                browse_fb::BrowseRelayLifecycle::Tailing,
                *interest_id,
            ),
        };
        let payload = browse_fb::BrowseRelayPayload::create(
            &mut fbb,
            &browse_fb::BrowseRelayPayloadArgs {
                schema_version: SCHEMA_VERSION,
                op,
                relay_url,
                kinds,
                lifecycle,
                interest_id,
            },
        );
        browse_fb::finish_browse_relay_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !browse_fb::browse_relay_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing NBRW file identifier"));
        }
        let root = browse_fb::root_as_browse_relay_payload(bytes)
            .map_err(|e| malformed(format!("not a valid BrowseRelayPayload buffer: {e}")))?;
        gate_schema_version(root.schema_version())?;
        match root.op() {
            browse_fb::BrowseRelayOp::Open => {
                let relay_url = root.relay_url().unwrap_or_default().to_string();
                let kinds = root
                    .kinds()
                    .map(|values| values.iter().collect())
                    .unwrap_or_default();
                let lifecycle = lifecycle_from_wire(root.lifecycle())?;
                Ok(BrowseRelayAction::Open {
                    relay_url,
                    kinds,
                    lifecycle,
                    interest_id: root.interest_id(),
                })
            }
            browse_fb::BrowseRelayOp::Close => Ok(BrowseRelayAction::Close {
                interest_id: root.interest_id(),
            }),
            unknown => Err(malformed(format!(
                "unknown BrowseRelayOp ordinal: {}",
                unknown.0
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch_schema_version(mut bytes: Vec<u8>, new_version: u32) -> Vec<u8> {
        let root_off = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let vtable_soff = i32::from_le_bytes([
            bytes[root_off],
            bytes[root_off + 1],
            bytes[root_off + 2],
            bytes[root_off + 3],
        ]);
        let vtable_off = (root_off as i64 - vtable_soff as i64) as usize;
        let field_off = u16::from_le_bytes([bytes[vtable_off + 4], bytes[vtable_off + 5]]) as usize;
        let abs = root_off + field_off;
        bytes[abs..abs + 4].copy_from_slice(&new_version.to_le_bytes());
        bytes
    }

    fn patch_ubyte_slot(mut bytes: Vec<u8>, slot: usize, value: u8) -> Vec<u8> {
        let root_off = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let vtable_soff = i32::from_le_bytes([
            bytes[root_off],
            bytes[root_off + 1],
            bytes[root_off + 2],
            bytes[root_off + 3],
        ]);
        let vtable_off = (root_off as i64 - vtable_soff as i64) as usize;
        let field_off = u16::from_le_bytes([
            bytes[vtable_off + 4 + slot * 2],
            bytes[vtable_off + 5 + slot * 2],
        ]) as usize;
        assert_ne!(field_off, 0, "slot {slot} must be present");
        bytes[root_off + field_off] = value;
        bytes
    }

    #[test]
    fn open_round_trips() {
        let action = BrowseRelayAction::Open {
            relay_url: "wss://relay.example".to_string(),
            kinds: vec![1, 30023],
            lifecycle: BrowseLifecycle::OneShot,
            interest_id: 42,
        };
        assert_eq!(
            BrowseRelayAction::decode(&action.encode()).expect("decodes"),
            action
        );
    }

    #[test]
    fn close_round_trips() {
        let action = BrowseRelayAction::Close { interest_id: 42 };
        assert_eq!(
            BrowseRelayAction::decode(&action.encode()).expect("decodes"),
            action
        );
    }

    #[test]
    fn malformed_buffer_is_rejected() {
        assert!(matches!(
            BrowseRelayAction::decode(b"junk"),
            Err(ActionPayloadDecodeError::Malformed { .. })
        ));
    }

    #[test]
    fn wrong_file_identifier_is_rejected() {
        let mut bytes = BrowseRelayAction::Close { interest_id: 42 }.encode();
        bytes[4..8].copy_from_slice(b"XXXX");
        assert!(matches!(
            BrowseRelayAction::decode(&bytes),
            Err(ActionPayloadDecodeError::Malformed { .. })
        ));
    }

    #[test]
    fn wrong_schema_version_is_rejected() {
        let bytes = BrowseRelayAction::Close { interest_id: 42 }.encode();
        let err = BrowseRelayAction::decode(&patch_schema_version(bytes, 999))
            .expect_err("schema mismatch must reject");
        assert_eq!(
            err,
            ActionPayloadDecodeError::SchemaVersionMismatch {
                found: 999,
                expected: 1
            }
        );
    }

    #[test]
    fn unknown_op_ordinal_is_rejected() {
        let bytes = BrowseRelayAction::Close { interest_id: 42 }.encode();
        assert!(matches!(
            BrowseRelayAction::decode(&patch_ubyte_slot(bytes, 1, 99)),
            Err(ActionPayloadDecodeError::Malformed { .. })
        ));
    }

    #[test]
    fn unknown_lifecycle_ordinal_is_rejected() {
        let bytes = BrowseRelayAction::Open {
            relay_url: "wss://relay.example".to_string(),
            kinds: vec![1],
            lifecycle: BrowseLifecycle::OneShot,
            interest_id: 42,
        }
        .encode();
        assert!(matches!(
            BrowseRelayAction::decode(&patch_ubyte_slot(bytes, 4, 99)),
            Err(ActionPayloadDecodeError::Malformed { .. })
        ));
    }
}
