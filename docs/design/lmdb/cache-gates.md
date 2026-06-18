# Cache adoption acceptance gates (epic #1523)

> Added in PR implementing issue #1524. Companion to `tests.md` §5.

## Purpose

These gates close the acceptance criteria for epic #1523 (LMDB streaming
`query_visit` adoption). They verify two independent properties:

1. **No over-conversion** — early `ControlFlow::Break` stops LMDB event
   deserialization immediately; the streaming path does not pay the full-corpus
   conversion cost for a partial read.
2. **Mem ≡ LMDB parity** — the same query over the same events produces
   byte-identical results (same count, same newest-first order) across both
   `MemEventStore` and `LmdbEventStore`.

## Gate inventory

| Test file | Backend | Gate |
|---|---|---|
| `cache_no_materialization_gate.rs` | LMDB only | Materialization regression (8 tests) |
| `cache_serve_replay_fixtures.rs` | Mem + LMDB | Parity fixtures (5 fixtures × 2 backends) |

## Run commands

```bash
# Materialization regression gate (LMDB only)
cargo test -p nmp-testing --features lmdb-backend \
  --test cache_no_materialization_gate

# Mem ≡ LMDB parity fixtures (Mem backend)
cargo test -p nmp-testing --test cache_serve_replay_fixtures

# Mem ≡ LMDB parity fixtures (both backends)
cargo test -p nmp-testing --features lmdb-backend \
  --test cache_serve_replay_fixtures

# Run both gates (matches CI)
cargo test -p nmp-testing --features lmdb-backend \
  --test cache_no_materialization_gate \
  && cargo test -p nmp-testing --features lmdb-backend \
  --test cache_serve_replay_fixtures
```

## Materialization counter

The LMDB materialization counter (`CONVERSION_COUNT` in
`crates/nmp-store/src/lmdb/query_streaming.rs`) counts how many LMDB events
are deserialized (`EventBorrow → StoredEvent`) per `run_filter_visit` call.
It is exposed under the `test-support` feature:

```rust
use nmp_core::store::{conversion_count, reset_conversion_count};

reset_conversion_count();
store.query_visit(&q, limit, &mut |ev| {
    // ... visitor that breaks early
    ControlFlow::Break(())
})?;
assert_eq!(conversion_count(), 1); // only one event was deserialized
```

The counter is never compiled into production binaries. It is only active
when `features = ["test-support", "lmdb-backend"]` are both enabled.

## Parity fixtures

| Fixture | Query shape | Insert count | Assert |
|---|---|---|---|
| `replay_feed` | `KindTime` | 150 kind:1 | count=150, newest-first |
| `replay_author_kind` | `AuthorsKind` | 60 authors × 3 events | count=180, newest-first |
| `replay_thread` | `Etag` | root + 80 replies | count=80, newest-first |
| `replay_dm_ciphertext` | `AuthorKind` | 40 kind:4 + 40 kind:14 | count=80, no kind:1 noise |
| `replay_profile_metadata` | `AuthorKind` | 5 kind:0 (replaceable) | count=1 (newest only) |

`replay_relay_provenance` is deferred pending a dedicated relay-provenance
`StoreQuery` variant (see TODO comment in `cache_serve_replay_fixtures.rs`).
