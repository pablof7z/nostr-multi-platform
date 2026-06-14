//! Wave C publish-cluster slice of [`Kernel::builtin_typed_projections`].
//!
//! The three publish/outbox built-ins (`publish_queue` / `publish_outbox` /
//! `outbox_summary`) carry heavier nested DTO→Row mappings than the
//! relay/settings built-ins. Their mappings must be inlined where the
//! `pub(super)`/`pub(crate)` DTO types (`PublishQueueEntry`,
//! `PublishOutboxItem`, `OutboxSummarySnapshot`, ...) are reachable — i.e. in a
//! `kernel::` descendant — but kept under the same owner and out of
//! `mod.rs` so that file stays under the LOC ceiling. Each row is built from
//! the SAME accessor the generic JSON projection in
//! [`snapshot_projections_with_publish_cluster`](super::super::Kernel::snapshot_projections_with_publish_cluster)
//! reads, in the same tick, so the typed and JSON wire forms cannot diverge.

use super::{
    encode_outbox_summary, encode_publish_outbox, encode_publish_queue, OutboxSummaryModel,
    PublishOutboxItemRow, PublishOutboxModel, PublishOutboxRelayRow, PublishQueueEntryRow,
    PublishQueueModel, RelayAckOutcomeRow, OUTBOX_SUMMARY_FILE_IDENTIFIER, OUTBOX_SUMMARY_SCHEMA_ID,
    OUTBOX_SUMMARY_SCHEMA_VERSION, PUBLISH_OUTBOX_FILE_IDENTIFIER, PUBLISH_OUTBOX_SCHEMA_ID,
    PUBLISH_OUTBOX_SCHEMA_VERSION, PUBLISH_QUEUE_FILE_IDENTIFIER, PUBLISH_QUEUE_SCHEMA_ID,
    PUBLISH_QUEUE_SCHEMA_VERSION,
};
use crate::update_envelope::TypedProjectionData;

impl super::super::Kernel {
    /// Encode the Wave C publish-cluster (Tier-2) built-ins as typed
    /// FlatBuffer sidecar entries, in `publish_queue` → `publish_outbox` →
    /// `outbox_summary` order. Called by
    /// [`builtin_typed_projections`](super::super::Kernel::builtin_typed_projections);
    /// see that method's doc for the mechanism.
    pub(in crate::kernel) fn publish_cluster_typed_projections(&self) -> Vec<TypedProjectionData> {
        let mut out = Vec::with_capacity(3);

        // `publish_queue` — encoded from the SAME `PublishQueueEntry` slice the
        // JSON path serialises (`publish_queue_snapshot()`). The two
        // `#[serde(skip)]` fields (`signed_event`, `target`) never cross the
        // wire and are omitted.
        let publish_queue = PublishQueueModel {
            entries: self
                .publish_queue_snapshot()
                .iter()
                .map(|entry| PublishQueueEntryRow {
                    event_id: entry.event_id.clone(),
                    kind: entry.kind,
                    title: entry.title.clone(),
                    target_relays: entry.target_relays as u32,
                    status: entry.status.clone(),
                    can_retry: entry.can_retry,
                    relay_outcomes: entry
                        .relay_outcomes
                        .iter()
                        .map(|outcome| RelayAckOutcomeRow {
                            relay_url: outcome.relay_url.clone(),
                            status: outcome.status.clone(),
                            message: outcome.message.clone(),
                            relay_reason: outcome.relay_reason.clone(),
                        })
                        .collect(),
                })
                .collect(),
        };
        out.push(TypedProjectionData {
            key: PUBLISH_QUEUE_SCHEMA_ID.to_string(),
            schema_id: PUBLISH_QUEUE_SCHEMA_ID.to_string(),
            schema_version: PUBLISH_QUEUE_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(PUBLISH_QUEUE_FILE_IDENTIFIER).into_owned(),
            payload: encode_publish_queue(&publish_queue),
            // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
            ..Default::default()
        });

        // `publish_outbox` — encoded from the SAME `PublishOutboxItem` vector the
        // JSON path serialises (`publish_outbox_items()`), nested relays
        // included.
        let publish_outbox = PublishOutboxModel {
            items: self
                .publish_outbox_items()
                .iter()
                .map(|item| PublishOutboxItemRow {
                    handle: item.handle.clone(),
                    event_id: item.event_id.clone(),
                    kind: item.kind,
                    title: item.title.clone(),
                    preview: item.preview.clone(),
                    created_at: item.created_at,
                    status: item.status.clone(),
                    status_label: item.status_label.clone(),
                    system_image: item.system_image.clone(),
                    can_retry: item.can_retry,
                    target_relays: item.target_relays as u32,
                    relays: item
                        .relays
                        .iter()
                        .map(|relay| PublishOutboxRelayRow {
                            relay_url: relay.relay_url.clone(),
                            status: relay.status.clone(),
                            status_label: relay.status_label.clone(),
                            attempt: relay.attempt,
                            attempt_label: relay.attempt_label.clone(),
                            message: relay.message.clone(),
                            relay_reason: relay.relay_reason.clone(),
                        })
                        .collect(),
                })
                .collect(),
        };
        out.push(TypedProjectionData {
            key: PUBLISH_OUTBOX_SCHEMA_ID.to_string(),
            schema_id: PUBLISH_OUTBOX_SCHEMA_ID.to_string(),
            schema_version: PUBLISH_OUTBOX_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(PUBLISH_OUTBOX_FILE_IDENTIFIER).into_owned(),
            payload: encode_publish_outbox(&publish_outbox),
            // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
            ..Default::default()
        });

        // `outbox_summary` — encoded from the SAME `OutboxSummarySnapshot` the
        // JSON path serialises (`outbox_summary_snapshot()`). The kernel owns
        // both the counters AND the English `title`/`subtitle` strings.
        let dto = self.outbox_summary_snapshot();
        let outbox_summary = OutboxSummaryModel {
            title: dto.title.clone(),
            subtitle: dto.subtitle.clone(),
            total: dto.total,
            sending: dto.sending,
            retrying: dto.retrying,
            queued: dto.queued,
            failed: dto.failed,
        };
        out.push(TypedProjectionData {
            key: OUTBOX_SUMMARY_SCHEMA_ID.to_string(),
            schema_id: OUTBOX_SUMMARY_SCHEMA_ID.to_string(),
            schema_version: OUTBOX_SUMMARY_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(OUTBOX_SUMMARY_FILE_IDENTIFIER).into_owned(),
            payload: encode_outbox_summary(&outbox_summary),
            // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
            ..Default::default()
        });

        out
    }
}
