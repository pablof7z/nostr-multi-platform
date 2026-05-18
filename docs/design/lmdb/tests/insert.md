# LMDB tests: insert-path invariants (§2.1–2.7a)

> Sub-file of [`../tests.md`](../tests.md). Covers §7.1 insert invariants.

### 2.1 Insert API single path (§7.1 row "Insert API")

File: `crates/nmp-testing/tests/store_insert_path.rs`

```rust
for_each_backend!(insert_returns_insert_outcome, |h: &mut StoreHarness| {
    let event = h.signed(EventBuilder::text_note("hello", &[]));
    let outcome = h.store.insert(event.clone(), &"wss://t/".into(), 0).unwrap();
    assert!(matches!(outcome, InsertOutcome::Inserted { .. }));
    assert!(h.store.get_by_id(&event.id.to_bytes()).unwrap().is_some());
});
```

Plus a runtime-instrumented test enforcing that `insert()` is the only path that writes to the primary `events` sub-db. The test uses a `WriteCounting<S>` newtype that wraps any `EventStore` and intercepts every write; at teardown it asserts that all writes to the primary sub-db originated from `insert()` call frames (verified via a `AtomicBool` flag set on entry to `insert` and checked inside the write interceptor).

```rust
// crates/nmp-testing/tests/store_insert_path.rs
struct WriteCounting<S: EventStore> {
    inner: S,
    in_insert: Arc<AtomicBool>,
    illegal_primary_writes: Arc<AtomicUsize>,
}

// ...wraps every EventStore method; every put() to the primary sub-db checks
// in_insert; increments illegal_primary_writes if false.
for_each_backend!(only_insert_writes_primary, |h: &mut StoreHarness| {
    let wc = WriteCounting::wrap(h.take_store());
    // Exercise every non-insert method that could conceivably write.
    let _ = wc.delete_by_filter(DeleteFilter::ByIdList(vec![]));
    let _ = wc.gc_step(GcBudget { max_events_per_step: 0, max_duration_ms: 0 });
    assert_eq!(wc.illegal_primary_writes.load(Ordering::SeqCst), 0);
});
```

### 2.2 Signature verification (§7.1 row "Signature/delegation validity")

File: `crates/nmp-testing/tests/store_invalid_sig.rs`

Builds an event, mutates the signature, inserts. Expects `InsertOutcome::Rejected { reason: RejectReason::BadSignature }` and no row in primary, secondaries, provenance, or tombstones. Also tests a malformed NIP-26 delegation tag (rejects with `BadDelegation`).

### 2.3 Duplicate id → provenance merge (§7.1 row "Duplicate id")

File: `crates/nmp-testing/tests/store_provenance_merge.rs`

```rust
for_each_backend!(duplicate_merges_provenance_keeps_earliest, |h| {
    let ev = h.signed(EventBuilder::text_note("x", &[]));
    let o1 = h.store.insert(ev.clone(), &"wss://a/".into(), 1000).unwrap();
    let o2 = h.store.insert(ev.clone(), &"wss://b/".into(), 2000).unwrap();
    assert!(matches!(o1, InsertOutcome::Inserted { .. }));
    assert!(matches!(o2, InsertOutcome::Duplicate { sources_after: 2, .. }));
    let p = h.store.provenance_for(&ev.id.to_bytes()).unwrap();
    assert_eq!(p.len(), 2);
    let primary = p.iter().find(|e| e.primary).unwrap();
    assert_eq!(primary.relay_url, "wss://a/");
    assert_eq!(primary.first_seen_ms, 1000); // earliest preserved
});
```

### 2.4 Replaceable supersession (§7.1 row "Replaceable kinds")

File: `crates/nmp-testing/tests/store_replaceable.rs`

Inserts two kind:0 from same pubkey, second with later `created_at`. Asserts: `get_by_id(first_id)` returns None; `scan_by_author_kind(pk, &[0], None, None, 10)` returns one row; the row's id is the second. Tie-break test: two kind:0 with same `created_at` — keep the lexicographically smaller id.

### 2.5 Parameterized replaceable (§7.1 row "Parameterized replaceable")

File: `crates/nmp-testing/tests/store_param_replaceable.rs`

Insert two kind:30023 with same `(pubkey, d=foo)`, second newer; assert only the second is returned by `get_param_replaceable(pk, 30023, b"foo")`. Insert a third with same kind+pubkey but `d=bar` — assert both `foo` and `bar` are independently retrievable. Assert that a kind:30024 with `d=foo` (different kind) does not collide with the kind:30023.

### 2.6 Kind:5 self-delete + tombstone persistence (§7.1 row "Kind 5")

File: `crates/nmp-testing/tests/store_kind5_tombstone.rs`

- Insert kind:1 by Alice.
- Insert kind:5 by Alice referencing the kind:1 via `e` tag.
- Assert kind:1 gone from primary; tombstone row exists with `target_id == kind1.id`, `origin == Kind5`.
- Insert the same kind:1 again — assert `InsertOutcome::Tombstoned`, no primary row created.
- Restart store; repeat the re-insertion — assert tombstone persists across restart.

### 2.7 Foreign kind:5 ignored (§7.1 row "Kind 5" — foreign clause)

File: `crates/nmp-testing/tests/store_kind5_foreign.rs`

- Insert kind:1 by Alice.
- Insert kind:5 by Bob referencing Alice's kind:1.
- Assert: kind:1 is still present in primary (Bob can't delete Alice's event); the kind:5 event itself is stored (so other clients can see it); no tombstone row was written.

### 2.7a Kind:5 `a`-tag delete arriving before the target event

File: `crates/nmp-testing/tests/store_kind5_addr_tombstone.rs`

Tests the `tombstones_addr` sub-db path (see [`../keys.md`](../keys.md) §4.2):

```rust
for_each_backend!(a_tag_delete_before_event_suppresses_reinsert, |h: &mut StoreHarness| {
    let d_tag = "my-article";
    let addr = format!("30023:{}:{}", h.keys.public_key(), d_tag);
    let kind5 = h.signed(EventBuilder::new(Kind::from(5), "", vec![
        Tag::parse(vec!["a", &addr]).unwrap(),
    ]));
    let outcome5 = h.store.insert(kind5.clone(), &"wss://t/".into(), 0).unwrap();
    assert!(matches!(outcome5, InsertOutcome::Inserted { .. } | InsertOutcome::Duplicate { .. }));

    let article = h.signed(EventBuilder::new(Kind::from(30023), "hello", vec![
        Tag::parse(vec!["d", d_tag]).unwrap(),
    ]));
    let article_id = article.id.to_bytes();
    let outcome_article = h.store.insert(article, &"wss://t/".into(), 0).unwrap();
    assert!(
        matches!(outcome_article, InsertOutcome::Tombstoned { origin: TombstoneOrigin::Kind5, .. }),
        "expected Tombstoned, got {outcome_article:?}"
    );
    assert!(h.store.get_by_id(&article_id).unwrap().is_none());
});
```

Restart variant: `h.restart()` between the kind:5 insert and the article insert — assert the address tombstone survives the restart.
