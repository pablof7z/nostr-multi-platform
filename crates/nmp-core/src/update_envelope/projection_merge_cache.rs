//! Rust-owned projection merge cache for incremental snapshot frames.
//!
//! Hosts that receive omitted `Unchanged` projections must apply
//! `Changed`/`Cleared` rows before rendering. This module keeps that policy on
//! the Rust side of the runtime boundary: web workers can publish already
//! merged `UpdateFrame` bytes and TypeScript clients can decode the current
//! frame without owning retention rules.

use std::collections::BTreeMap;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use super::{encode_typed_projections, TypedProjectionData, UpdateFrameDecodeError};
use crate::transport::wire as fb;
use crate::update_envelope::WireProjectionState;

/// Stateful merge cache keyed by projection id.
#[derive(Default)]
pub struct ProjectionMergeCache {
    frame_identity: Option<(u64, u64)>,
    projections: BTreeMap<String, TypedProjectionData>,
}

impl ProjectionMergeCache {
    /// Apply one snapshot frame and return bytes whose typed-projection vector
    /// contains the merged current set. Panic frames and malformed frames are
    /// returned as errors so the caller can surface a degraded state without
    /// mutating the last-good cache.
    pub fn merge_update_frame(&mut self, bytes: &[u8]) -> Result<Vec<u8>, UpdateFrameDecodeError> {
        self.merge_update_frame_with_extra_projections(bytes, std::iter::empty())
    }

    /// Apply one snapshot frame and append transient derived projections to the
    /// returned merged bytes.
    ///
    /// `extra` entries are not retained in this cache. They are for host-side
    /// composition roots that derive a compatibility sidecar from the merged
    /// source projections for the current outgoing frame.
    pub fn merge_update_frame_with_extra_projections(
        &mut self,
        bytes: &[u8],
        extra: impl IntoIterator<Item = TypedProjectionData>,
    ) -> Result<Vec<u8>, UpdateFrameDecodeError> {
        if !fb::update_frame_buffer_has_identifier(bytes) {
            return Err(UpdateFrameDecodeError::InvalidFlatbuffer(
                "missing NMPU file identifier".to_string(),
            ));
        }
        let frame = fb::root_as_update_frame(bytes)
            .map_err(|err| UpdateFrameDecodeError::InvalidFlatbuffer(format!("{err:?}")))?;
        if frame.kind() == fb::FrameKind::Panic {
            let msg = frame
                .panic()
                .map(|panic| panic.msg())
                .unwrap_or("")
                .to_string();
            return Err(UpdateFrameDecodeError::UnexpectedPanicFrame(msg));
        }
        if frame.kind() != fb::FrameKind::Snapshot {
            return Err(UpdateFrameDecodeError::MissingSnapshotPayload);
        }
        let snapshot = frame
            .snapshot()
            .ok_or(UpdateFrameDecodeError::MissingSnapshotPayload)?;
        let identity = (snapshot.session_id(), snapshot.snapshot_epoch());

        // Transactional merge (fail-closed): build the next projection set in a
        // LOCAL and only commit it to `self` AFTER decode fully succeeds. A
        // malformed frame that carries a new `(session_id, snapshot_epoch)` must
        // NOT clear/replace the live cache before its rows decode — otherwise a
        // decode error mid-frame would leave `self.projections` poisoned (empty
        // or partially applied) and the NEXT valid incremental frame would merge
        // from that corrupted baseline, silently overwriting `last_good`. The
        // `?` below propagates any decode error BEFORE the commit, so on error
        // `self` is left exactly as it was (the caller still holds last-good).
        let mut next_projections = if self.frame_identity == Some(identity) {
            // Same frame identity: continue from the current baseline.
            self.projections.clone()
        } else {
            // New frame identity: a fresh baseline (the prior epoch's rows are
            // dropped — but only once we are sure THIS frame decodes).
            BTreeMap::new()
        };
        for row in super::typed_projection_decode::decode_typed_projections(&snapshot)? {
            match row.state {
                WireProjectionState::Changed => {
                    next_projections.insert(row.key.clone(), row);
                }
                WireProjectionState::Cleared => {
                    next_projections.remove(&row.key);
                }
            }
        }
        // Commit: decode succeeded, so atomically adopt the new identity + set.
        self.frame_identity = Some(identity);
        self.projections = next_projections;
        let mut output = self.projections.clone();
        for entry in extra {
            output.insert(entry.key.clone(), entry);
        }
        let merged: Vec<TypedProjectionData> = output.values().cloned().collect();
        Ok(rewrite_typed_projections(&snapshot, &merged))
    }
}

fn rewrite_typed_projections(
    snapshot: &fb::SnapshotFrame<'_>,
    typed: &[TypedProjectionData],
) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();
    let typed_projections = encode_typed_projections(&mut builder, typed);
    let update_kind = snapshot.update_kind().map(|s| builder.create_string(s));
    let metrics = snapshot.metrics().map(|m| encode_metrics(&mut builder, &m));
    let relay_status = snapshot
        .relay_status()
        .map(|r| encode_relay_status(&mut builder, &r));
    let relay_statuses = encode_relay_statuses(&mut builder, snapshot.relay_statuses());
    let logical_interests = encode_logical_interests(&mut builder, snapshot.logical_interests());
    let wire_subscriptions = encode_wire_subscriptions(&mut builder, snapshot.wire_subscriptions());
    let logs = encode_string_vector(&mut builder, snapshot.logs());
    let last_error_toast = snapshot
        .last_error_toast()
        .map(|s| builder.create_string(s));
    let last_error_category = snapshot
        .last_error_category()
        .map(|s| builder.create_string(s));
    let last_planner_error = snapshot
        .last_planner_error()
        .map(|s| builder.create_string(s));
    let store_open_failure = snapshot
        .store_open_failure()
        .map(|s| builder.create_string(s));
    let negentropy_sync_stats = snapshot
        .negentropy_sync_stats()
        .map(|s| encode_negentropy_sync_stats(&mut builder, &s));

    let snapshot_offset = fb::SnapshotFrame::create(
        &mut builder,
        &fb::SnapshotFrameArgs {
            schema_version: snapshot.schema_version(),
            typed_projections,
            rev: snapshot.rev(),
            kernel_schema_version: snapshot.kernel_schema_version(),
            last_tick_ms: snapshot.last_tick_ms(),
            update_kind,
            running: snapshot.running(),
            metrics,
            relay_status,
            relay_statuses,
            logical_interests,
            wire_subscriptions,
            logs,
            last_error_toast,
            last_error_category,
            last_planner_error,
            store_open_failure,
            no_configured_relays: snapshot.no_configured_relays(),
            negentropy_sync_stats,
            snapshot_epoch: snapshot.snapshot_epoch(),
            session_id: snapshot.session_id(),
        },
    );
    let root = fb::UpdateFrame::create(
        &mut builder,
        &fb::UpdateFrameArgs {
            kind: fb::FrameKind::Snapshot,
            snapshot: Some(snapshot_offset),
            panic: None,
        },
    );
    fb::finish_update_frame_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}

fn encode_metrics<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    m: &fb::Metrics<'_>,
) -> WIPOffset<fb::Metrics<'bldr>> {
    fb::Metrics::create(
        builder,
        &fb::MetricsArgs {
            generated_events: m.generated_events(),
            note_events: m.note_events(),
            profile_events: m.profile_events(),
            duplicate_events: m.duplicate_events(),
            delete_events: m.delete_events(),
            stored_events: m.stored_events(),
            tombstones: m.tombstones(),
            visible_items: m.visible_items(),
            visible_profiled_items: m.visible_profiled_items(),
            visible_placeholder_avatar_items: m.visible_placeholder_avatar_items(),
            open_views: m.open_views(),
            events_since_last_update: m.events_since_last_update(),
            diagnostic_firehose_events: m.diagnostic_firehose_events(),
            inserted_count: m.inserted_count(),
            updated_count: m.updated_count(),
            removed_count: m.removed_count(),
            emit_hz_configured: m.emit_hz_configured(),
            update_sequence: m.update_sequence(),
            estimated_store_bytes: m.estimated_store_bytes(),
            payload_bytes: m.payload_bytes(),
            store_to_payload_ratio: m.store_to_payload_ratio(),
            actor_queue_depth: m.actor_queue_depth(),
            frames_rx: m.frames_rx(),
            events_rx: m.events_rx(),
            eose_rx: m.eose_rx(),
            notices_rx: m.notices_rx(),
            closed_rx: m.closed_rx(),
            bytes_rx: m.bytes_rx(),
            bytes_tx: m.bytes_tx(),
            contacts_authors: m.contacts_authors(),
            timeline_authors: m.timeline_authors(),
            first_event_ms: m.first_event_ms(),
            target_profile_loaded_ms: m.target_profile_loaded_ms(),
            timeline_opened_ms: m.timeline_opened_ms(),
            timeline_first_item_ms: m.timeline_first_item_ms(),
            update_emitted_ms: m.update_emitted_ms(),
            last_event_to_emit_ms: m.last_event_to_emit_ms(),
            max_event_to_emit_ms: m.max_event_to_emit_ms(),
            max_events_per_update: m.max_events_per_update(),
            claim_drops_total: m.claim_drops_total(),
            make_update_us: m.make_update_us(),
            serialize_us: m.serialize_us(),
            update_frame_degradations_total: m.update_frame_degradations_total(),
        },
    )
}

fn encode_relay_status<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    r: &fb::RelayStatus<'_>,
) -> WIPOffset<fb::RelayStatus<'bldr>> {
    let role = r.role().map(|s| builder.create_string(s));
    let relay_url = r.relay_url().map(|s| builder.create_string(s));
    let connection = r.connection().map(|s| builder.create_string(s));
    let auth = r.auth().map(|s| builder.create_string(s));
    let negentropy_probe = r.negentropy_probe().map(|s| builder.create_string(s));
    let last_notice = r.last_notice().map(|s| builder.create_string(s));
    let last_error = r.last_error().map(|s| builder.create_string(s));
    let error_category = r.error_category().map(|s| builder.create_string(s));
    let last_close_reason = r.last_close_reason().map(|s| builder.create_string(s));
    fb::RelayStatus::create(
        builder,
        &fb::RelayStatusArgs {
            role,
            relay_url,
            connection,
            auth,
            negentropy_probe,
            active_wire_subscriptions: r.active_wire_subscriptions(),
            reconnect_count: r.reconnect_count(),
            last_connected_at_ms: r.last_connected_at_ms(),
            last_event_at_ms: r.last_event_at_ms(),
            last_notice,
            last_error,
            error_category,
            events_rx: r.events_rx(),
            bytes_rx: r.bytes_rx(),
            bytes_tx: r.bytes_tx(),
            denied: r.denied(),
            last_close_reason,
        },
    )
}

fn encode_relay_statuses<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    rows: Option<flatbuffers::Vector<'_, flatbuffers::ForwardsUOffset<fb::RelayStatus<'_>>>>,
) -> Option<
    WIPOffset<flatbuffers::Vector<'bldr, flatbuffers::ForwardsUOffset<fb::RelayStatus<'bldr>>>>,
> {
    let rows = rows?;
    let offsets: Vec<_> = (0..rows.len())
        .map(|i| encode_relay_status(builder, &rows.get(i)))
        .collect();
    Some(builder.create_vector(&offsets))
}

fn encode_logical_interests<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    rows: Option<
        flatbuffers::Vector<'_, flatbuffers::ForwardsUOffset<fb::LogicalInterestStatus<'_>>>,
    >,
) -> Option<
    WIPOffset<
        flatbuffers::Vector<'bldr, flatbuffers::ForwardsUOffset<fb::LogicalInterestStatus<'bldr>>>,
    >,
> {
    let rows = rows?;
    let offsets: Vec<_> = (0..rows.len())
        .map(|i| {
            let row = rows.get(i);
            let key = row.key().map(|s| builder.create_string(s));
            let state = row.state().map(|s| builder.create_string(s));
            let relay_urls = encode_string_vector(builder, row.relay_urls());
            let cache_coverage = row.cache_coverage().map(|s| builder.create_string(s));
            fb::LogicalInterestStatus::create(
                builder,
                &fb::LogicalInterestStatusArgs {
                    key,
                    state,
                    refcount: row.refcount(),
                    relay_urls,
                    cache_coverage,
                    warming_until_ms: row.warming_until_ms(),
                },
            )
        })
        .collect();
    Some(builder.create_vector(&offsets))
}

fn encode_wire_subscriptions<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    rows: Option<
        flatbuffers::Vector<'_, flatbuffers::ForwardsUOffset<fb::WireSubscriptionStatus<'_>>>,
    >,
) -> Option<
    WIPOffset<
        flatbuffers::Vector<'bldr, flatbuffers::ForwardsUOffset<fb::WireSubscriptionStatus<'bldr>>>,
    >,
> {
    let rows = rows?;
    let offsets: Vec<_> = (0..rows.len())
        .map(|i| {
            let row = rows.get(i);
            let wire_id = row.wire_id().map(|s| builder.create_string(s));
            let relay_url = row.relay_url().map(|s| builder.create_string(s));
            let filter_summary = row.filter_summary().map(|s| builder.create_string(s));
            let state = row.state().map(|s| builder.create_string(s));
            let close_reason = row.close_reason().map(|s| builder.create_string(s));
            fb::WireSubscriptionStatus::create(
                builder,
                &fb::WireSubscriptionStatusArgs {
                    wire_id,
                    relay_url,
                    filter_summary,
                    state,
                    logical_consumer_count: row.logical_consumer_count(),
                    events_rx: row.events_rx(),
                    opened_at_ms: row.opened_at_ms(),
                    last_event_at_ms: row.last_event_at_ms(),
                    eose_at_ms: row.eose_at_ms(),
                    close_reason,
                },
            )
        })
        .collect();
    Some(builder.create_vector(&offsets))
}

fn encode_string_vector<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    values: Option<flatbuffers::Vector<'_, flatbuffers::ForwardsUOffset<&str>>>,
) -> Option<WIPOffset<flatbuffers::Vector<'bldr, flatbuffers::ForwardsUOffset<&'bldr str>>>> {
    let values = values?;
    let offsets: Vec<_> = (0..values.len())
        .map(|i| builder.create_string(values.get(i)))
        .collect();
    Some(builder.create_vector(&offsets))
}

fn encode_negentropy_sync_stats<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    stats: &fb::NegentropySyncStats<'_>,
) -> WIPOffset<fb::NegentropySyncStats<'bldr>> {
    fb::NegentropySyncStats::create(
        builder,
        &fb::NegentropySyncStatsArgs {
            rounds: stats.rounds(),
            have_ids: stats.have_ids(),
            need_ids: stats.need_ids(),
            local_item_count: stats.local_item_count(),
            transfer_avoided_bytes: stats.transfer_avoided_bytes(),
            last_reconcile_at_ms: stats.last_reconcile_at_ms(),
        },
    )
}

#[cfg(test)]
mod tests;
