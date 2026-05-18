# LMDB sub-design: test plan

> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Maps every insert invariant in `docs/product-spec/subsystems.md` §7.1 to a concrete test in `crates/nmp-testing/tests/`. Each test exists for both `MemEventStore` (always) and `LmdbEventStore` (under `#[cfg(feature = "lmdb-backend")]`).
>
> **Sub-files:** [`tests/insert.md`](tests/insert.md) (§2.1–2.7a), [`tests/migration.md`](tests/migration.md) (§2.12–2.13).

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

### 1.1 Write-counting test hook (`LmdbEventStore`-only)

To verify that insert paths write the expected number of secondary-index entries without reaching into LMDB internals, `LmdbEventStore` exposes a backend-internal observer hook. This is **not** on the `EventStore` trait — it lives on the concrete `LmdbEventStore` type and is invisible to the actor:

```rust
// crates/nmp-core/src/store/lmdb/mod.rs
// Compiled only under test or the "test-support" feature (never in release binaries).
#[cfg(any(test, feature = "test-support"))]
pub trait WriteObserver: Send + Sync {
    fn on_put(&self, sub_db: &'static str);
}

#[cfg(any(test, feature = "test-support"))]
impl LmdbEventStore {
    /// Attach an observer that counts every LMDB `put` in subsequent writes.
    /// Observer is cleared when the store is dropped.
    pub fn with_write_observer(&mut self, obs: Arc<dyn WriteObserver>) { /* ... */ }
}
```

Tests that need it construct `LmdbEventStore` directly and attach a `WriteCounter`:

```rust
#[cfg(feature = "lmdb-backend")]
#[test]
fn insert_kind30023_writes_dtag_time_index() {
    let counter = Arc::new(WriteCounter::default());
    let mut store = LmdbEventStore::open(tmp_path()).unwrap();
    store.with_write_observer(counter.clone());
    store.insert(kind30023_event(), &"wss://t/".into(), 0).unwrap();
    assert_eq!(counter.puts_for("idx_kind_dtag"), 1);
    assert_eq!(counter.puts_for("idx_kind_dtag_time"), 1);
}
```

`WriteCounter` is a `Arc<Mutex<HashMap<&'static str, usize>>>` in `crates/nmp-testing/src/write_counter.rs`.

## 2. Invariant → test mapping

| Invariant (§7.1) | Test file | Detail |
|---|---|---|
| Insert API single path | `store_insert_path.rs` | [§2.1](tests/insert.md#21-insert-api-single-path-71-row-insert-api) |
| Signature/delegation validity | `store_invalid_sig.rs` | [§2.2](tests/insert.md#22-signature-verification-71-row-signaturedelegation-validity) |
| Duplicate id → merge provenance | `store_provenance_merge.rs` | [§2.3](tests/insert.md#23-duplicate-id--provenance-merge-71-row-duplicate-id) |
| Replaceable supersession | `store_replaceable.rs` | [§2.4](tests/insert.md#24-replaceable-supersession-71-row-replaceable-kinds) |
| Parameterized replaceable | `store_param_replaceable.rs` | [§2.5](tests/insert.md#25-parameterized-replaceable-71-row-parameterized-replaceable) |
| Kind:5 self-delete + tombstone | `store_kind5_tombstone.rs` | [§2.6](tests/insert.md#26-kind5-self-delete--tombstone-persistence-71-row-kind-5) |
| Foreign kind:5 ignored | `store_kind5_foreign.rs` | [§2.7](tests/insert.md#27-foreign-kind5-ignored-71-row-kind-5--foreign-clause) |
| Kind:5 `a`-tag before target | `store_kind5_addr_tombstone.rs` | [§2.7a](tests/insert.md#27a-kind5-a-tag-delete-arriving-before-the-target-event) |
| NIP-40 expiration scheduled + reaped | `store_nip40_expiration.rs` | §2.8 below |
| Watermarks survive restart | `store_watermarks.rs` | §2.9 below |
| Claim/release; GC drops un-claimed | `store_gc_claims.rs` | §2.10 below |
| `nmp dump` round-trip byte-identical | `store_dump_roundtrip.rs` | §2.11 below |
| Migration v0→v1; rollback on N+1 fail | `store_domain_migration.rs` | [§2.12](tests/migration.md#212-domain-migration-success--failure-master-doc-6) |
| Migration crash-recovery atomicity | `store_domain_migration.rs` | [§2.12a](tests/migration.md#212a-migration-atomicity--crash-recovery-watermarksmd-42) |
| Domain isolation module A/B | `store_domain_isolation.rs` | [§2.13](tests/migration.md#213-domain-isolation-kernel-substratemd-8) |
| Cold-start ≤ 1.5 s | `firehose-bench cold_start` | §2.14 below |
| Working-set ≤ 100 MB | `reactivity-bench` | §2.15 below |
| Restart preserves replaceable | `store_replaceable_restart.rs` | §2.16 below |

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
- Assert: 10 claimed events still present in hot; 40 unclaimed events evicted from LRU but still readable from disk.
- Release `ClaimerId(1)`; insert another 20 events; call `gc_step`.
- Assert: previously claimed events now subject to LRU eviction.

### 2.11 Dump round-trip (master doc §10)

File: `crates/nmp-testing/tests/store_dump_roundtrip.rs`

- Build a populated store: 1000 events, 50 tombstones, 100 watermarks, 200 domain rows across 3 namespaces.
- `dump(&mut buf1, DumpFormat::Jsonl)`. Open a fresh store; replay every line; `dump(&mut buf2, ...)`.
- Assert `buf1 == buf2` byte-for-byte (sort by stable key first).

### 2.14 Cold-start performance (master doc §12)

Scenario in `crates/nmp-testing/bin/firehose-bench/src/scenarios/cold_start.rs`:

- Pre-populate an LMDB store with ~20k events (10k kind:1, 8k kind:0, 2k kind:3/10002).
- Gate: ≤ 1.5 s on iPhone 12 hardware; ≤ 800 ms on iPhone 16 Pro simulator.

### 2.15 Working-set memory (ADR-0003)

Scenario in `crates/nmp-testing/bin/reactivity-bench` (`--scenario working_set_lmdb`):

- Insert 1M synthetic events; open 100 views covering 10k events; run 60 s with 10 inserts/sec.
- Gate: max RSS ≤ 100 MB over the run.

### 2.16 Restart preserves replaceable semantics (`plan.md` §M3 exit gate)

File: `crates/nmp-testing/tests/store_replaceable_restart.rs`

- Write kind:0 v1; assert present. Write kind:0 v2 (newer); assert v1 gone, v2 present.
- Restart store; assert v2 still present, v1 still gone.
- Write kind:0 v0 (older than v2); assert no change (`InsertOutcome::Superseded`).

## 3. Property tests

In `crates/nmp-testing/tests/store_props.rs` using `proptest`:

- **Insert is total under random valid events.** Assert `get_by_id` agrees with a `HashMap` reference impl.
- **Replaceable convergence.** For any insertion order, the final event is the (max created_at, min id) winner.
- **Provenance commutativity.** Two relay sources in any order produce identical provenance post-state.
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
