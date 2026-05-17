# Reactivity: Validation Harness

[Back to Design: Reactivity](../reactivity.md)

## 8. What this design rules out

Listed so we notice when we accidentally violate them:

- **Async view recompute.** Views are synchronous. Async data flow goes through a separate `CoreMsg` and lands as a fresh insert/projection change.
- **Cross-view dependencies.** Views read from the store and from projections, never from other views.
- **Mutation of `EventStore` from within a view.** Views observe; they don't write. Only the actor's top-level handlers and actions write.
- **Hidden allocations on hot paths.** The reverse-index lookup, the view dispatch, and the delta buffer must not allocate in steady state (we'll use `SmallVec`, `IndexSet`, etc. where appropriate).
- **Per-event FFI calls.** All FFI emission happens at the batch boundary, never per-event.

---

## 9. Open questions to settle empirically

These questions can be answered with measurement, not argument. They block locking in the design but not starting to build (the design can absorb them).

1. **Reverse-index hit rate.** For realistic Nostr apps, what fraction of inserts have ≥ 1 view interested? Affects whether we should optimize the no-hit path.
2. **Recompute cost per view kind.** Is incremental insert into a 200-item timeline genuinely cheaper than full rebuild from a 10k-event store? Probably yes, but at what threshold does it flip?
3. **Projection cache scope.** Should `author_display` cache **every** pubkey we've ever seen, or only those currently referenced by an open view? The latter saves memory; the former saves recompute on view open.
4. **Delta buffer thresholds.** Is 16ms / 256 deltas the right default flush trigger, or should it adapt to platform-measured callback latency?
5. **`UpdatedMany` vs N × `Updated`.** When a projection change affects 50 items in a timeline, is one fat delta cheaper or 50 small ones? Wire cost says fat; per-platform application cost says depends.
6. **`catch_all_filter` cost.** What's the per-event overhead of filter matching for views that need it? Is it tolerable for hashtag search?
7. **Backpressure threshold.** Is 100ms p99 callback latency the right trigger for switching to `FullState` catch-up, or too aggressive / too lax?

---

## 10. Next step: build the stress harness *before* committing to this design

This design has assumptions that need to be measured, not argued. Before locking it in (i.e., before Phase 1 of the build plan goes very far), build a **standalone stress harness** in `nmp-testing`:

### 10.1 Harness scope

A headless Rust binary (`nmp-testing/bin/reactivity-bench`) that:

- Spawns a configurable `EventStore` (in-memory, LMDB, or SQLite backend).
- Pre-populates it with N synthetic events (configurable: 1k, 10k, 100k, 1M).
- Opens M views with configurable filter mixes (timelines, threads, profiles, hashtag catch-alls).
- Replays a configurable event stream (steady 100/sec, burst, firehose 500/sec, hashtag firehose 2000/sec).
- Reports: per-event lookup time, per-view recompute time, delta buffer fill rate, `ViewBatch` emission rate, memory footprint, allocation counts.

### 10.2 Scenarios to run

1. **Quiet idle.** 10k events in store, 10 views open, 1 event/sec.
2. **Following timeline scroll.** 100k events in store, 1 timeline view over 1k authors, scroll triggers (cursor advances) every 500ms.
3. **Hashtag firehose.** 1M events in store, 1 catch-all view over hashtag `#nostr`, 200 events/sec inbound.
4. **Profile fan-out.** 10k events, 50 timeline views over overlapping author sets, kind:0 for shared author arrives — measure how many views update and how fast.
5. **Thread blow-up.** 1 thread view, the root event has 500 replies + 5000 reactions; measure incremental vs full rebuild.
6. **Account switch.** 10 accounts, each with active views; switch between them; measure teardown + setup time.

### 10.3 Gates on harness results (rev 1, post run 001)

Refined per ADR-0001 through ADR-0004:

- **Reverse-index lookup p99 ≤ 100µs** at 100k events / 50 views. (Run 001: validated at 84 ns – 1,083 ns.)
- **Per-view incremental recompute p99 ≤ 1ms.** (Run 001: validated at ≤ 9,625 ns.)
- **Delta emission ≤ 60 deltas/sec/view** under all scenarios. (Per-view, not absolute. ADR-0002.)
- **False-wakeup rate ≤ 0.10** across all scenarios. (ADR-0001 gate. Run 001: 98%/49% under v0; expected near-zero under composite-key model.)
- **Candidates per delta ≤ 1.25.** (Sister metric to false-wake rate.)
- **Working-set memory ≤ 100 MB** at 100 active views / 10k hot events / 1M cached on disk. (ADR-0003.)
- **Zero per-event allocations on the steady-state path** after 1,000-event warmup, verified by counting allocator. (ADR-0004.)

If any gate fails, the design choices in §3–§7 get revisited before Phase 1 proceeds further. Each gate failure surfaces a write-up in `docs/perf/reactivity-bench/<date>-run-<n>.md` plus an ADR when a design change is adopted.

### 10.4 Where the harness lives

```
crates/nmp-testing/
├── src/
│   └── ...
└── bin/
    └── reactivity-bench/
        ├── main.rs
        ├── scenarios/
        └── reports/
```

Output is JSON, archived per run in `docs/perf/reactivity-bench/<date>.json`, with a Markdown summary in `docs/perf/reactivity-bench/<date>.md`. The proof app's performance overlay (Phase 8) reuses the same metric definitions.

### 10.5 Why this gate matters

The reverse-index + projection-cache + delta-buffer architecture is the load-bearing performance story for the entire framework. If it doesn't measure up, snapshots+ViewBatch falls back to "ship `FullState` everywhere" which we already know doesn't scale for Nostr timelines (Appendix A1 of the spec). The harness is how we know we don't have to fall back to the SQLite-shared-store hybrid prematurely (Appendix A2 of the spec).

Build the harness first. Measure. Then commit to Phase 1 of the build plan in earnest.
