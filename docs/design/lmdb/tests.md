# LMDB sub-design: test plan

> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Maps every insert invariant in `docs/product-spec/subsystems.md` §7.1 to a concrete test in `crates/nmp-testing/tests/`. Each test exists for both `MemEventStore` (always) and `LmdbEventStore` (under `#[cfg(feature = "lmdb-backend")]`).

## 1. Test harness shape

```rust
// crates/nmp-testing/src/store_harness.rs
pub struct StoreHarness {
    pub store: Box<dyn EventStore>,
    pub tmp: tempfile::TempDir,
    pub keys: nostr::Keys,
}

impl StoreHarness {
    pub fn mem() -> Self { /* MemEventStore */ }
    pub fn lmdb() -> Self { /* LmdbEventStore in tmp dir */ }

    pub fn insert(&self, builder: EventBuilder, source: &str) -> InsertOutcome { /* ... */ }
    pub fn assert_present(&self, id: &EventId);
    pub fn assert_tombstoned(&self, id: &EventId);
    pub fn restart(&mut self);   // close + reopen the store; LMDB only
}

// Tests use a macro to run against both backends.
macro_rules! for_each_backend {
    ($name:ident, $body:expr) => {
        #[test] fn $name() { let mut h = StoreHarness::mem(); $body(&mut h); }
        #[cfg(feature = "lmdb-backend")]
        #[test] fn paste::paste!([<$name _lmdb>])() {
            let mut h = StoreHarness::lmdb(); $body(&mut h);
        }
    };
}
```

The harness lives in `crates/nmp-testing/src/` so per-test files are short and declarative.

## 2. Invariant → test mapping

Every row of the §7.1 table:

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

Tests the `tombstones_addr` sub-db path (see [`keys.md`](keys.md) §4.2):

```rust
for_each_backend!(a_tag_delete_before_event_suppresses_reinsert, |h: &mut StoreHarness| {
    // Build a kind:5 that references a kind:30023 by address (not yet in store).
    let d_tag = "my-article";
    let addr = format!("30023:{}:{}", h.keys.public_key(), d_tag);
    let kind5 = h.signed(EventBuilder::new(Kind::from(5), "", vec![
        Tag::parse(vec!["a", &addr]).unwrap(),
    ]));
    let outcome5 = h.store.insert(kind5.clone(), &"wss://t/".into(), 0).unwrap();
    // The kind:5 itself should be stored (for other clients to render).
    assert!(matches!(outcome5, InsertOutcome::Inserted { .. } | InsertOutcome::Duplicate { .. }));

    // Now insert the target parameterized replaceable event.
    let article = h.signed(EventBuilder::new(Kind::from(30023), "hello", vec![
        Tag::parse(vec!["d", d_tag]).unwrap(),
    ]));
    let article_id = article.id.to_bytes();
    let outcome_article = h.store.insert(article, &"wss://t/".into(), 0).unwrap();
    // Must be suppressed because the address tombstone matches.
    assert!(
        matches!(outcome_article, InsertOutcome::Tombstoned { origin: TombstoneOrigin::Kind5, .. }),
        "expected Tombstoned, got {outcome_article:?}"
    );
    assert!(h.store.get_by_id(&article_id).unwrap().is_none(), "article must not be stored");

    // Confirm that a subsequent re-insertion of the same article is also suppressed
    // (the address tombstone should have promoted an event-id tombstone for the specific id).
    let article2 = h.signed_with_id(article_id, EventBuilder::new(Kind::from(30023), "hello", vec![
        Tag::parse(vec!["d", d_tag]).unwrap(),
    ]));
    let outcome2 = h.store.insert(article2, &"wss://t/".into(), 0).unwrap();
    assert!(matches!(outcome2, InsertOutcome::Tombstoned { .. }));
});
```

Restart variant: `h.restart()` between the kind:5 insert and the article insert — assert the address tombstone survives the restart.

### 2.8 NIP-40 expiration scheduling (§7.1 row "NIP-40 expiration")

File: `crates/nmp-testing/tests/store_nip40_expiration.rs`

- Insert kind:1 with `expiration` tag at `now + 1 second`.
- Assert `scan_expiring_before(now + 5, 10)` returns the event.
- Call `gc_step(GcBudget { max_events_per_step: 10, max_duration_ms: 100 })` at `now + 2`.
- Assert primary row gone; tombstone written with `origin == NIP40Expiry`.
- Insert same event again — assert `InsertOutcome::Tombstoned`.
- Insert an event with `expiration` already in the past — assert `InsertOutcome::Rejected { reason: ExpiredOnArrival }`.
- Restart store; insert new event with `expiration` at `now + 1`; assert the reaper picks it up after restart (the `idx_expires` cursor scan is the source of truth — no separate timer needs to survive restart).

### 2.9 Watermarks (§7.1 "Sync watermarks")

File: `crates/nmp-testing/tests/store_watermarks.rs`

- Write a watermark; read it back; assert equal.
- Restart store; read again; assert preserved.
- Test `coverage()`: row with `synced_up_to = now - 60s` → `Coverage::CompleteAsOf` (under default 300s staleness); row with `synced_up_to = now - 600s` → `Coverage::PartialUpTo`; missing row → `Coverage::Unknown`.
- `list_watermarks_for_relay("wss://a/")` returns only rows for that relay.
- Concurrent writes to the same key (simulated): last-writer-wins, no row corruption.

### 2.10 Claims + GC (§7.1 "GC")

File: `crates/nmp-testing/tests/store_gc_claims.rs`

- Insert 100 events; all in hot LRU (under default 10k ceiling).
- Claim 10 of them under `ClaimerId(1)`.
- Configure `target_hot_size = 50`; insert another 50 events; call `gc_step`.
- Assert: 10 claimed events still present in hot (`store.get_by_id` is a fast in-memory hit — measurable via a counter exposed for the test); 40 unclaimed events evicted from LRU but still readable from disk.
- Release `ClaimerId(1)`; insert another 20 events; call `gc_step`.
- Assert: previously claimed events now subject to LRU eviction.

### 2.11 Dump round-trip (master doc §10)

File: `crates/nmp-testing/tests/store_dump_roundtrip.rs`

- Build a populated store: 1000 events, 50 tombstones, 100 watermarks, 200 domain rows across 3 namespaces.
- `dump(&mut buf1, DumpFormat::Jsonl)`.
- Open a fresh store; replay every line; `dump(&mut buf2, ...)`.
- Assert `buf1 == buf2` byte-for-byte (sort by stable key first — the dump iterates sub-dbs in a deterministic order documented in the dump module).

### 2.12 Domain migration success + failure (master doc §6)

File: `crates/nmp-testing/tests/store_domain_migration.rs`

- Register `TestModuleV1` with `SCHEMA_VERSION = 1` and no migrations; open store; assert `_meta.test_module.schema_version == 1`.
- Close store; register `TestModuleV2` with `SCHEMA_VERSION = 2` and one migration v1→v2 that writes one key; open store; assert migration ran and key exists.
- Close; register `TestModuleV3` with `SCHEMA_VERSION = 3` and a deliberately failing migration v2→v3; open store; assert `Effect::DomainSchemaTooNew { namespace: "test_module" }` (under degraded-mode rules) and `_meta` still at v2.
- Close; remove the failing migration; reopen — assert successful catch-up to v3 (idempotent retry).

### 2.12a Migration atomicity / crash-recovery (watermarks.md §4.2)

File: `crates/nmp-testing/tests/store_domain_migration.rs` (extended section)

Tests the single-`RwTxn` atomicity invariant for migration steps: verifies that a simulated crash (i.e., aborting the write transaction mid-migration) leaves the `_meta` version unchanged and the store in a retryable state.

```rust
#[cfg(feature = "lmdb-backend")]
#[test]
fn migration_crash_leaves_version_unchanged() {
    // Use a FailingMigration that panics after writing data but before commit.
    // We catch the unwind and re-open the store.
    let dir = tempfile::tempdir().unwrap();
    let result = std::panic::catch_unwind(|| {
        let store = LmdbEventStore::open(dir.path()).unwrap();
        let migrations = vec![DomainMigration {
            from_version: 0,
            to_version: 1,
            apply: Box::new(|tx| {
                tx.put(b"key", b"value")?;
                panic!("simulated crash after data write, before commit");
            }),
        }];
        let _ = store.run_migrations("test_ns", 1, &migrations);
    });
    assert!(result.is_err(), "expected panic");

    // Re-open: version must still be 0 (data write was not committed).
    let store2 = LmdbEventStore::open(dir.path()).unwrap();
    let version = store2.read_meta_schema_version_raw("test_ns").unwrap().unwrap_or(0);
    assert_eq!(version, 0, "version must not be bumped after a crashed migration");
    // The data write must also be absent (rolled back with the transaction).
    let handle = store2.domain_open("test_ns").unwrap();
    assert!(handle.get(b"key").unwrap().is_none());
}
```

### 2.13 Domain isolation (`kernel-substrate.md` §8)

File: `crates/nmp-testing/tests/store_domain_isolation.rs`

- Open `DomainHandle` for module A; write key `K`.
- Open `DomainHandle` for module B; read key `K` — assert returns `None`.
- Module B's `scan_prefix(b"")` returns only module B's rows.

### 2.14 Cold-start performance (master doc §12)

Scenario in `crates/nmp-testing/bin/firehose-bench/src/scenarios/cold_start.rs` (already exists in M1; extended here):

- Pre-populate an LMDB store with a representative session (~20k events: 10k kind:1, 8k kind:0, 2k kind:3 / 10002).
- Tar + ship the file with the test fixture.
- Measure: open store, register modules, run the bootstrap sequence that the actor runs on app launch, until the first `AppUpdate::FullState` is emitted with non-empty timeline.
- Gate: ≤ 1.5 s on iPhone 12 hardware; ≤ 800 ms on iPhone 16 Pro simulator.

### 2.15 Working-set memory (ADR-0003)

Scenario in `crates/nmp-testing/bin/reactivity-bench` — extended with a new `--scenario working_set_lmdb` mode:

- Insert 1M synthetic events into the store.
- Open 100 view subscriptions covering 10k events.
- Run for 60 seconds with light churn (insert 10 events / sec).
- Sample RSS every 5 seconds via `/proc/self/status` on Linux / `mach_task_basic_info` on iOS.
- Gate: max RSS ≤ 100 MB over the run.

### 2.16 Restart preserves replaceable semantics (`plan.md` §M3 exit gate)

File: `crates/nmp-testing/tests/store_replaceable_restart.rs`

- Write kind:0 v1; assert present.
- Write kind:0 v2 (newer); assert v1 gone, v2 present.
- Restart store; assert v2 still present, v1 still gone.
- Write kind:0 v0 (older than v2); assert no change (`InsertOutcome::Superseded`).

## 3. Property tests

In `crates/nmp-testing/tests/store_props.rs` using `proptest`:

- **Insert is total under random valid events.** Generate a vec of valid signed events, insert in any order, assert the store's `get_by_id` agrees with the model (a `HashMap` reference impl).
- **Replaceable convergence.** For any sequence of replaceable inserts for the same `(pubkey, kind, [d])` key, the final stored event is the (max created_at, min id) winner regardless of insertion order.
- **Provenance commutativity.** For any two relay sources `r1, r2` and identical event, the post-state of provenance is identical to inserting `r2` first then `r1`.
- **`nmp dump` is a fixed point.** Round-trip equality after N random operations.

## 4. Cross-test invariants (asserted in a `teardown` hook for every test)

Every test ends with `harness.assert_invariants()`:

1. Every event in the primary store has a `provenance` row with ≥ 1 entry.
2. Every secondary index entry's `event_id` resolves to an existing primary row.
3. Every tombstone's `target_id` does **not** exist in the primary store.
4. The `_meta._kernel.schema_version` is at the latest version the binary knows.
5. The hot LRU contains only events that exist in the primary store.

Violation of any invariant fails the test with a precise diff of which sub-db is out of sync.

## 5. CI integration

`cargo test --workspace --features lmdb-backend` becomes part of the pre-merge gate from M3 onward (`plan.md` §6 will be updated). The criterion benches in `crates/nmp-testing/benches/store_*.rs` run nightly with regression checks against the previous week's median (>5% regression on any p99 fails the nightly).
