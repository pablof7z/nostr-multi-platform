//! Framework Magic Contract — M4-gated test: C10.
//!
//! C10 Watermarks gate backfill; cache miss becomes authoritative; NIP-77 default.
//!
//! M4 (NIP-77 negentropy sync engine) is DONE on master.
//!
//! Design: `docs/design/framework-magic/sync.md` §C10.

use nmp_core::store::{Coverage, MemEventStore, SyncMethod, WatermarkKey, WatermarkRow};
use nmp_core::store::EventStore as _;
use nmp_nip77::{decide_strategy, GateInputs};
use nmp_nip77::capability::RelayCapabilities;

// ── C10 ───────────────────────────────────────────────────────────────────────

/// C10: Watermarks gate backfill; cache miss is authoritative when coverage
/// is complete; `decide_strategy` defaults to negentropy when the relay
/// supports it.
///
/// Four sub-properties:
/// 1. **No watermark → `Coverage::Unknown`** — the store reports `Unknown` for
///    a (filter, relay) pair with no persisted watermark, meaning the planner
///    must always backfill.
/// 2. **Watermark written → `Coverage::CompleteAsOf` or `PartialUpTo`** — after
///    recording a row the store classifies it as bounded coverage.
/// 3. **Authoritative cache miss** — `CompleteAsOf` coverage with a NIP-77
///    capable relay → `decide_strategy` returns `SkipReq` (cache is
///    authoritative; a miss means the event doesn't exist).
/// 4. **NIP-77 default** — when the relay reports negentropy capability and
///    coverage is `Unknown`, `decide_strategy` prefers `NegThenReq`.
///
/// Design: `docs/design/framework-magic/sync.md` §C10.
#[test]
fn c10_watermark_gates_backfill_and_authoritative_miss() {
    let store = MemEventStore::new();

    let key = WatermarkKey {
        filter_hash: [0xABu8; 32],
        relay_url: "wss://sync.example/".to_string(),
    };

    // --- 1. No watermark → Unknown ------------------------------------------
    let before = store.coverage(&key).expect("coverage before write");
    assert!(
        matches!(before, Coverage::Unknown),
        "no watermark must yield Unknown coverage: {before:?}"
    );

    // --- 2. Watermark written → bounded coverage ----------------------------
    // Use a recent updated_at so the coverage is CompleteAsOf (not stale).
    let now_s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let row = WatermarkRow {
        key: key.clone(),
        synced_up_to: 1_700_000_000,
        last_sync_method: SyncMethod::Negentropy,
        last_negentropy_state: Some(vec![0x01, 0x02, 0x03]),
        bytes_saved_vs_req: 12_000,
        updated_at: now_s,
    };
    store.write_watermark(row.clone()).expect("write watermark");

    let after = store.coverage(&key).expect("coverage after write");
    assert!(
        matches!(after, Coverage::CompleteAsOf(_) | Coverage::PartialUpTo(_)),
        "written watermark must yield bounded coverage: {after:?}"
    );
    // synced_up_to must round-trip.
    let synced_up_to = match after {
        Coverage::CompleteAsOf(ts) | Coverage::PartialUpTo(ts) => ts,
        Coverage::Unknown => panic!("unexpected Unknown after write"),
    };
    assert_eq!(synced_up_to, 1_700_000_000, "synced_up_to must round-trip");

    // --- 3. Authoritative cache miss ----------------------------------------
    // CompleteAsOf + negentropy-capable relay → SkipReq.
    // Pass Coverage::CompleteAsOf directly so the test is independent of
    // the staleness window in the MemEventStore implementation.
    let caps_with_neg = RelayCapabilities {
        supports_nip77: true,
    };
    let read_back = store.read_watermark(&key).expect("read").expect("present");
    let strategy_skip = decide_strategy(
        &key,
        GateInputs {
            coverage: Coverage::CompleteAsOf(1_700_000_000),
            capabilities: Some(caps_with_neg),
            watermark: Some(read_back.clone()),
        },
    );
    assert!(
        matches!(strategy_skip, nmp_nip77::SyncStrategy::SkipReq),
        "CompleteAsOf + nip77-capable relay must SkipReq (authoritative cache miss): {strategy_skip:?}"
    );

    // --- 4. NIP-77 default: Unknown + nip77-capable → NegThenReq -------------
    let key2 = WatermarkKey {
        filter_hash: [0xCDu8; 32],
        relay_url: "wss://sync.example/".to_string(),
    };
    let strategy_neg = decide_strategy(
        &key2,
        GateInputs {
            coverage: Coverage::Unknown,
            capabilities: Some(caps_with_neg),
            watermark: None,
        },
    );
    assert!(
        matches!(strategy_neg, nmp_nip77::SyncStrategy::NegThenReq),
        "Unknown + nip77-capable relay must default to NegThenReq: {strategy_neg:?}"
    );

    // Without negentropy capability → REQ scan from zero.
    let caps_no_neg = RelayCapabilities { supports_nip77: false };
    let strategy_req = decide_strategy(
        &key2,
        GateInputs {
            coverage: Coverage::Unknown,
            capabilities: Some(caps_no_neg),
            watermark: None,
        },
    );
    assert!(
        matches!(strategy_req, nmp_nip77::SyncStrategy::ReqSince(0)),
        "Unknown + no-neg relay must fall back to ReqSince(0): {strategy_req:?}"
    );
}
