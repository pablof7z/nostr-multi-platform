---
title: NMP Performance Test Battery
slug: nmp-perf-battery
summary: Framework-level performance battery proving the snapshot-broadcast + claim-GC + single-actor model for ANY NMP app. Chirp iOS is the testing ground, not the subject.
tags:
  - performance
  - testing
  - framework
  - snapshot
  - reactivity
  - opus-plan
volatility: hot
confidence: high
created: 2026-05-31
updated: 2026-05-31
verified: 2026-05-31
compiled-from: workflow:nmp-perf-battery-v1
---

# NMP Performance Test Battery

Prove NMP's performance *claims* to adopters. Chirp iOS is the harness; every metric here must generalize to **any app built on NMP**, not just Chirp.

## Critical reframing (read first)

The 98% idle false-wake in `docs/perf/reactivity-bench/2026-05-17-run-001.md` measured a **proposed** per-view reactivity engine that lives only in `crates/nmp-testing/bin/reactivity-bench`. It is **not the live path.**

The live actor (`crates/nmp-core/src/actor/tick.rs`) gates on a whole-kernel `changed_since_emit` flag and emits a full FlatBuffers snapshot at ≤4Hz. The test `idle_ticks_do_not_emit_snapshots_when_state_unchanged` proves exactly **1 frame/sec at true idle** — the ~4/sec SwiftUI re-renders in Chirp are not spurious Rust emits.

Under a live-but-quiet feed, `poll_claim_expansion` and claim churn legitimately bump `changed_since_emit`. The actor sends a full snapshot. iOS re-evaluates every row body because `TimelineItem` is not Equatable-diffed by identity. The fix is iOS-side identity diffing (C3), not a Rust reactive engine change.

The "D8 composite reverse index" the bench described **is shipped** — in the **planner** (`nmp-planner/src/interest.rs`), deduplicating N interests → M≤N wire REQs. That is coalescing at the REQ level. The unshipped piece is the per-view dispatch engine (D1), which is gated before it lands.

## Battery — 14 metrics, ordered by priority

| # | ID | Layer | Target | Current state |
|---|---|---|---|---|
| 1 | **C3-idle-rerender-ios** | ios | 0 body re-evals for unchanged rows | ~4/sec |
| 2 | **S2-snapshot-scaling-vs-state** | rust | per-event µs at 100k ≤ 1.5× at 1k | **Fixed by PR #873** |
| 3 | **M3-working-set-bounded** | rust | ≤100 MB @ 100 views/10k hot events; 0 monotonic growth/1h | unmeasured |
| 4 | **D1-false-wake-engine-gate** | rust | false_wakeup_rate ≤ 0.10 at idle and scroll | engine not shipped; bench-only |
| 5 | **B1-typed-decode-success** | ffi | ≥99.9% typed-decode over 1000-tick window | unmeasured |
| 6 | **C1-snapshot-apply-p99** | ios | p99 ≤ 16ms; ceiling ≤ 50ms | ceiling gated; p99 unmeasured |
| 7 | **R4-coalescing-ratio** | rust | M/N ≤ 0.3 for typical interest set | planner coalesces; ratio unmeasured |
| 8 | **Q5-actor-queue-drain** | rust | drains to 0 within 500ms after 10k-cmd burst | unmeasured |
| 9 | **X6-cold-start-latency** | ios | first event ≤ 1500ms median vs local relay | unmeasured |
| 10 | **I7-timeline-ingest-throughput** | rust | ≤ 50µs/event; 0 allocs at steady state | 45.95µs/event |
| 11 | **A1-warm-reclaim-name-gap** | rust | 0 ticks showing shortHex for cached author | ~1–2 ticks/back-nav |
| 12 | **A2-name-regression-count** | ios | 0 name regressions (hard gate) | non-zero on every back-nav |
| 13 | **P8-scroll-fps** | ios | ≥58fps; hitch < 5ms/s with concurrent 4Hz snapshots | no baseline set |
| 14 | **N9-nav-transition-frames** | ios | 0 dropped frames on push/pop | no baseline set |

## Priority rationale

**C3** is #1: every NMP app on iOS suffers this. Any state mutation anywhere causes every visible row to re-evaluate its SwiftUI body. Row-level equatable identity diffing is the canonical fix for snapshot-driven UIs and has zero Rust-side risk.

**S2** is #2: `estimated_store_bytes` ran two full O(store) field-length scans on **every** snapshot emit — O(events) work every 250ms. At 100k stored events this silently broke NMP's promise of bounded platform cost. Fixed by PR #873 (memoized Cell, invalidated at all 5 mutation sites).

**D1** is #4 (not #1): the live actor doesn't use the per-view reactive engine. Until that engine ships, conjunctive-dispatch false-wake rate is irrelevant to production. When it ships, D1 gates its merge.

## Tests that currently cover these metrics

| Metric | Test | Location |
|---|---|---|
| S2 | `snapshot_make_update_cost_is_sublinear_in_store_size` | `crates/nmp-core` (added by PR #873) |
| S2 | `estimated_store_bytes_cache_matches_fresh_compute` | same |
| I7 | `timeline_ingest_perf` | `crates/nmp-core/src/kernel/timeline_perf_tests.rs` |
| A1 | `warm_reclaim_reemits_profile_next_tick_with_no_req` | PR #821 |
| A1/A2 | `testProfileName_persistsThroughNavRoundtrip` | `ios/Chirp/ChirpUITests/ChirpUITests.swift` |
| P8 | `testScrollPerformance` | same (no baseline set yet) |
| N9 | `testNavTransitionPerformance` | same (no baseline set yet) |

## Next optimizations in priority order

1. **C3 — iOS row Equatable diffing** (priority 1, medium effort): Make all snapshot row types (`TimelineItem`, `EventCard`, etc.) conform to `Equatable`/`Identifiable` by stable `id`. In the snapshot apply path, diff incoming rows against current before mutating SwiftUI state. Only rows whose data changed should trigger body re-evaluation. Expected: C3 collapses from ~4/sec → 0 for quiet feeds.

2. **M3 — GC proof harness** (priority 3, medium effort): Rust test that opens 100 views, ingests 10k events, drops all claims, calls prune, and asserts RSS returns to within 1.1× cold baseline. GC exists conceptually but is unproven.

3. **B1 — FlatBuffers decode telemetry** (priority 5, low effort): Counter in the Swift decode path incremented on typed-decode failure. Expose via diagnostics projection. One decode failure should log a warning.

## How to run the Rust portion

```sh
# S2 scaling test (fast, ~2s)
cargo test -p nmp-core --lib perf -- --show-output

# I7 ingest throughput (requires --release)
cargo test -p nmp-core timeline_ingest_perf --release -- --ignored --nocapture
```

## See Also
- [[chirp-ios-reliability-metrics-testing-plan]] — A1/A2/B1/C1/C3 metrics (earlier Opus plan)
- `docs/perf/reactivity-bench/2026-05-17-run-001.md` — bench that measured 98% false-wake (proposed engine)
- `docs/perf/timeline-ingest-measured-2026-05-21.md` — I7 baseline measurement
