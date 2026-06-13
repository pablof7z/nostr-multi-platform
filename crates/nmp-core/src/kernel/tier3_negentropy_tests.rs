//! GAP-5 negentropy session stats — Tier-3 round-trip tests.
//!
//! Verifies that `set_negentropy_sync_stats` → `make_update` → FlatBuffers
//! encode → typed `SnapshotFrame` accessor produces the correct values.

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::transport::wire as fb;

fn with_snapshot_frame<R>(bytes: &[u8], f: impl FnOnce(fb::SnapshotFrame<'_>) -> R) -> R {
    assert!(
        fb::update_frame_buffer_has_identifier(bytes),
        "frame must carry the NMPU identifier"
    );
    let frame = fb::root_as_update_frame(bytes).expect("decode update frame");
    assert_eq!(frame.kind(), fb::FrameKind::Snapshot, "expected a snapshot frame");
    let snapshot = frame.snapshot().expect("snapshot frame present");
    f(snapshot)
}

/// GAP-5: when `set_negentropy_sync_stats` is called, the typed Tier-3
/// `SnapshotFrame.negentropy_sync_stats` table carries the exact values. Tests
/// the full encode → decode round-trip: Rust struct → FlatBuffers → accessor.
#[test]
fn gap5_negentropy_sync_stats_round_trips_through_tier3() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_negentropy_sync_stats(3, 7, 2, 10);

    let (bytes, json) = kernel.make_update_frame_and_json_for_test(true);

    // JSON representation (Serialize on NegentropySyncStats).
    let json_stats = &json["negentropy_sync_stats"];
    assert_eq!(json_stats["rounds"].as_u64(), Some(3), "JSON rounds");
    assert_eq!(json_stats["have_ids"].as_u64(), Some(7), "JSON have_ids");
    assert_eq!(json_stats["need_ids"].as_u64(), Some(2), "JSON need_ids");
    assert_eq!(json_stats["local_item_count"].as_u64(), Some(10), "JSON local_item_count");
    // transfer_avoided_bytes = (10 - 7) * 512 = 1536
    assert_eq!(
        json_stats["transfer_avoided_bytes"].as_u64(),
        Some(1536),
        "JSON transfer_avoided_bytes = (local - have) × AVG_EVENT_BYTES"
    );
    assert!(
        json_stats.get("last_reconcile_at_ms").is_some(),
        "last_reconcile_at_ms must be present after a session"
    );

    // Typed FlatBuffers representation.
    with_snapshot_frame(&bytes, |frame| {
        let stats = frame
            .negentropy_sync_stats()
            .expect("negentropy_sync_stats table must be present");
        assert_eq!(stats.rounds(), 3, "FlatBuffers rounds");
        assert_eq!(stats.have_ids(), 7, "FlatBuffers have_ids");
        assert_eq!(stats.need_ids(), 2, "FlatBuffers need_ids");
        assert_eq!(stats.local_item_count(), 10, "FlatBuffers local_item_count");
        assert_eq!(
            stats.transfer_avoided_bytes(),
            1536,
            "FlatBuffers transfer_avoided_bytes = (local - have) × AVG_EVENT_BYTES"
        );
        assert!(
            stats.last_reconcile_at_ms().is_some(),
            "FlatBuffers last_reconcile_at_ms must be present after a session"
        );
    });
}

/// GAP-5: on a fresh kernel (no session completed), `negentropy_sync_stats`
/// is present on the wire (always emitted) with all-zero fields.
#[test]
fn gap5_negentropy_sync_stats_default_zeros_on_fresh_kernel() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (bytes, _json) = kernel.make_update_frame_and_json_for_test(true);

    with_snapshot_frame(&bytes, |frame| {
        let stats = frame
            .negentropy_sync_stats()
            .expect("negentropy_sync_stats table always present");
        assert_eq!(stats.rounds(), 0, "fresh kernel: rounds == 0");
        assert_eq!(stats.have_ids(), 0, "fresh kernel: have_ids == 0");
        assert_eq!(stats.need_ids(), 0, "fresh kernel: need_ids == 0");
        assert_eq!(stats.local_item_count(), 0, "fresh kernel: local_item_count == 0");
        assert_eq!(
            stats.transfer_avoided_bytes(),
            0,
            "fresh kernel: transfer_avoided_bytes == 0"
        );
        assert_eq!(
            stats.last_reconcile_at_ms(),
            None,
            "fresh kernel: last_reconcile_at_ms == None (no session yet)"
        );
    });
}
