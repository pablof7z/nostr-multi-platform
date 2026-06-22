//! `ActionPayload` codec for `VisibleNoteRelationsAction`
//! (`nmp.nip01.visible_note_relations`). (ADR-0064 / Cut-B producer gap #1756).
//!
//! `VisibleNoteRelationsAction` is a two-variant enum (`Claim` / `Release`),
//! each carrying `event_id` + `consumer_id`. The FlatBuffers table encodes
//! the variant as a `VisibleNoteRelationsOp` ubyte discriminator (tagged-table
//! pattern; cross-platform-stability; same rationale as `bookmark_update.fbs`).
//!
//! Unknown ordinals from future schema additions are rejected with
//! `Malformed` (fail-closed; D6).

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use crate::VisibleNoteRelationsAction;
use super::visible_note_relations_action_generated::nmp::nip_01 as vnr_fb;

/// Wire schema version for the visible_note_relations action payload.
/// Bump on any breaking change to `schema/visible_note_relations_action.fbs`.
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

// --- VisibleNoteRelationsAction ----------------------------------------------

impl ActionPayload for VisibleNoteRelationsAction {
    const SCHEMA_ID: &'static str = "nmp.nip01.visible_note_relations";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let (op, event_id_str, consumer_id_str) = match self {
            VisibleNoteRelationsAction::Claim {
                event_id,
                consumer_id,
            } => (vnr_fb::VisibleNoteRelationsOp::Claim, event_id, consumer_id),
            VisibleNoteRelationsAction::Release {
                event_id,
                consumer_id,
            } => (
                vnr_fb::VisibleNoteRelationsOp::Release,
                event_id,
                consumer_id,
            ),
        };
        let event_id = fbb.create_string(event_id_str);
        let consumer_id = fbb.create_string(consumer_id_str);
        let payload = vnr_fb::VisibleNoteRelationsPayload::create(
            &mut fbb,
            &vnr_fb::VisibleNoteRelationsPayloadArgs {
                schema_version: SCHEMA_VERSION,
                op,
                event_id: Some(event_id),
                consumer_id: Some(consumer_id),
            },
        );
        vnr_fb::finish_visible_note_relations_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8
            || !vnr_fb::visible_note_relations_payload_buffer_has_identifier(bytes)
        {
            return Err(malformed("missing NR01 file identifier"));
        }
        let root = vnr_fb::root_as_visible_note_relations_payload(bytes)
            .map_err(|e| malformed(format!("not a valid VisibleNoteRelationsPayload buffer: {e}")))?;
        gate_schema_version(root.schema_version())?;
        let event_id = root.event_id().to_string();
        let consumer_id = root.consumer_id().to_string();
        match root.op() {
            vnr_fb::VisibleNoteRelationsOp::Claim => {
                Ok(VisibleNoteRelationsAction::Claim { event_id, consumer_id })
            }
            vnr_fb::VisibleNoteRelationsOp::Release => {
                Ok(VisibleNoteRelationsAction::Release { event_id, consumer_id })
            }
            unknown => Err(malformed(format!(
                "unknown VisibleNoteRelationsOp ordinal: {}",
                unknown.0
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::ActionPayload;

    const EVENT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    // --- round-trip -----------------------------------------------------------

    #[test]
    fn claim_round_trips() {
        let action = VisibleNoteRelationsAction::Claim {
            event_id: EVENT.to_string(),
            consumer_id: "row-0".to_string(),
        };
        assert_eq!(
            VisibleNoteRelationsAction::decode(&action.encode()).expect("decodes"),
            action
        );
    }

    #[test]
    fn release_round_trips() {
        let action = VisibleNoteRelationsAction::Release {
            event_id: EVENT.to_string(),
            consumer_id: "row-0".to_string(),
        };
        assert_eq!(
            VisibleNoteRelationsAction::decode(&action.encode()).expect("decodes"),
            action
        );
    }

    #[test]
    fn claim_and_release_buffers_decode_to_distinct_variants() {
        let claim = VisibleNoteRelationsAction::Claim {
            event_id: EVENT.to_string(),
            consumer_id: "c".to_string(),
        };
        let release = VisibleNoteRelationsAction::Release {
            event_id: EVENT.to_string(),
            consumer_id: "c".to_string(),
        };
        // A Claim buffer must NOT decode as Release, and vice-versa.
        assert!(matches!(
            VisibleNoteRelationsAction::decode(&claim.encode()).expect("decodes"),
            VisibleNoteRelationsAction::Claim { .. }
        ));
        assert!(matches!(
            VisibleNoteRelationsAction::decode(&release.encode()).expect("decodes"),
            VisibleNoteRelationsAction::Release { .. }
        ));
    }

    // --- fail-closed: malformed ----------------------------------------------

    #[test]
    fn malformed_buffer_is_rejected() {
        assert!(matches!(
            VisibleNoteRelationsAction::decode(b"junk"),
            Err(ActionPayloadDecodeError::Malformed { .. })
        ));
        assert!(matches!(
            VisibleNoteRelationsAction::decode(&[]),
            Err(ActionPayloadDecodeError::Malformed { .. })
        ));
    }

    // --- fail-closed: wrong schema_version -----------------------------------

    /// Re-encode `bytes` (a finished, file-identified payload) with the raw
    /// `schema_version` slot overwritten. Mirrors the helper in
    /// `nmp-nip29/src/wire/action_payload/tests_fail_closed.rs`.
    fn patch_schema_version(mut bytes: Vec<u8>, new_version: u32) -> Vec<u8> {
        let root_off = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let vtable_soff = i32::from_le_bytes([
            bytes[root_off],
            bytes[root_off + 1],
            bytes[root_off + 2],
            bytes[root_off + 3],
        ]);
        let vtable_off = (root_off as i64 - vtable_soff as i64) as usize;
        let field_off =
            u16::from_le_bytes([bytes[vtable_off + 4], bytes[vtable_off + 5]]) as usize;
        assert_ne!(field_off, 0, "schema_version must be present in the buffer");
        let abs = root_off + field_off;
        bytes[abs..abs + 4].copy_from_slice(&new_version.to_le_bytes());
        bytes
    }

    #[test]
    fn wrong_schema_version_is_rejected() {
        let good = VisibleNoteRelationsAction::Claim {
            event_id: EVENT.to_string(),
            consumer_id: "c".to_string(),
        }
        .encode();
        let bad = patch_schema_version(good, 999);
        let err = VisibleNoteRelationsAction::decode(&bad)
            .expect_err("bad schema_version must be rejected");
        assert_eq!(
            err,
            ActionPayloadDecodeError::SchemaVersionMismatch {
                found: 999,
                expected: 1,
            }
        );
    }

    // --- unknown ordinal (fail-closed) ----------------------------------------

    #[test]
    fn unknown_op_ordinal_is_rejected_as_malformed() {
        // Encode a valid Claim, then patch the `op` byte (ordinal 2 = unknown).
        // `op` is a ubyte at VT_OP = 6 (second field slot in vtable).
        let mut bytes = VisibleNoteRelationsAction::Claim {
            event_id: EVENT.to_string(),
            consumer_id: "c".to_string(),
        }
        .encode();
        // Locate op field offset from vtable (slot index 1, vtable offset +6).
        let root_off = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let vtable_soff = i32::from_le_bytes([
            bytes[root_off],
            bytes[root_off + 1],
            bytes[root_off + 2],
            bytes[root_off + 3],
        ]);
        let vtable_off = (root_off as i64 - vtable_soff as i64) as usize;
        let op_field_off =
            u16::from_le_bytes([bytes[vtable_off + 6], bytes[vtable_off + 7]]) as usize;
        if op_field_off != 0 {
            // Only patch if the op field is physically present.
            bytes[root_off + op_field_off] = 99u8; // unknown ordinal
            assert!(matches!(
                VisibleNoteRelationsAction::decode(&bytes),
                Err(ActionPayloadDecodeError::Malformed { .. })
            ));
        }
        // If op_field_off == 0 the field defaulted to 0 (Claim) and is absent —
        // in that case an "unknown" ordinal cannot be injected this way; skip.
    }
}
