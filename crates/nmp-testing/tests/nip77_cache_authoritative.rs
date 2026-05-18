//! M4 exit-gate test #4 — authoritative cache miss.
//!
//! "Cache-miss against a fully-synced (filter, relay) pair answers
//! authoritatively (no fallback fetch)." — `docs/plan/m4-negentropy.md`
//!
//! Concretely: when [`coverage`] returns `CompleteAsOf(_)` and the store has
//! no event for the requested id, the coverage gate must return
//! [`SyncStrategy::SkipReq`].  No wire traffic.  No REQ.  Period.

use nmp_core::store::{Coverage, EventStore, MemEventStore, SyncMethod, WatermarkKey, WatermarkRow};
use nmp_nip77::{
    decide_strategy, GateInputs, RelayCapabilities, SyncStrategy,
};

#[test]
fn cache_miss_against_fully_synced_pair_is_authoritative() {
    let store = MemEventStore::new();
    let now_s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let key = WatermarkKey {
        filter_hash: [0xEE; 32],
        relay_url: "wss://r/".into(),
    };
    store
        .write_watermark(WatermarkRow {
            key: key.clone(),
            synced_up_to: now_s,
            last_sync_method: SyncMethod::Negentropy,
            last_negentropy_state: None,
            bytes_saved_vs_req: 0,
            updated_at: now_s,
        })
        .expect("write watermark");

    let coverage = store.coverage(&key).expect("coverage");
    assert!(
        matches!(coverage, Coverage::CompleteAsOf(_)),
        "fresh watermark should report CompleteAsOf, got {coverage:?}"
    );

    let strategy = decide_strategy(
        &key,
        GateInputs {
            coverage,
            capabilities: Some(RelayCapabilities {
                supports_nip77: true,
            }),
            watermark: store.read_watermark(&key).unwrap(),
        },
    );
    assert_eq!(
        strategy,
        SyncStrategy::SkipReq,
        "fully-synced (filter, relay) pair must skip REQ entirely"
    );
    assert!(
        !strategy.issues_wire_traffic(),
        "SkipReq must not produce any wire traffic"
    );
}

#[test]
fn stale_watermark_does_not_yield_authoritative_skip() {
    // A `PartialUpTo` reading — what coverage returns when `updated_at` is
    // older than the 300 s staleness window — must NOT be treated as
    // authoritative.  This is the negative case that proves the gate isn't
    // returning `SkipReq` for the wrong reason.
    let store = MemEventStore::new();
    let now_s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let key = WatermarkKey {
        filter_hash: [0xEF; 32],
        relay_url: "wss://r/".into(),
    };
    store
        .write_watermark(WatermarkRow {
            key: key.clone(),
            synced_up_to: now_s.saturating_sub(1_000),
            last_sync_method: SyncMethod::Negentropy,
            last_negentropy_state: None,
            bytes_saved_vs_req: 0,
            updated_at: now_s.saturating_sub(1_000),
        })
        .expect("write watermark");

    let coverage = store.coverage(&key).expect("coverage");
    assert!(matches!(coverage, Coverage::PartialUpTo(_)));

    let strategy = decide_strategy(
        &key,
        GateInputs {
            coverage,
            capabilities: Some(RelayCapabilities {
                supports_nip77: true,
            }),
            watermark: store.read_watermark(&key).unwrap(),
        },
    );
    assert_ne!(strategy, SyncStrategy::SkipReq);
    assert!(strategy.issues_wire_traffic());
}
