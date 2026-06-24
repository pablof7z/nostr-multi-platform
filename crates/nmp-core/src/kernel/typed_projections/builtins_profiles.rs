//! Wave C event-cluster slice of [`Kernel::builtin_typed_projections`].
//!
//! ADR-0063 Lane H: `mention_profiles` / `claimed_profiles` / `resolved_profiles`
//! typed projections deleted. Only `claimed_events` remains from the former
//! four-entry cluster.
//!
//! `claimed_events` flattens a `primary_id`-keyed map to a key-sorted
//! `[{key, value}]` vector (FlatBuffers has no map type). The DTO→Row mapping
//! is inlined where `ClaimedEventDto` is reachable (kernel::descendant). Each
//! Model is built from the SAME accessor the generic JSON projection in
//! [`snapshot_projections_with_publish_cluster`](super::super::Kernel::snapshot_projections_with_publish_cluster)
//! reads, in the same tick, so the typed and JSON wire forms cannot diverge.

use super::{
    encode_claimed_events, ClaimedEventRow, ClaimedEventsModel, CLAIMED_EVENTS_FILE_IDENTIFIER,
    CLAIMED_EVENTS_SCHEMA_ID, CLAIMED_EVENTS_SCHEMA_VERSION,
};
use crate::update_envelope::TypedProjectionData;

impl super::super::Kernel {
    /// Encode the Wave C event-cluster (Tier-2) built-ins as typed FlatBuffer
    /// sidecar entries. Called by
    /// [`builtin_typed_projections`](super::super::Kernel::builtin_typed_projections).
    ///
    /// ADR-0063 Lane H: mention_profiles / claimed_profiles / resolved_profiles
    /// removed; only claimed_events remains.
    pub(in crate::kernel) fn profiles_cluster_typed_projections(&self) -> Vec<TypedProjectionData> {
        let mut out = Vec::with_capacity(1);

        // `claimed_events` — encoded from the SAME `claimed_events()` BTreeMap the
        // JSON path serialises (already key-sorted). `tags: Vec<Vec<String>>` is
        // carried verbatim into the `[TagRow]` shape.
        let claimed_events = ClaimedEventsModel {
            entries: self
                .claimed_events()
                .iter()
                .map(|(key, dto)| {
                    (
                        key.clone(),
                        ClaimedEventRow {
                            primary_id: dto.primary_id.clone(),
                            id: dto.id.clone(),
                            author_pubkey: dto.author_pubkey.clone(),
                            kind: dto.kind,
                            created_at: dto.created_at,
                            tags: dto.tags.clone(),
                            content: dto.content.clone(),
                            content_tree_bytes: dto.content_tree_bytes.clone(),
                            signed_event_json: None,
                        },
                    )
                })
                .collect(),
        };
        out.push(TypedProjectionData {
            key: CLAIMED_EVENTS_SCHEMA_ID.to_string(),
            schema_id: CLAIMED_EVENTS_SCHEMA_ID.to_string(),
            schema_version: CLAIMED_EVENTS_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(CLAIMED_EVENTS_FILE_IDENTIFIER).into_owned(),
            payload: encode_claimed_events(&claimed_events),
            // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
            ..Default::default()
        });

        out
    }
}
