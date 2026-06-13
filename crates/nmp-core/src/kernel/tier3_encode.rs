//! ADR-0044 — typed Tier-3 envelope encoding for `SnapshotFrame`.
//!
//! Builds the typed `SnapshotFrame` envelope offsets (`rev`, `running`,
//! `metrics`, `relay_statuses`, …) directly from the `KernelSnapshot` struct.
//! It lives in `kernel` (not `update_envelope`) because the struct's fields are
//! `pub(super)`-visible here, so the typed fields are populated from the struct
//! independently of the JSON `payload` (never by re-walking the generic tree).
//!
//! `encode_tier3` returns a [`Tier3Offsets`] bundle of `Copy` `WIPOffset`
//! handles and scalars; the caller (`update_envelope::tier3_frame`) re-borrows
//! the same builder to assemble the final `SnapshotFrame` — the same pattern
//! `encode_typed_projections` already uses.
//!
//! Values are raw per ADR-0032. `usize`/`u128` counters narrow to `u64`
//! (saturating, so an impossibly-large value clamps rather than wraps);
//! `Option<u128>`/`Option<bool>` become FlatBuffers native-optional scalars;
//! `Option<String>` becomes a string field that is simply absent when `None`
//! (absent = healthy / not-active).

use super::types::{
    KernelSnapshot, LogicalInterestStatus, Metrics, NegentropySyncStats, RelayStatus,
    WireSubscriptionStatus,
};
use crate::transport::wire as fb;
use flatbuffers::{FlatBufferBuilder, WIPOffset};

/// Narrow a `u128` counter to the wire `u64`, saturating instead of wrapping.
#[inline]
fn u64_sat(value: u128) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// Narrow an optional `u128` timestamp to an optional wire `u64` (saturating).
#[inline]
fn opt_u64_sat(value: Option<u128>) -> Option<u64> {
    value.map(u64_sat)
}

/// Typed Tier-3 envelope offsets, ready to drop into `SnapshotFrameArgs`.
///
/// Every field is a `Copy` handle (`WIPOffset`) or a scalar, so the producing
/// `encode_tier3` can return this bundle and let the caller re-borrow the same
/// `FlatBufferBuilder` to build the enclosing `SnapshotFrame`.
pub(crate) struct Tier3Offsets<'b> {
    pub(crate) rev: u64,
    pub(crate) kernel_schema_version: u32,
    pub(crate) last_tick_ms: u64,
    pub(crate) update_kind: WIPOffset<&'b str>,
    pub(crate) running: bool,
    pub(crate) metrics: WIPOffset<fb::Metrics<'b>>,
    pub(crate) relay_status: WIPOffset<fb::RelayStatus<'b>>,
    pub(crate) relay_statuses:
        WIPOffset<flatbuffers::Vector<'b, flatbuffers::ForwardsUOffset<fb::RelayStatus<'b>>>>,
    pub(crate) logical_interests: WIPOffset<
        flatbuffers::Vector<'b, flatbuffers::ForwardsUOffset<fb::LogicalInterestStatus<'b>>>,
    >,
    pub(crate) wire_subscriptions: WIPOffset<
        flatbuffers::Vector<'b, flatbuffers::ForwardsUOffset<fb::WireSubscriptionStatus<'b>>>,
    >,
    pub(crate) logs: WIPOffset<flatbuffers::Vector<'b, flatbuffers::ForwardsUOffset<&'b str>>>,
    pub(crate) last_error_toast: Option<WIPOffset<&'b str>>,
    pub(crate) last_error_category: Option<WIPOffset<&'b str>>,
    pub(crate) last_planner_error: Option<WIPOffset<&'b str>>,
    pub(crate) store_open_failure: Option<WIPOffset<&'b str>>,
    pub(crate) no_configured_relays: Option<bool>,
    pub(crate) negentropy_sync_stats: WIPOffset<fb::NegentropySyncStats<'b>>,
}

impl KernelSnapshot {
    /// Build the typed Tier-3 envelope offsets from this snapshot's fields.
    ///
    /// Populated entirely from the struct (never from a re-walk of the JSON
    /// `payload`), so the typed and JSON representations are independent
    /// encodings of the same source state.
    pub(crate) fn encode_tier3<'b>(
        &self,
        builder: &mut FlatBufferBuilder<'b>,
    ) -> Tier3Offsets<'b> {
        // Nested tables / vectors first (FlatBuffers builds inner offsets before
        // the table that references them).
        let negentropy_sync_stats =
            encode_negentropy_sync_stats(builder, &self.negentropy_sync_stats);
        let metrics = encode_metrics(builder, &self.metrics);
        let relay_status = encode_relay_status(builder, &self.relay_status);
        let relay_statuses = encode_relay_statuses(builder, &self.relay_statuses);
        let logical_interests = encode_logical_interests(builder, &self.logical_interests);
        let wire_subscriptions = encode_wire_subscriptions(builder, &self.wire_subscriptions);

        let log_offsets: Vec<_> = self
            .logs
            .iter()
            .map(|line| builder.create_string(line))
            .collect();
        let logs = builder.create_vector(&log_offsets);

        let update_kind = builder.create_string(self.update_kind);
        let last_error_toast = create_opt_string(builder, self.last_error_toast.as_deref());
        let last_error_category = create_opt_string(builder, self.last_error_category.as_deref());
        let last_planner_error = create_opt_string(builder, self.last_planner_error.as_deref());
        let store_open_failure = create_opt_string(builder, self.store_open_failure.as_deref());

        Tier3Offsets {
            rev: self.rev,
            kernel_schema_version: self.schema_version,
            last_tick_ms: self.last_tick_ms,
            update_kind,
            running: self.running,
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
            no_configured_relays: self.no_configured_relays,
            negentropy_sync_stats,
        }
    }
}

/// Create a string offset only when `Some`; `None` leaves the field absent on
/// the wire (FlatBuffers reads it back as null). Encodes the "absent = healthy /
/// not-active" contract for the `Option<String>` diagnostic fields.
fn create_opt_string<'b>(
    builder: &mut FlatBufferBuilder<'b>,
    value: Option<&str>,
) -> Option<WIPOffset<&'b str>> {
    value.map(|text| builder.create_string(text))
}

fn encode_metrics<'b>(
    builder: &mut FlatBufferBuilder<'b>,
    metrics: &Metrics,
) -> WIPOffset<fb::Metrics<'b>> {
    fb::Metrics::create(
        builder,
        &fb::MetricsArgs {
            generated_events: metrics.generated_events,
            note_events: metrics.note_events,
            profile_events: metrics.profile_events,
            duplicate_events: metrics.duplicate_events,
            delete_events: metrics.delete_events,
            stored_events: metrics.stored_events as u64,
            tombstones: metrics.tombstones as u64,
            visible_items: metrics.visible_items as u64,
            visible_profiled_items: metrics.visible_profiled_items as u64,
            visible_placeholder_avatar_items: metrics.visible_placeholder_avatar_items as u64,
            open_views: metrics.open_views,
            events_since_last_update: metrics.events_since_last_update,
            diagnostic_firehose_events: metrics.diagnostic_firehose_events,
            inserted_count: metrics.inserted_count as u64,
            updated_count: metrics.updated_count as u64,
            removed_count: metrics.removed_count as u64,
            events_per_second_configured: metrics.events_per_second_configured,
            emit_hz_configured: metrics.emit_hz_configured,
            update_sequence: metrics.update_sequence,
            estimated_store_bytes: metrics.estimated_store_bytes as u64,
            payload_bytes: metrics.payload_bytes as u64,
            store_to_payload_ratio: metrics.store_to_payload_ratio,
            actor_queue_depth: metrics.actor_queue_depth,
            frames_rx: metrics.frames_rx,
            events_rx: metrics.events_rx,
            eose_rx: metrics.eose_rx,
            notices_rx: metrics.notices_rx,
            closed_rx: metrics.closed_rx,
            bytes_rx: metrics.bytes_rx,
            bytes_tx: metrics.bytes_tx,
            contacts_authors: metrics.contacts_authors as u64,
            timeline_authors: metrics.timeline_authors as u64,
            first_event_ms: opt_u64_sat(metrics.first_event_ms),
            target_profile_loaded_ms: opt_u64_sat(metrics.target_profile_loaded_ms),
            timeline_opened_ms: opt_u64_sat(metrics.timeline_opened_ms),
            timeline_first_item_ms: opt_u64_sat(metrics.timeline_first_item_ms),
            update_emitted_ms: opt_u64_sat(metrics.update_emitted_ms),
            last_event_to_emit_ms: opt_u64_sat(metrics.last_event_to_emit_ms),
            max_event_to_emit_ms: u64_sat(metrics.max_event_to_emit_ms),
            max_events_per_update: metrics.max_events_per_update,
            dispatch_drops_total: metrics.dispatch_drops_total,
            claim_drops_total: metrics.claim_drops_total,
            make_update_us: u64_sat(metrics.make_update_us),
            serialize_us: u64_sat(metrics.serialize_us),
            update_frame_degradations_total: metrics.update_frame_degradations_total,
        },
    )
}

fn encode_relay_status<'b>(
    builder: &mut FlatBufferBuilder<'b>,
    status: &RelayStatus,
) -> WIPOffset<fb::RelayStatus<'b>> {
    let role = builder.create_string(&status.role);
    let relay_url = builder.create_string(&status.relay_url);
    let connection = builder.create_string(&status.connection);
    let auth = builder.create_string(&status.auth);
    let negentropy_probe = builder.create_string(&status.negentropy_probe);
    let last_notice = create_opt_string(builder, status.last_notice.as_deref());
    let last_error = create_opt_string(builder, status.last_error.as_deref());
    let error_category = create_opt_string(builder, status.error_category.as_deref());
    let last_close_reason = create_opt_string(builder, status.last_close_reason.as_deref());
    fb::RelayStatus::create(
        builder,
        &fb::RelayStatusArgs {
            role: Some(role),
            relay_url: Some(relay_url),
            connection: Some(connection),
            auth: Some(auth),
            negentropy_probe: Some(negentropy_probe),
            active_wire_subscriptions: status.active_wire_subscriptions as u64,
            reconnect_count: status.reconnect_count,
            last_connected_at_ms: opt_u64_sat(status.last_connected_at_ms),
            last_event_at_ms: opt_u64_sat(status.last_event_at_ms),
            last_notice,
            last_error,
            error_category,
            events_rx: status.events_rx,
            bytes_rx: status.bytes_rx,
            bytes_tx: status.bytes_tx,
            denied: status.denied,
            last_close_reason,
        },
    )
}

fn encode_relay_statuses<'b>(
    builder: &mut FlatBufferBuilder<'b>,
    statuses: &[RelayStatus],
) -> WIPOffset<flatbuffers::Vector<'b, flatbuffers::ForwardsUOffset<fb::RelayStatus<'b>>>> {
    let offsets: Vec<_> = statuses
        .iter()
        .map(|status| encode_relay_status(builder, status))
        .collect();
    builder.create_vector(&offsets)
}

fn encode_logical_interests<'b>(
    builder: &mut FlatBufferBuilder<'b>,
    interests: &[LogicalInterestStatus],
) -> WIPOffset<flatbuffers::Vector<'b, flatbuffers::ForwardsUOffset<fb::LogicalInterestStatus<'b>>>>
{
    let offsets: Vec<_> = interests
        .iter()
        .map(|interest| {
            let key = builder.create_string(&interest.key);
            let state = builder.create_string(&interest.state);
            let cache_coverage = builder.create_string(&interest.cache_coverage);
            let url_offsets: Vec<_> = interest
                .relay_urls
                .iter()
                .map(|url| builder.create_string(url))
                .collect();
            let relay_urls = builder.create_vector(&url_offsets);
            fb::LogicalInterestStatus::create(
                builder,
                &fb::LogicalInterestStatusArgs {
                    key: Some(key),
                    state: Some(state),
                    refcount: interest.refcount,
                    relay_urls: Some(relay_urls),
                    cache_coverage: Some(cache_coverage),
                    warming_until_ms: opt_u64_sat(interest.warming_until_ms),
                },
            )
        })
        .collect();
    builder.create_vector(&offsets)
}

fn encode_wire_subscriptions<'b>(
    builder: &mut FlatBufferBuilder<'b>,
    subscriptions: &[WireSubscriptionStatus],
) -> WIPOffset<flatbuffers::Vector<'b, flatbuffers::ForwardsUOffset<fb::WireSubscriptionStatus<'b>>>>
{
    let offsets: Vec<_> = subscriptions
        .iter()
        .map(|sub| {
            let wire_id = builder.create_string(&sub.wire_id);
            let relay_url = builder.create_string(&sub.relay_url);
            let filter_summary = builder.create_string(&sub.filter_summary);
            let state = builder.create_string(&sub.state);
            let close_reason = create_opt_string(builder, sub.close_reason.as_deref());
            fb::WireSubscriptionStatus::create(
                builder,
                &fb::WireSubscriptionStatusArgs {
                    wire_id: Some(wire_id),
                    relay_url: Some(relay_url),
                    filter_summary: Some(filter_summary),
                    state: Some(state),
                    logical_consumer_count: sub.logical_consumer_count,
                    events_rx: sub.events_rx,
                    opened_at_ms: u64_sat(sub.opened_at_ms),
                    last_event_at_ms: opt_u64_sat(sub.last_event_at_ms),
                    eose_at_ms: opt_u64_sat(sub.eose_at_ms),
                    close_reason,
                },
            )
        })
        .collect();
    builder.create_vector(&offsets)
}

fn encode_negentropy_sync_stats<'b>(
    builder: &mut FlatBufferBuilder<'b>,
    stats: &NegentropySyncStats,
) -> WIPOffset<fb::NegentropySyncStats<'b>> {
    fb::NegentropySyncStats::create(
        builder,
        &fb::NegentropySyncStatsArgs {
            rounds: stats.rounds,
            have_ids: stats.have_ids,
            need_ids: stats.need_ids,
            local_item_count: stats.local_item_count,
            transfer_avoided_bytes: stats.transfer_avoided_bytes,
            last_reconcile_at_ms: stats.last_reconcile_at_ms,
        },
    )
}
