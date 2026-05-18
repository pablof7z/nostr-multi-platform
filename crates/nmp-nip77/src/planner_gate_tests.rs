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
        pin_to: None,
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
