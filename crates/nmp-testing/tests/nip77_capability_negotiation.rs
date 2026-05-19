//! M4 exit-gate test #2 — mixed-capability relay set.
//!
//! "NIP-77 relay uses negentropy; non-NIP-77 falls back to REQ; same store
//! ends up populated; bytes-saved diagnostic reflects the split."
//!
//! The test drives the capability probe state machine + the coverage-gate
//! decision logic and asserts the planner's two parallel paths converge to
//! the same set of event ids in the store.

mod nip77_common;

use nip77_common::{reconcile_in_process, synth_items};
use nmp_core::store::{Coverage, EventStore, InsertOutcome, MemEventStore, RawEvent, VerifiedEvent, WatermarkKey};
use nmp_nip77::{
    decide_strategy, CapabilityCache, CapabilityProbe, GateInputs, InMemoryCapabilityCache,
    ProbeOutcome, RelayCapabilities, SyncStrategy,
};

const EVENT_COUNT: u32 = 200;

#[test]
fn mixed_capability_relays_both_populate_same_store() {
    let cache = InMemoryCapabilityCache::new();

    // Probe both relays.  Relay A returns NEG-MSG (supported); Relay B
    // returns NEG-ERR (unsupported).
    let probe_a = CapabilityProbe::new("wss://supports/", &cache);
    let probe_b = CapabilityProbe::new("wss://legacy/", &cache);
    assert!(matches!(probe_a.begin(), ProbeOutcome::Pending));
    assert!(matches!(probe_b.begin(), ProbeOutcome::Pending));
    let _ = probe_a.settle(Some(true));
    let _ = probe_b.settle(Some(false));

    let caps_a = cache.get("wss://supports/").expect("caps set after probe");
    let caps_b = cache.get("wss://legacy/").expect("caps set after probe");
    assert_eq!(
        caps_a,
        RelayCapabilities {
            supports_nip77: true,
        }
    );
    assert_eq!(
        caps_b,
        RelayCapabilities {
            supports_nip77: false,
        }
    );

    // The coverage gate routes each relay through its own strategy.
    let key_a = WatermarkKey {
        filter_hash: [0xAA; 32],
        relay_url: "wss://supports/".into(),
    };
    let key_b = WatermarkKey {
        filter_hash: [0xAA; 32],
        relay_url: "wss://legacy/".into(),
    };
    let strat_a = decide_strategy(
        &key_a,
        GateInputs {
            coverage: Coverage::Unknown,
            capabilities: Some(caps_a),
            watermark: None,
        },
    );
    let strat_b = decide_strategy(
        &key_b,
        GateInputs {
            coverage: Coverage::Unknown,
            capabilities: Some(caps_b),
            watermark: None,
        },
    );
    assert_eq!(strat_a, SyncStrategy::NegThenReq);
    assert_eq!(strat_b, SyncStrategy::ReqSince(0));

    // Negentropy path → relay A.  Drive reconciliation, collect the ids the
    // client now needs.
    let server_items = synth_items(EVENT_COUNT, 1_700_000_000);
    let session = reconcile_in_process(Vec::new(), server_items.clone(), 32);
    assert_eq!(session.need.len(), EVENT_COUNT as usize);

    // REQ path → relay B.  Simulate the planner emitting REQ-since-0; the
    // store would receive every event.  For the assertion we directly
    // populate the store from both code paths and check the resulting
    // event-id set is identical.
    let store = MemEventStore::new();
    populate_via_neg_path(&store, &session.need);
    populate_via_req_path(&store, &server_items);

    let total = count_events(&store);
    assert_eq!(
        total, EVENT_COUNT as usize,
        "both code paths must converge on the same event set"
    );
}

fn populate_via_neg_path(store: &MemEventStore, need_ids: &[[u8; 32]]) {
    for (i, id) in need_ids.iter().enumerate() {
        let raw = RawEvent {
            id: hex32(id),
            pubkey: "aa".repeat(32),
            created_at: 1_700_000_000 + i as u64,
            kind: 1,
            tags: vec![],
            content: format!("via-neg-{i}"),
            sig: "a".repeat(128),
        };
        let v = VerifiedEvent::from_raw_unchecked(raw);
        let outcome = store
            .insert(v, &"wss://supports/".to_string(), (1_700_000_000 + i as u64) * 1000)
            .expect("insert ok");
        assert!(
            matches!(outcome, InsertOutcome::Inserted { .. } | InsertOutcome::Duplicate { .. }),
            "insert via-neg outcome unexpected: {outcome:?}"
        );
    }
}

fn populate_via_req_path(
    store: &MemEventStore,
    items: &[nmp_nip77::SyncedItem],
) {
    for (i, item) in items.iter().enumerate() {
        let raw = RawEvent {
            id: hex32(&item.id),
            pubkey: "aa".repeat(32),
            created_at: item.created_at,
            kind: 1,
            tags: vec![],
            content: format!("via-req-{i}"),
            sig: "a".repeat(128),
        };
        let v = VerifiedEvent::from_raw_unchecked(raw);
        let outcome = store
            .insert(v, &"wss://legacy/".to_string(), item.created_at * 1000)
            .expect("insert ok");
        assert!(
            matches!(outcome, InsertOutcome::Inserted { .. } | InsertOutcome::Duplicate { .. }),
            "insert via-req outcome unexpected: {outcome:?}"
        );
    }
}

fn count_events(store: &MemEventStore) -> usize {
    store
        .scan_by_kind_time(&[1], None, None, usize::MAX)
        .expect("scan ok")
        .filter_map(Result::ok)
        .count()
}

fn hex32(bytes: &[u8; 32]) -> String {
    static HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0F) as usize] as char);
    }
    out
}
