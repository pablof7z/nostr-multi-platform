//! Browser-owned `refs.event.envelopes` derived projection.
//!
//! The browser runtime receives raw kernel frames where `refs.event` is the
//! authoritative event-reference source. Web shells still want the render-facing
//! NEMB sidecar so they can dispatch content cards without re-parsing Nostr
//! event kinds or tags. This module is that Rust composition root: keep a
//! persistent `RefEventStore`, derive envelopes through `nmp-content`, and hand
//! the caller one transient typed projection to append to the outgoing frame.

use std::collections::BTreeMap;

use nmp_content::wire::{
    encode_ref_event_envelopes, EMBED_SIDECAR_FILE_IDENTIFIER, EMBED_SIDECAR_PROJECTION_KEY,
    EMBED_SIDECAR_SCHEMA_ID, EMBED_SIDECAR_SCHEMA_VERSION,
};
use nmp_content::{derive_ref_event_store_envelopes, EmbeddedEventEnvelope};
use nmp_core::{
    decode_snapshot_envelope, decode_snapshot_typed_projections,
    refs::{RefEventStore, REFS_EVENT_KEY},
    TypedProjectionData,
};

/// Stateful browser mirror of `refs.event` plus derived render envelopes.
#[derive(Clone, Debug, Default)]
pub(crate) struct BrowserEmbedSidecar {
    ref_events: RefEventStore,
    envelopes: BTreeMap<String, EmbeddedEventEnvelope>,
}

impl BrowserEmbedSidecar {
    /// Apply the raw kernel frame's `refs.event` row-delta, if present.
    ///
    /// Missing or malformed source rows are fail-closed no-ops: the prior store
    /// and envelope map remain live. The caller clones this state before merge
    /// and only commits the clone after the frame itself merges successfully.
    pub(crate) fn apply_raw_frame(&mut self, frame_bytes: &[u8]) {
        let Ok(envelope) = decode_snapshot_envelope(frame_bytes) else {
            return;
        };
        let Ok(projections) = decode_snapshot_typed_projections(frame_bytes) else {
            return;
        };
        let Some(entry) = projections
            .iter()
            .find(|entry| entry.key == REFS_EVENT_KEY || entry.schema_id == REFS_EVENT_KEY)
        else {
            return;
        };

        self.ref_events
            .apply_sidecar(&entry.payload, envelope.session_id, envelope.snapshot_epoch);
        self.envelopes = derive_ref_event_store_envelopes(&self.ref_events);
    }

    /// Build the transient NEMB projection appended to the browser frame.
    pub(crate) fn typed_projection(&self) -> TypedProjectionData {
        TypedProjectionData {
            key: EMBED_SIDECAR_PROJECTION_KEY.to_string(),
            schema_id: EMBED_SIDECAR_SCHEMA_ID.to_string(),
            schema_version: EMBED_SIDECAR_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(EMBED_SIDECAR_FILE_IDENTIFIER).into_owned(),
            payload: encode_ref_event_envelopes(&self.envelopes),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_content::EmbedKindProjection;
    use nmp_core::refs::{encode_ref_row_delta_batch, RefRow, RefRowDeltaBatch};
    use nmp_core::typed_projections::{encode_claimed_events, ClaimedEventRow, ClaimedEventsModel};
    use nmp_core::{encode_snapshot_frame, SnapshotEnvelope, WireProjectionState};

    #[test]
    fn derives_nemb_envelope_from_refs_event_row() {
        let primary_id = "ab".repeat(32);
        let row = ClaimedEventRow {
            primary_id: primary_id.clone(),
            id: primary_id.clone(),
            author_pubkey: "cd".repeat(32),
            kind: 1,
            created_at: 123,
            tags: Vec::new(),
            content: "hello from refs.event".to_string(),
            content_tree_bytes: Vec::new(),
            signed_event_json: None,
        };
        let event_payload = encode_claimed_events(&ClaimedEventsModel {
            entries: vec![(primary_id.clone(), row)],
        });
        let refs_payload = encode_ref_row_delta_batch(&RefRowDeltaBatch {
            namespace: "event".to_string(),
            baseline: true,
            rows: vec![RefRow::changed(primary_id.clone(), 1, event_payload)],
        });
        let frame = encode_snapshot_frame(
            &SnapshotEnvelope {
                session_id: 1,
                update_kind: "ViewBatch".to_string(),
                ..Default::default()
            },
            &[TypedProjectionData {
                key: REFS_EVENT_KEY.to_string(),
                schema_id: REFS_EVENT_KEY.to_string(),
                schema_version: 1,
                file_identifier: "NRRD".to_string(),
                payload: refs_payload,
                state: WireProjectionState::Changed,
                ..Default::default()
            }],
        );

        let mut sidecar = BrowserEmbedSidecar::default();
        sidecar.apply_raw_frame(&frame);
        let typed = sidecar.typed_projection();
        let envelopes = nmp_content::wire::decode_ref_event_envelopes(&typed.payload)
            .expect("NEMB payload decodes");
        let envelope = envelopes
            .get(&primary_id)
            .expect("refs.event row resolves to an envelope");

        match &envelope.projection {
            EmbedKindProjection::ShortNote(note) => {
                assert_eq!(note.id, primary_id);
                assert!(!note.content_tree.roots.is_empty());
            }
            other => panic!("expected shortNote projection, got {other:?}"),
        }
    }
}
