---
title: Cache Baseline
slug: cache-baseline
topic: data-persistence
summary: Baseline capture (#1522) is a hard precondition before #1516 or any performance-affecting PR
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-19
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:129d2615-7195-4082-924e-9b96e3f1de8b
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# Cache Baseline

## Purpose and Precondition

Baseline capture (#1522) is a hard precondition before #1516 or any performance-affecting PR. Native shells must not own cache policy. PR #1527 (issue #1522) created 11 LMDB baseline tests covering all 6 StoreQuery variants plus a cache-baseline measurement binary. Issue #1516 is now resolved by PR #1531, which replaced query_visit's Vec materialization with lazy per-row conversion via run_filter_visit in a new query_streaming.rs sibling module; ControlFlow::Break now stops the LMDB scan immediately. (Previously: the LMDB query_visit currently materializes a Vec<StoredEvent> via run_filter before visiting—this was the baseline behavior that #1522 must document before #1516 changes it.) No production behavior change is permitted in #1522 — only the two new test/binary files and Cargo.toml. The PR description for #1522 must paste actual numbers from the cache-baseline binary run. §8 follow-ups are tracked as concrete implementation issues (#1515–#1524) with a cross-reference table, not prose. File sizes must stay under 500 lines; files must be split rather than raising the baseline.

<!-- citations: [^129d2-14] [^129d2-15] [^129d2-16] [^129d2-17] [^129d2-24] [^129d2-49] [^129d2-75] [^129d2-97] [^129d2-113] [^129d2-122] -->
## Binary and Test Implementation

The cache-baseline binary must not use #![cfg] on the crate root (causes link error when the feature is off); instead it gates logic via a run() fn with #[cfg(feature = "lmdb-backend")] and a main() that prints a notice when the feature is absent. The cache-baseline binary and store_cache_baseline integration tests must use StoreHarness::lmdb() directly, not the for_each_backend! macro. The store_cache_baseline tests must call assert_invariants() at the end of each test. <!-- [^129d2-25] -->


The early-stop materialization regression gate uses an instrumented AtomicU64 scan counter gated #[cfg(any(test, feature = "test-support"))]; the test is marked #[ignore] until #1516 streaming lands and flips green. <!-- [^129d2-37] -->
## Metrics and Dependencies

The cache-baseline binary reports events-scanned vs events-returned (currently equal because LMDB pre-materializes via run_filter), total store size as the upper-bound scan domain, and min/mean/p50 latency in microseconds across 50 iterations. It must not add dhat, alloc-counter, or Criterion dependencies—only std::time::Instant and std::collections. Materialization regression gates are hard (failing assertions) for events-scanned count and replay-chunk count; query latency and allocation bytes are delta-report-only (machine-sensitive, pasted into PR descriptions). Cache-baseline latency and allocation measurements are machine-sensitive and must be delta-reported in PR descriptions, not hard-gated in CI; events-scanned count and replay-chunk count are deterministic and safe as hard CI gates. No new GitHub Actions workflow is created for #1524; deterministic gates run as plain cargo test on PR, and any wall-clock gate would go nightly like the s3-snapshot-pressure-gate precedent. Query latency and allocation bytes/query are delta-report-only dimensions (machine-sensitive); events-scanned count and replay-chunk count are hard gates (deterministic).

<!-- citations: [^129d2-26] [^129d2-36] [^129d2-50] [^129d2-59] [^129d2-76] [^129d2-99] [^129d2-107] [^129d2-137] -->
## Output Footer

The #1522 cache-baseline measurement binary prints a table of events-scanned vs events-returned and latency per query shape, noting that pre-#1516 LmdbEventStore::query_visit materializes Vec<StoredEvent> via run_filter() before visiting so returned equals scanned. PR-delta perf reporting uses the cache-baseline binary output (mean_us, min_us per scenario) as report-only, not a hard CI gate; only events-scanned count and replay-chunk count are hard gates because they are deterministic.

<!-- citations: [^129d2-27] [^129d2-60] [^129d2-130] -->
## Parallel Work and Scope

Issue #1516 (streaming query_visit) and #1518 (provenance indexes) can proceed in parallel after baseline capture (#1522) lands, provided their write sets stay separate. PR #1531 has implemented the #1516 streaming query_visit change. Sub-issue #1521 runs after main store surfaces are stable (after #1516 and #1518 land).

Issue #1517 (cache coverage audit) is test-and-doc only with no production code changes; shape_to_store_queries is already correct. Issue #1517 added coverage assertions and doc comments to queries.rs and a new cache_serve_coverage_tests.rs with 9 covered-shape and 4 tracked-exception test cases asserting non-empty and empty respectively, plus a test pinning each covered shape to its expected StoreQuery variant discriminant. The #1517 coverage tests document that Etag and Ptag carry no time cursor (intentional conservative over-serve), and add an intentionally-uncovered cases subsection covering wildcard kinds, multi-tag intersection, event-ids-only, unrecognized tag keys, and text/search.

P4 findings 5 and 6 (web ProjectionMergeCache and config single-sourcing) are deferred to a follow-up issue because web is post-v1. Follow-up issue #1546 captures these P4 web findings: move ProjectionMergeCache into the wasm worker (Rust side) and single-source web config from Rust nmp-chirp-config.

<!-- citations: [^129d2-51] [^129d2-78] [^11850-51] [^129d2-114] -->
## Final Acceptance Gates (Issue #1524)

Issue #1524 (final acceptance gates) must: widen CONVERSION_COUNT to test-support visibility, add an 8-test materialization gate asserting conversion_count == n_break (not the full corpus) for each of the 6 StoreQuery variants—ensuring that a regression to Vec-materialization would trip the gate (cache_no_materialization_gate asserts conversion_count == n_break; if streaming is reverted to collect().take(limit), the count jumps and the test fails), add 6-fixture Mem≡LMDB parity tests using for_each_backend! for all 6 cache-serve shapes (feed, author-kind, thread, DM ciphertext, profile metadata, relay provenance), add a CI step running cargo test -p nmp-testing --features lmdb-backend to exercise the cache-baseline, materialization-gate, and replay-fixture tests, and create cache-gates.md documenting hard vs delta-report-only thresholds. The #1524 acceptance gate exposes CONVERSION_COUNT under #[cfg(any(test, feature = "test-support"))] so nmp-testing integration tests can assert streaming (no over-materialization) via conversion_count() == n_break.

<!-- citations: [^129d2-77] [^129d2-98] [^129d2-123] [^129d2-138] -->
