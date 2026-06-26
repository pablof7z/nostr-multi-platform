---
title: LMDB Event Store
slug: lmdb-event-store
topic: data-persistence
summary: The `EventStore` trait is the unified interface for event persistence, with `MemEventStore` for tests/WASM and `LmdbEventStore` for production (selected via `St
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-19
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:7c780fef-d33c-4d22-bcdb-2d9ab625a4f9
  - session:129d2615-7195-4082-924e-9b96e3f1de8b
  - session:e6b44a84-8cfc-48b2-863a-58382398b5df
---

# LMDB Event Store

## Event Store Interface

The `EventStore` trait is the unified interface for event persistence, with `MemEventStore` for tests/WASM and `LmdbEventStore` for production (selected via `StorageBackend` enum).

Interest registration enqueues cache-serve work; the single-path drains a chunk synchronously (to ensure data is available before the next frame), while the batch path enqueues all N interests then drains once, continuing the rest chunked across later ticks.

Late-opening projections must use the observed-projection registration path so
matching LMDB events are replayed before the observer is activated. Public raw
event taps are not a supported app hydration mechanism.

Baseline tests use StoreHarness::lmdb() directly, not the for_each_backend! macro. Mem≡LMDB parity tests use for_each_backend! so both backends are held to the identical contract on every PR.

The #1516 change makes LMDB query_visit use lazy per-row conversion via run_filter_visit with ControlFlow::Break stopping immediately (no Vec materialization); the old run_filter Vec path is retained for scan_by_*/EventIter callers. (Previously: query_visit materialized a Vec<StoredEvent> in run_filter() before visiting.) The streaming query_visit implementation (run_filter_visit) converts one event per cursor row via into_owned → nostr_to_raw → stored_from_raw; ControlFlow::Break stops immediately with no further conversions. The CONVERSION_COUNT AtomicUsize counter in query_streaming.rs is exposed under #[cfg(any(test, feature = "test-support"))] so integration tests in nmp-testing can reach it via nmp_core::store::reset_lmdb_conversion_count / lmdb_conversion_count. A materialization regression gate (#1524) includes per-variant streaming proof tests for all 6 StoreQuery variants (KindTime, AuthorsKind, AuthorKind, KindDtag, Etag, Ptag), each asserting conversion_count() == n_break. The regression gate requires that after inserting N events and calling query_visit with a visitor that Breaks after k events, conversion_count equals exactly k (not N), proving lazy per-row streaming with no pre-materialization. Each variant must have its own streaming proof test because each follows a distinct build_filter code path. The test early_break_converts_exactly_n_not_full_corpus asserts that after inserting 10,000 events and breaking at 10, conversion_count equals 10 (not 10,000). Parallel test execution corrupts the shared CONVERSION_COUNT static; a Mutex serializer is needed in integration tests to prevent cross-test count leakage. The cache-baseline binary measures events-scanned vs events-returned and latency per StoreQuery variant. Events-scanned count and replay-chunk count are deterministic and suitable as hard CI gates.

The #1517 audit found that shape_to_store_queries() is already correct and complete per ADR-0045 E1–E3; uncovered cases (empty kinds, multi-tag, event-ids-only, unrecognized tag keys, text/search) are intentional. The issue required only tests and documentation, no production code changes. The cache_serve_coverage_tests.rs for #1517 contains two table-driven tests pinning the exact StoreQuery variant for each covered shape and enumerating tracked exceptions.

PR #1543 (issue #1521) added typed StoreError variants, open_impl_with_limits test seam, and 7 health-diagnostics tests for LMDB failure classification.

<!-- citations: [^129d2-90] [^129d2-91] [^7c780-2] [^7c780-3] [^129d2-20] [^129d2-21] [^129d2-41] [^129d2-55] [^129d2-103] [^129d2-115] [^129d2-133] [^129d2-140] [^e6b44-1] -->
## LMDB Sub-Databases

NMP's LMDB backend stores event blobs in the upstream fork's 11 sub-databases and NMP's own secondary state (provenance, tombstones, addr_tombstones, domain_versions, domain_data, relay_author_scores, lru_access, expiry_index, relay_index, coverage, nmp-relay-kind, interaction-counters) in separate sub-databases, all committed atomically in a single `RwTxn`. Tombstone enforcement in LMDB is done entirely through NMP's `tombstones`/`addr_tombstones` sub-databases; the upstream fork's `deleted_ids`/`deleted_coordinates` sub-databases are left empty to maintain single-writer-per-fact (D4).

The nmp-relay-kind sub-database uses key format `relay_url||0x00||kind(4 BE)||event_id(32)` with empty value (presence-only, not a counter), enabling both `relay_kind_coverage` and `relay_kind_count` via prefix scans. The `relay_kind_coverage` and `relay_kind_count` trait methods return aggregate kind/count facts, never per-event ids; private kinds (4, 13, 14, 15, 1059, 1060) always return empty/0. NMP_ADDITIONAL_DBS must be bumped when adding new LMDB sub-databases (was 10, bumped to 11 for relay-kind, then 12 for interaction-counters). Kinds 4, 13, 14, 15, 1059, 1060 (NIP-04 DM, NIP-17 seal/rumor, NIP-59 gift-wrap) are excluded from the relay-kind provenance index at write time via a privacy gate so private events never surface in relay_kind_coverage or relay_kind_count queries. The existing V-52 relay-index (nmp-relay-index) is out of scope for the privacy gate; retrofitting it would be a behavior change to a shipped surface and requires a separate issue. All relay-kind mutation occurs inside `provenance::upsert` and `provenance::delete` in the same `RwTxn`, satisfying the single-writer ADR-0011 constraint. The `provenance::delete` function needs `kind` passed as a parameter because the kind is not available at delete call sites; at insert-replace sites it comes from `event.kind`, and at delete/gc sites it is captured from the already-loaded event. Storing the kind in the relay-kind key makes the delete self-sufficient without an extra event load, since the kind is needed at every delete call site.

The interaction-counters sub-database (nmp-interaction-counters) uses key format `target_event_id(32)||counter_kind(1)` with value `count(8 BE u64)`; zero-value rows are deleted rather than stored. All interaction-counter writes happen in the same `RwTxn` as the triggering event write across insert, insert_kind5, delete, and gc paths, preserving ADR-0011. The shared CounterKind enum (Reply=1, Reaction=2, Repost=3, Zap=4) and classify() function live in `src/interaction.rs` as the single source of truth for both LMDB and Mem backends, with NIP-10 reply-marker precedence: reply > root > bare e-tag.

Rebase conflicts between #1518 (relay-kind) and #1519 (interaction-counters) are resolved by keeping both sets of hooks at all insert/delete/gc sites, with NMP_ADDITIONAL_DBS bumped to 12.

StoreError gains typed variants ReaderExhaustion { max_readers: u32 }, MapFull { map_size_bytes: u64 }, CorruptEnv(String), and VersionMismatch { detail: String } with bounded log-safe Display strings (D6/no-secrets compliance). MdbError in heed is non-exhaustive; classify_heed_err uses a `_ =>` catch-all arm to avoid compile-break.

open_impl_with_limits is a test seam in open.rs that allows failure-injection testing for map-full and reader-exhaustion errors, with MAP_SIZE and MAX_READERS as pub(super) constants.

<!-- citations: [^129d2-92] [^129d2-93] [^7c780-4] [^129d2-32] [^129d2-40] [^129d2-63] [^129d2-102] [^129d2-110] [^129d2-116] [^129d2-124] [^129d2-132] [^129d2-141] -->
## LMDB Key Encoding

All integer keys in LMDB are stored big-endian so that byte-order equals numeric order.

LMDB ordering is created_at DESC, id ASC; the LMDB fork's BTreeSet-backed iterator already delivers (created_at DESC, id ASC) natively, making the old run_filter post-sort a no-op. Harness IDs are monotonic ascending hex so insertion order equals id-ascending order for equal created_at values. MemEventStore matches this ordering.

KindDtag.d_tag is Vec<u8>, not String; test fixtures must use b"...".to_vec().

StoreHarness synthetic-sig builders (VerifiedEvent::from_raw_unchecked) are the correct fixture source for integration tests; nostr::Keys signing is not required.

Etag and Ptag StoreQuery variants carry no since/until time cursor; this is intentional conservative over-serve where relay fills the tail.

Shapes with wildcard kinds, multi-tag intersections, bare event IDs, or unrecognized tag keys produce no StoreQuery and fall back to relay delivery — these are intentional uncovered cases.

The AuthorsKind StoreQuery emits a single query for multi-author shapes (not per-author fan-out), collapsing follow-feed cold-start from O(N×follow_size) to O(1×follow_size).

Each StoreQuery variant maps 1:1 to an LMDB secondary index: AuthorKind/AuthorsKind → akc_index, KindTime → kc_index, Etag/Ptag → ktc_index, KindDtag → coordinate_index; the mapping is complete per ADR-0045 E1–E3.

PR #1535 (#1518 relay-kind index) required splitting `mem/insert.rs` by extracting `handle_kind5_insert` into `mem/insert_kind5.rs` to stay under the baseline.

<!-- citations: [^129d2-64] [^7c780-5] [^129d2-18] [^129d2-19] [^129d2-28] [^129d2-29] [^129d2-30] [^129d2-31] [^129d2-39] [^129d2-54] [^129d2-131] [^129d2-139] -->
## LMDB Environment

The LMDB environment is one per app data directory, with no cross-process sharing.

The project enforces a single LMDB writer constraint (ADR-0011 single-writer discipline); no second LMDB writer is ever allowed. All writes happen in the same `RwTxn` per ADR-0011.

No native shells owning cache policy are permitted.

Binary feature-gating must use an inner #[cfg(feature="lmdb-backend")] fn run() with a main() that conditionally calls it, never #![cfg] on the binary main (which causes a link error).

<!-- citations: [^129d2-117] [^7c780-6] [^129d2-5] [^129d2-42] [^129d2-56] [^129d2-125] [^129d2-134] [^129d2-142] -->
## Production Garbage Collection

Production GC runs via `gc_step_with_pins()` on a 60-second tick, with Phase 1 (expiry index scan), Phase 2 (LRU ceiling enforcement), and Phase 3 (tombstone purge at most once per hour). <!-- [^7c780-7] -->


#1521 (LMDB diagnostics) runs after main store surfaces are stable, not before #1516/#1518 land. <!-- [^129d2-43] -->
## Merge Rule Considerations

The `rule5_limit` merge rule refuses to merge two shapes that carry a limit; dropping the per-author `limit: 1000` eliminates this merge barrier. <!-- [^7c780-8] -->
