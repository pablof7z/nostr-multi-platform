use crate::types::{InsertOutcome, RawEvent, VerifiedEvent};
use crate::{EventStore, MemEventStore};

fn unchecked(raw: RawEvent) -> VerifiedEvent {
    VerifiedEvent::from_raw_unchecked(raw)
}

/// P2: tombstone upsert must max-merge `deleted_at` and union sources.
#[test]
fn tombstone_max_merge_takes_newer_deleted_at() {
    let store = MemEventStore::new();
    let target_hex = "0a".repeat(32);
    let k5a_hex = "a1".repeat(32);
    let k5b_hex = "b2".repeat(32);

    // First kind:5 at t=100.
    let k5a = RawEvent {
        id: k5a_hex.clone(),
        pubkey: "01".repeat(32),
        created_at: 100,
        kind: 5,
        tags: vec![vec!["e".into(), target_hex.clone()]],
        content: String::new(),
        sig: "a".repeat(128),
    };
    store
        .insert(unchecked(k5a), &"wss://r1/".to_string(), 100_000)
        .unwrap();

    // Second kind:5 at t=200 (newer — should win for deleted_at).
    let k5b = RawEvent {
        id: k5b_hex.clone(),
        pubkey: "01".repeat(32),
        created_at: 200,
        kind: 5,
        tags: vec![vec!["e".into(), target_hex.clone()]],
        content: String::new(),
        sig: "a".repeat(128),
    };
    store
        .insert(unchecked(k5b), &"wss://r2/".to_string(), 200_000)
        .unwrap();

    let st = store.state.lock().unwrap();
    let tomb = st
        .tombstones
        .get(&target_hex)
        .expect("tombstone must exist");
    assert_eq!(
        tomb.deleted_at, 200,
        "max-merge must take the newer deleted_at"
    );
    assert!(
        tomb.sources.contains(&"wss://r1/".to_string()),
        "must union r1"
    );
    assert!(
        tomb.sources.contains(&"wss://r2/".to_string()),
        "must union r2"
    );
}

/// P2: same-id re-delivery for replaceable events must merge provenance,
/// not count as a new supersession.
#[test]
fn replaceable_dup_id_merges_provenance() {
    let store = MemEventStore::new();
    let pk = "01".repeat(32);
    let id = "aa".repeat(32);
    let ev = RawEvent {
        id: id.clone(),
        pubkey: pk.clone(),
        created_at: 1000,
        kind: 0, // replaceable
        tags: vec![],
        content: String::new(),
        sig: "a".repeat(128),
    };

    let o1 = store
        .insert(unchecked(ev.clone()), &"wss://r1/".to_string(), 1_000_000)
        .unwrap();
    assert!(matches!(o1, InsertOutcome::Inserted { .. }));

    let o2 = store
        .insert(unchecked(ev), &"wss://r2/".to_string(), 2_000_000)
        .unwrap();
    assert!(
        matches!(o2, InsertOutcome::Duplicate { .. }),
        "re-delivery of same id must be Duplicate, got {o2:?}"
    );

    let id_bytes = [0xaau8; 32];
    let prov = store.provenance_for(&id_bytes).unwrap();
    assert_eq!(prov.len(), 2, "both relays must be in provenance");
}
