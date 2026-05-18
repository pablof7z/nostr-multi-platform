//! Unit tests for [`crate::planner_gate`].  Sibling file so the production
//! module stays under the 300 LOC soft cap (AGENTS.md).

use crate::capability::{CapabilityCache, InMemoryCapabilityCache, RelayCapabilities};
use crate::planner_gate::apply_coverage_filter;
use nmp_core::planner::{
    CompiledPlan, InterestId, InterestShape, RelayPlan, RoutingSource, SubShape,
};
use nmp_core::store::{
    EventStore, MemEventStore, SyncMethod, WatermarkKey, WatermarkRow,
};
use std::collections::{BTreeMap, BTreeSet};

fn make_plan(relay: &str, since: Option<u64>) -> CompiledPlan {
    let shape = InterestShape {
        authors: BTreeSet::new(),
        kinds: BTreeSet::from([1]),
        tags: BTreeMap::new(),
        since,
        until: None,
        limit: None,
        event_ids: BTreeSet::new(),
        addresses: BTreeSet::new(),
        relay_pin: None,
    };
    let sub = SubShape {
        shape,
        originating_interests: vec![InterestId(7)],
        canonical_filter_hash: "00000001".into(),
    };
    let mut relays = BTreeMap::new();
    relays.insert(
        relay.to_string(),
        RelayPlan {
            relay_url: relay.to_string(),
            role_tags: BTreeSet::from([RoutingSource::Nip65]),
            sub_shapes: vec![sub],
        },
    );
    CompiledPlan {
        plan_id: "test".into(),
        per_relay: relays,
        unroutable_authors: BTreeSet::new(),
    }
}

fn canon_static(_s: &SubShape) -> [u8; 32] {
    [0xAA; 32]
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[test]
fn skip_req_removes_subshape_and_drops_empty_relay() {
    let store = MemEventStore::new();
    let caps = InMemoryCapabilityCache::new();
    let now_s = now();
    store
        .write_watermark(WatermarkRow {
            key: WatermarkKey {
                filter_hash: [0xAA; 32],
                relay_url: "wss://r/".into(),
            },
            synced_up_to: now_s,
            last_sync_method: SyncMethod::Negentropy,
            last_negentropy_state: None,
            bytes_saved_vs_req: 0,
            updated_at: now_s,
        })
        .unwrap();
    caps.set(
        "wss://r/",
        RelayCapabilities {
            supports_nip77: true,
        },
    );
    let mut plan = make_plan("wss://r/", None);
    let report = apply_coverage_filter(&mut plan, &store, &caps, canon_static);

    assert_eq!(report.count_skipped(), 1);
    assert!(plan.per_relay.is_empty(), "empty relays must be dropped");
}

#[test]
fn req_since_bumps_subshape_when_no_capability() {
    let store = MemEventStore::new();
    let caps = InMemoryCapabilityCache::new();
    store
        .write_watermark(WatermarkRow {
            key: WatermarkKey {
                filter_hash: [0xAA; 32],
                relay_url: "wss://legacy/".into(),
            },
            synced_up_to: 1_000,
            last_sync_method: SyncMethod::ReqScan,
            last_negentropy_state: None,
            bytes_saved_vs_req: 0,
            updated_at: 1_000, // stale ⇒ PartialUpTo
        })
        .unwrap();
    caps.set(
        "wss://legacy/",
        RelayCapabilities {
            supports_nip77: false,
        },
    );
    let mut plan = make_plan("wss://legacy/", None);
    let report = apply_coverage_filter(&mut plan, &store, &caps, canon_static);

    assert_eq!(report.count_bumped(), 1);
    let sub = &plan.per_relay["wss://legacy/"].sub_shapes[0];
    assert_eq!(sub.shape.since, Some(1_001));
}

#[test]
fn req_since_no_op_when_existing_since_already_newer() {
    let store = MemEventStore::new();
    let caps = InMemoryCapabilityCache::new();
    store
        .write_watermark(WatermarkRow {
            key: WatermarkKey {
                filter_hash: [0xAA; 32],
                relay_url: "wss://legacy/".into(),
            },
            synced_up_to: 500,
            last_sync_method: SyncMethod::ReqScan,
            last_negentropy_state: None,
            bytes_saved_vs_req: 0,
            updated_at: 500,
        })
        .unwrap();
    caps.set(
        "wss://legacy/",
        RelayCapabilities {
            supports_nip77: false,
        },
    );
    let mut plan = make_plan("wss://legacy/", Some(2_000));
    let _ = apply_coverage_filter(&mut plan, &store, &caps, canon_static);
    assert_eq!(
        plan.per_relay["wss://legacy/"].sub_shapes[0].shape.since,
        Some(2_000)
    );
}

#[test]
fn bumping_since_recomputes_canonical_filter_hash() {
    // P1 regression — the M4 codex review (076173d.md) flagged that
    // `apply_coverage_filter` mutated `shape.since` without refreshing the
    // sub-shape's `canonical_filter_hash`. Because the wire-emitter keys
    // sub-ids by that hash, the bumped REQ never reached the relay — the
    // diff treated old + bumped as identical.
    let store = MemEventStore::new();
    let caps = InMemoryCapabilityCache::new();
    store
        .write_watermark(WatermarkRow {
            key: WatermarkKey {
                filter_hash: [0xAA; 32],
                relay_url: "wss://legacy/".into(),
            },
            synced_up_to: 1_000,
            last_sync_method: SyncMethod::ReqScan,
            last_negentropy_state: None,
            bytes_saved_vs_req: 0,
            updated_at: 1_000,
        })
        .unwrap();
    caps.set(
        "wss://legacy/",
        RelayCapabilities {
            supports_nip77: false,
        },
    );

    // Seed both `canonical_filter_hash` and the compiler-equivalent hash for
    // the unmutated shape. Coverage gate must produce a different hash after
    // bumping `since`.
    let mut plan = make_plan("wss://legacy/", None);
    let hash_before = plan.per_relay["wss://legacy/"].sub_shapes[0]
        .canonical_filter_hash
        .clone();

    let report = apply_coverage_filter(&mut plan, &store, &caps, canon_static);
    assert_eq!(report.count_bumped(), 1, "expected ReqSince bump");

    let bumped_sub = &plan.per_relay["wss://legacy/"].sub_shapes[0];
    assert_eq!(
        bumped_sub.shape.since,
        Some(1_001),
        "since should advance past synced_up_to"
    );
    assert_ne!(
        bumped_sub.canonical_filter_hash, hash_before,
        "mutating `since` must invalidate the canonical filter hash so the \
         wire-emitter routes a new REQ frame to the relay"
    );

    // Cross-check: recomputing the hash from scratch matches the post-gate
    // value (i.e. the gate uses the same algorithm the compiler does).
    use nmp_core::planner::canonical_filter_hash as canon;
    assert_eq!(
        bumped_sub.canonical_filter_hash,
        canon(&bumped_sub.shape),
        "post-gate hash must equal the canonical hash of the mutated shape"
    );
}

#[test]
fn bumped_plan_diff_emits_close_and_req() {
    // Integration-flavoured P1 regression — model the actor's plan-emit path
    // (prior plan → coverage gate → next plan → plan_diff) and assert that the
    // diff is non-empty when the gate bumps `since`. Without the
    // `recompute_hash` fix this would return an empty diff and the relay
    // would silently keep its stale REQ.
    let store = MemEventStore::new();
    let caps = InMemoryCapabilityCache::new();
    store
        .write_watermark(WatermarkRow {
            key: WatermarkKey {
                filter_hash: [0xAA; 32],
                relay_url: "wss://legacy/".into(),
            },
            synced_up_to: 1_000,
            last_sync_method: SyncMethod::ReqScan,
            last_negentropy_state: None,
            bytes_saved_vs_req: 0,
            updated_at: 1_000,
        })
        .unwrap();
    caps.set(
        "wss://legacy/",
        RelayCapabilities {
            supports_nip77: false,
        },
    );

    let prior = make_plan("wss://legacy/", None);
    let mut next = prior.clone();
    let report = apply_coverage_filter(&mut next, &store, &caps, canon_static);
    assert_eq!(report.count_bumped(), 1);

    let frames = nmp_core::subs::plan_diff(Some(&prior), Some(&next), &[]);
    let closes = frames
        .iter()
        .filter(|f| matches!(f, nmp_core::subs::WireFrame::Close { .. }))
        .count();
    let reqs = frames
        .iter()
        .filter(|f| matches!(f, nmp_core::subs::WireFrame::Req { .. }))
        .count();
    assert_eq!(
        (closes, reqs),
        (1, 1),
        "bumped `since` must produce one CLOSE (old sub-id) and one REQ \
         (new sub-id); identical sub-ids would mean the relay keeps the stale \
         REQ. Frames observed: {frames:?}"
    );
}

#[test]
fn neg_then_req_keeps_subshape_unchanged() {
    let store = MemEventStore::new();
    let caps = InMemoryCapabilityCache::new();
    store
        .write_watermark(WatermarkRow {
            key: WatermarkKey {
                filter_hash: [0xAA; 32],
                relay_url: "wss://supports/".into(),
            },
            synced_up_to: 500,
            last_sync_method: SyncMethod::Negentropy,
            last_negentropy_state: None,
            bytes_saved_vs_req: 0,
            updated_at: 500,
        })
        .unwrap();
    caps.set(
        "wss://supports/",
        RelayCapabilities {
            supports_nip77: true,
        },
    );
    let mut plan = make_plan("wss://supports/", None);
    let _ = apply_coverage_filter(&mut plan, &store, &caps, canon_static);
    let sub = &plan.per_relay["wss://supports/"].sub_shapes[0];
    assert_eq!(sub.shape.since, None);
}
