use flatbuffers::FlatBufferBuilder;

use super::*;
use crate::transport::wire as fb;
use crate::update_envelope::{
    decode_snapshot_typed_projections, encode_snapshot_frame, SnapshotEnvelope,
};

fn row(key: &str, payload: &[u8], state: WireProjectionState) -> TypedProjectionData {
    TypedProjectionData {
        key: key.to_string(),
        schema_id: format!("{key}.schema"),
        schema_version: 1,
        file_identifier: "TEST".to_string(),
        payload: payload.to_vec(),
        projection_rev: 1,
        state,
    }
}

fn frame(session_id: u64, snapshot_epoch: u64, rows: &[TypedProjectionData]) -> Vec<u8> {
    encode_snapshot_frame(
        &SnapshotEnvelope {
            session_id,
            snapshot_epoch,
            update_kind: "ViewBatch".to_string(),
            ..Default::default()
        },
        rows,
    )
}

fn keys(bytes: &[u8]) -> Vec<String> {
    decode_snapshot_typed_projections(bytes)
        .expect("merged frame decodes")
        .into_iter()
        .map(|row| row.key)
        .collect()
}

#[test]
fn changed_rows_are_retained_when_next_frame_omits_them() {
    let mut cache = ProjectionMergeCache::default();
    let first = cache
        .merge_update_frame(&frame(
            10,
            0,
            &[row("profile", b"a", WireProjectionState::Changed)],
        ))
        .expect("first frame merges");
    assert_eq!(keys(&first), vec!["profile"]);

    let second = cache
        .merge_update_frame(&frame(10, 0, &[]))
        .expect("omitted frame merges");
    let rows = decode_snapshot_typed_projections(&second).expect("merged frame decodes");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, "profile");
    assert_eq!(rows[0].payload, b"a");
}

#[test]
fn extra_projections_are_output_only_not_retained() {
    let mut cache = ProjectionMergeCache::default();
    let first = cache
        .merge_update_frame_with_extra_projections(
            &frame(10, 0, &[row("profile", b"a", WireProjectionState::Changed)]),
            [row(
                "claimed_event_embeds",
                b"sidecar",
                WireProjectionState::Changed,
            )],
        )
        .expect("first frame merges");
    assert_eq!(keys(&first), vec!["claimed_event_embeds", "profile"]);

    let second = cache
        .merge_update_frame(&frame(10, 0, &[]))
        .expect("second frame merges");
    assert_eq!(keys(&second), vec!["profile"]);
}

#[test]
fn cleared_rows_remove_cached_projection() {
    let mut cache = ProjectionMergeCache::default();
    cache
        .merge_update_frame(&frame(
            10,
            0,
            &[row("profile", b"a", WireProjectionState::Changed)],
        ))
        .expect("first frame merges");
    let cleared = cache
        .merge_update_frame(&frame(
            10,
            0,
            &[row("profile", b"", WireProjectionState::Cleared)],
        ))
        .expect("clear frame merges");
    assert!(keys(&cleared).is_empty());
}

#[test]
fn identity_change_resets_cache_before_applying_rows() {
    let mut cache = ProjectionMergeCache::default();
    cache
        .merge_update_frame(&frame(
            10,
            0,
            &[row("profile", b"a", WireProjectionState::Changed)],
        ))
        .expect("first frame merges");
    let next_identity = cache
        .merge_update_frame(&frame(11, 0, &[]))
        .expect("identity bump merges");
    assert!(keys(&next_identity).is_empty());
}

/// Build a structurally-valid Snapshot frame carrying ONE malformed typed
/// projection (a `TypedProjection` with no `key`), stamped with the given
/// `(session_id, snapshot_epoch)`. `merge_update_frame` reads the identity,
/// then fails inside `decode_typed_projections` ("missing key"). This is the
/// "new identity + decode error" frame the transactional-commit guarantee
/// must survive.
fn malformed_frame_new_identity(session_id: u64, snapshot_epoch: u64) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();
    // A TypedProjection with `key: None` → decode_typed_projections returns
    // InvalidValue("missing key") AFTER the snapshot identity has been read.
    let schema_id = builder.create_string("x.schema");
    let payload = builder.create_vector(b"x");
    let typed_payload = fb::TypedPayload::create(
        &mut builder,
        &fb::TypedPayloadArgs {
            schema_id: Some(schema_id),
            schema_version: 1,
            file_identifier: None,
            payload: Some(payload),
        },
    );
    let projection = fb::TypedProjection::create(
        &mut builder,
        &fb::TypedProjectionArgs {
            key: None, // ← the malformation
            payload: Some(typed_payload),
            projection_rev: 1,
            state: fb::ProjectionPresenceState::Changed,
        },
    );
    let typed_projections = builder.create_vector(&[projection]);
    let snapshot = fb::SnapshotFrame::create(
        &mut builder,
        &fb::SnapshotFrameArgs {
            schema_version: 1,
            typed_projections: Some(typed_projections),
            snapshot_epoch,
            session_id,
            ..Default::default()
        },
    );
    let root = fb::UpdateFrame::create(
        &mut builder,
        &fb::UpdateFrameArgs {
            kind: fb::FrameKind::Snapshot,
            snapshot: Some(snapshot),
            panic: None,
        },
    );
    fb::finish_update_frame_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}

/// Fail-closed transactional merge (#2073): a malformed frame B carrying a NEW
/// `(session_id, snapshot_epoch)` must NOT poison the cache. It returns an
/// error (the caller degrades to last-good) AND leaves the cache exactly at
/// frame A's state, so a subsequent valid incremental frame C merges from A —
/// not from an empty/cleared baseline.
#[test]
fn malformed_new_identity_frame_does_not_poison_cache() {
    let mut cache = ProjectionMergeCache::default();

    // Frame A: a good baseline at identity (10, 0).
    let a = cache
        .merge_update_frame(&frame(
            10,
            0,
            &[row("profile", b"a", WireProjectionState::Changed)],
        ))
        .expect("frame A merges");
    assert_eq!(keys(&a), vec!["profile"]);

    // Frame B: malformed, carries a NEW identity (11, 0). Pre-fix this would
    // clear the cache and adopt identity (11, 0) BEFORE failing the row decode.
    let err = cache
        .merge_update_frame(&malformed_frame_new_identity(11, 0))
        .expect_err("malformed frame B must return a decode error");
    assert!(
        matches!(err, UpdateFrameDecodeError::InvalidValue(_)),
        "expected InvalidValue (missing key), got {err:?}"
    );

    // Frame C: a valid INCREMENTAL frame at the ORIGINAL identity (10, 0) that
    // omits "profile". If B had poisoned the cache (cleared it + adopted
    // identity 11), the cache would now be empty and "profile" would be gone.
    // With the transactional fix the cache is still A, so "profile" survives.
    let c = cache
        .merge_update_frame(&frame(10, 0, &[]))
        .expect("frame C merges");
    let rows = decode_snapshot_typed_projections(&c).expect("merged frame decodes");
    assert_eq!(rows.len(), 1, "profile must survive the malformed frame B");
    assert_eq!(rows[0].key, "profile");
    assert_eq!(rows[0].payload, b"a", "last-good payload not poisoned");
}

#[test]
fn merge_rewrite_preserves_non_projection_snapshot_fields() {
    let mut cache = ProjectionMergeCache::default();
    let merged = cache
        .merge_update_frame(&rich_frame(&[row(
            "profile",
            b"a",
            WireProjectionState::Changed,
        )]))
        .expect("rich frame merges");
    let snapshot = fb::root_as_update_frame(&merged)
        .expect("merged frame decodes")
        .snapshot()
        .expect("snapshot payload remains present");

    assert_eq!(snapshot.schema_version(), 7);
    assert_eq!(snapshot.rev(), 42);
    assert_eq!(snapshot.kernel_schema_version(), 9);
    assert_eq!(snapshot.last_tick_ms(), 1234);
    assert_eq!(snapshot.update_kind(), Some("RichFrame"));
    assert!(snapshot.running());
    assert_eq!(snapshot.no_configured_relays(), Some(true));
    assert_eq!(snapshot.snapshot_epoch(), 5);
    assert_eq!(snapshot.session_id(), 6);
    assert_eq!(snapshot.last_error_toast(), Some("toast"));
    assert_eq!(snapshot.last_error_category(), Some("category"));
    assert_eq!(snapshot.last_planner_error(), Some("planner"));
    assert_eq!(snapshot.store_open_failure(), Some("store failed"));

    assert_eq!(snapshot.metrics().expect("metrics").serialize_us(), 31);
    assert_eq!(
        snapshot.relay_status().expect("relay status").auth(),
        Some("nip42")
    );
    assert_eq!(snapshot.relay_statuses().expect("relay statuses").len(), 1);
    assert_eq!(
        snapshot
            .logical_interests()
            .expect("logical interests")
            .len(),
        1
    );
    assert_eq!(
        snapshot
            .wire_subscriptions()
            .expect("wire subscriptions")
            .len(),
        1
    );
    assert_eq!(snapshot.logs().expect("logs").len(), 2);

    let stats = snapshot.negentropy_sync_stats().expect("negentropy stats");
    assert_eq!(stats.rounds(), 3);
    assert_eq!(stats.last_reconcile_at_ms(), Some(8));
}

fn rich_frame(rows: &[TypedProjectionData]) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();
    let typed_projections = super::encode_typed_projections(&mut builder, rows);
    let update_kind = builder.create_string("RichFrame");
    let toast = builder.create_string("toast");
    let category = builder.create_string("category");
    let planner = builder.create_string("planner");
    let store_failure = builder.create_string("store failed");
    let logs = string_vector(&mut builder, &["first log", "second log"]);
    let metrics = metrics(&mut builder);
    let relay_status = relay_status(&mut builder);
    let relay_statuses = builder.create_vector(&[relay_status]);
    let logical_interests = logical_interests(&mut builder);
    let wire_subscriptions = wire_subscriptions(&mut builder);
    let negentropy_sync_stats = fb::NegentropySyncStats::create(
        &mut builder,
        &fb::NegentropySyncStatsArgs {
            rounds: 3,
            have_ids: 4,
            need_ids: 5,
            local_item_count: 6,
            transfer_avoided_bytes: 7,
            last_reconcile_at_ms: Some(8),
        },
    );
    let snapshot = fb::SnapshotFrame::create(
        &mut builder,
        &fb::SnapshotFrameArgs {
            schema_version: 7,
            typed_projections,
            rev: 42,
            kernel_schema_version: 9,
            last_tick_ms: 1234,
            update_kind: Some(update_kind),
            running: true,
            metrics: Some(metrics),
            relay_status: Some(relay_status),
            relay_statuses: Some(relay_statuses),
            logical_interests: Some(logical_interests),
            wire_subscriptions: Some(wire_subscriptions),
            logs: Some(logs),
            last_error_toast: Some(toast),
            last_error_category: Some(category),
            last_planner_error: Some(planner),
            store_open_failure: Some(store_failure),
            no_configured_relays: Some(true),
            negentropy_sync_stats: Some(negentropy_sync_stats),
            snapshot_epoch: 5,
            session_id: 6,
        },
    );
    let root = fb::UpdateFrame::create(
        &mut builder,
        &fb::UpdateFrameArgs {
            kind: fb::FrameKind::Snapshot,
            snapshot: Some(snapshot),
            panic: None,
        },
    );
    fb::finish_update_frame_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}

fn metrics<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
) -> flatbuffers::WIPOffset<fb::Metrics<'bldr>> {
    fb::Metrics::create(
        builder,
        &fb::MetricsArgs {
            generated_events: 1,
            note_events: 2,
            serialize_us: 31,
            update_frame_degradations_total: 32,
            store_to_payload_ratio: 21.0,
            ..Default::default()
        },
    )
}

fn relay_status<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
) -> flatbuffers::WIPOffset<fb::RelayStatus<'bldr>> {
    let role = builder.create_string("both");
    let relay_url = builder.create_string("wss://relay.example");
    let connection = builder.create_string("connected");
    let auth = builder.create_string("nip42");
    fb::RelayStatus::create(
        builder,
        &fb::RelayStatusArgs {
            role: Some(role),
            relay_url: Some(relay_url),
            connection: Some(connection),
            auth: Some(auth),
            active_wire_subscriptions: 2,
            reconnect_count: 3,
            last_connected_at_ms: Some(4),
            last_event_at_ms: Some(5),
            ..Default::default()
        },
    )
}

fn logical_interests<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
) -> flatbuffers::WIPOffset<
    flatbuffers::Vector<'bldr, flatbuffers::ForwardsUOffset<fb::LogicalInterestStatus<'bldr>>>,
> {
    let key = builder.create_string("logical");
    let state = builder.create_string("warming");
    let relay_urls = string_vector(builder, &["wss://relay.example"]);
    let cache_coverage = builder.create_string("warm");
    let row = fb::LogicalInterestStatus::create(
        builder,
        &fb::LogicalInterestStatusArgs {
            key: Some(key),
            state: Some(state),
            refcount: 2,
            relay_urls: Some(relay_urls),
            cache_coverage: Some(cache_coverage),
            warming_until_ms: Some(77),
        },
    );
    builder.create_vector(&[row])
}

fn wire_subscriptions<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
) -> flatbuffers::WIPOffset<
    flatbuffers::Vector<'bldr, flatbuffers::ForwardsUOffset<fb::WireSubscriptionStatus<'bldr>>>,
> {
    let wire_id = builder.create_string("wire");
    let relay_url = builder.create_string("wss://relay.example");
    let filter_summary = builder.create_string("kind:1");
    let state = builder.create_string("open");
    let row = fb::WireSubscriptionStatus::create(
        builder,
        &fb::WireSubscriptionStatusArgs {
            wire_id: Some(wire_id),
            relay_url: Some(relay_url),
            filter_summary: Some(filter_summary),
            state: Some(state),
            logical_consumer_count: 2,
            events_rx: 3,
            opened_at_ms: 4,
            last_event_at_ms: Some(5),
            eose_at_ms: Some(6),
            close_reason: None,
        },
    );
    builder.create_vector(&[row])
}

fn string_vector<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    values: &[&str],
) -> flatbuffers::WIPOffset<flatbuffers::Vector<'bldr, flatbuffers::ForwardsUOffset<&'bldr str>>> {
    let offsets: Vec<_> = values
        .iter()
        .map(|value| builder.create_string(value))
        .collect();
    builder.create_vector(&offsets)
}
