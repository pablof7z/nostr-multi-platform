//! §2.9 Watermarks tests.
//!
//! See `docs/design/lmdb/tests.md` §2.9.

use nmp_core::store::{Coverage, SyncMethod, WatermarkKey, WatermarkRow};
use nmp_testing::for_each_backend;
use nmp_testing::store_harness::StoreHarness;

fn make_key(relay: &str) -> WatermarkKey {
    WatermarkKey {
        filter_hash: [0xab; 32],
        relay_url: relay.to_string(),
    }
}

fn make_row(relay: &str, synced_up_to: u64, updated_at: u64) -> WatermarkRow {
    WatermarkRow {
        key: make_key(relay),
        synced_up_to,
        last_sync_method: SyncMethod::ReqScan,
        last_negentropy_state: None,
        bytes_saved_vs_req: 0,
        updated_at,
    }
}

for_each_backend!(watermark_write_read_roundtrip, |h: &mut StoreHarness| {
    let row = make_row("wss://a/", 1_000_000, 2_000_000);
    h.store.write_watermark(row.clone()).unwrap();

    let read = h.store.read_watermark(&make_key("wss://a/")).unwrap();
    assert!(read.is_some(), "written watermark should be readable");
    let read = read.unwrap();
    assert_eq!(read.synced_up_to, row.synced_up_to);
    assert_eq!(read.key.relay_url, "wss://a/");
});

for_each_backend!(missing_watermark_returns_none, |h: &mut StoreHarness| {
    let result = h.store.read_watermark(&make_key("wss://missing/")).unwrap();
    assert!(result.is_none());
});

for_each_backend!(
    coverage_unknown_for_missing_watermark,
    |h: &mut StoreHarness| {
        let cov = h.store.coverage(&make_key("wss://x/")).unwrap();
        assert_eq!(cov, Coverage::Unknown);
    }
);

for_each_backend!(
    coverage_complete_for_fresh_watermark,
    |h: &mut StoreHarness| {
        // Write a watermark with updated_at = now (within 300s staleness window).
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let row = make_row("wss://a/", now - 10, now);
        h.store.write_watermark(row).unwrap();

        let cov = h.store.coverage(&make_key("wss://a/")).unwrap();
        assert!(
            matches!(cov, Coverage::CompleteAsOf(_)),
            "fresh watermark should be CompleteAsOf, got {cov:?}"
        );
    }
);

for_each_backend!(
    coverage_partial_for_stale_watermark,
    |h: &mut StoreHarness| {
        // Write a watermark with updated_at well in the past (> 300s staleness window).
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let stale_updated_at = now - 600; // 10 minutes ago
        let row = make_row("wss://a/", stale_updated_at - 60, stale_updated_at);
        h.store.write_watermark(row).unwrap();

        let cov = h.store.coverage(&make_key("wss://a/")).unwrap();
        assert!(
            matches!(cov, Coverage::PartialUpTo(_)),
            "stale watermark should be PartialUpTo, got {cov:?}"
        );
    }
);

for_each_backend!(
    list_watermarks_for_relay_filters_correctly,
    |h: &mut StoreHarness| {
        // Write watermarks for two different relays.
        let key_a = WatermarkKey {
            filter_hash: [0x01; 32],
            relay_url: "wss://a/".into(),
        };
        let key_b = WatermarkKey {
            filter_hash: [0x02; 32],
            relay_url: "wss://b/".into(),
        };

        h.store
            .write_watermark(WatermarkRow {
                key: key_a,
                synced_up_to: 100,
                last_sync_method: SyncMethod::Manual,
                last_negentropy_state: None,
                bytes_saved_vs_req: 0,
                updated_at: 1_000,
            })
            .unwrap();
        h.store
            .write_watermark(WatermarkRow {
                key: key_b,
                synced_up_to: 200,
                last_sync_method: SyncMethod::Negentropy,
                last_negentropy_state: None,
                bytes_saved_vs_req: 0,
                updated_at: 2_000,
            })
            .unwrap();

        let rows_a: Vec<_> = h
            .store
            .list_watermarks_for_relay("wss://a/")
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(rows_a.len(), 1);
        assert_eq!(rows_a[0].synced_up_to, 100);

        let rows_b: Vec<_> = h
            .store
            .list_watermarks_for_relay("wss://b/")
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(rows_b.len(), 1);
        assert_eq!(rows_b[0].synced_up_to, 200);
    }
);
