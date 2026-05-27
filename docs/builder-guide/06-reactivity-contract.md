# 06 — Reactivity contract (D8)

> **Status: SHIPS** · audience: agents · design = `docs/design/reactivity/*`,
> ADR-0001..0004; validation = `reactivity-bench` run 002.

D8 is the contract that keeps view payloads in sync with the event store at
firehose scale **without** O(views × inserts) work, false wakeups, or
post-warmup allocations. This section is the distilled contract; the
mechanism lives in `docs/design/reactivity/loop-and-reverse-index.md`,
`scheduling-and-data-model.md`, `view-deltas-and-projections.md`.

## The reactive loop (from the design)

```
relay/sync ─▶ CoreMsg::EventInserted(event)        [on the one actor thread]
                │
                ▼  EventStore::insert(event)        replaceable/GC/tombstone
                │
                ▼  ReverseIndex::lookup(event) ─▶ Vec<ViewId>
                │                                  composite-keyed (ADR-0001)
                ▼  for each ViewId:
                     view.on_event_inserted(event) ─▶ Option<ViewDelta>
                │
                ▼  DeltaBuffer::push(delta)
                │
                ── on tick (≤60Hz) ──
                ▼  DeltaBuffer::flush()             coalesce by view id (ADR-0002)
                ▼  AppUpdate::ViewBatch ─▶ update_tx.send()
                │
                ▼  Reconciler (background) ─▶ Platform UI thread
```

`EventStore` (events + reverse index), `ViewRegistry` (open views), and
`DeltaBuffer` all live on the single actor thread (see [04]). No locks, no
atomics — sequential message processing
(`loop-and-reverse-index.md:31-71`).

## Composite vs broad reverse-index keys

The store keys the reverse index on **composite** (conjunctive) tuples by
default. A view registers a `Dependencies` declaration; the registry picks the
most-specific index. Single-axis ("broad") keys are legal but trip a
`nmp-guardrails` debug-build warning (`loop-and-reverse-index.md:81-117`,
ADR-0001).

| Composite key (preferred) | Matches view shape |
|---|---|
| `by_kind_author[(k,a)]` | kinds + authors (timeline of followed authors) |
| `by_kind_e_tag[(k,e)]` | kinds + e-tag refs (reactions/replies to an event) |
| `by_kind_p_tag[(k,p)]` | kinds + p-tag refs (mentions of a pubkey) |
| `by_kind_author_d[(k,a,d)]` | kinds + author + d-tag (param-replaceable) |
| `by_kind_d_tag[(k,d)]` | kinds + d-tag only |

| Broad key (guardrailed) | Cost / guardrail |
|---|---|
| `by_kind[k]` | kinds only — broad-cost flag; every kind:k wakes it |
| `by_author[a]` | authors only — broad-cost flag |
| `by_e_tag[e]` | bare e-tag — broad-cost flag |
| `by_p_tag[p]` | bare p-tag — broad-cost flag |
| `catch_all` (`catch_all_filter`) | considered on **every** insert — explicit debug warning; full-text/regex/time-window scans only |

On insert the event computes its tuple signature — every `(kind, axis-value)`
pair it implies — and lookup unions the small resulting sets
(`loop-and-reverse-index.md:139-165`). Well-shaped apps never touch the broad
buckets, so false wakes go to zero.

**Why composite-first:** run 001 measured a 98% false-wake rate in
`quiet_idle` and 49% in `following_timeline_scroll` under the v0
union-of-axes design. Conjunctive keys eliminate it; the cost is registration
growth (kinds × authors), bounded by the working-set budget (ADR-0001).

## Current budgets

| Budget | Value | Source |
|---|---|---|
| Deltas per view per second | ≤ 60 (matches 60Hz flush) | ADR-0002 |
| Flush cadence | time ≥ 16 ms **or** ≥ 256 buffered deltas, whichever first | `scheduling-and-data-model.md:23-29` |
| Total deltas/sec | naturally ≤ 60 × active views — **no absolute ceiling** | ADR-0002 |
| Working-set memory | ≤ 100 MB at 100 views / 10k hot events | ADR-0003 |
| Hot working set | claimed events + recency window (default 5,000 globally) | ADR-0003 |
| Total cached events on disk | unbounded (backend-quota bounded) | ADR-0003 |
| Per-event allocations (post-1,000-event warmup) | **0** on insert→lookup→recompute→buffer path | ADR-0004 |
| Lookup p99 / recompute p99 gate | ≤ 100 µs | `loop-and-reverse-index.md:165` |

Within-view coalescing at flush is mandatory: the buffer sorts by view id and
applies per-view-kind merge rules before emitting one `AppUpdate::ViewBatch`
(`scheduling-and-data-model.md:31-53`, ADR-0002). Shared facts (author
display, reaction counts) live in **store projections**, not view-on-view
subscriptions, so a kind:0 arrival does a targeted O(items-by-that-author)
patch rather than a per-view scan (`view-deltas-and-projections.md:110-164`).

## reactivity-bench validation (run 002, report 1779051783)

Run 002 passed all ADR-0001..0004 gates. Harness:
`crates/nmp-testing/bin/reactivity-bench/main.rs` (counting `GlobalAlloc`,
gate exit code 2 on failure). Excerpt from
`docs/perf/reactivity-bench/1779051783-run-002.md` (overall: **passed**):

| Scenario | Lookup p99 | Recompute p99 | Max delta/view/s | False-wake | Allocs | Pass |
|---|---:|---:|---:|---:|---:|---|
| quiet_idle | 500 ns | 417 ns | 0.02 | 0.00 | n/a | ✓ |
| following_timeline_scroll | 584 ns | 5,167 ns | 40.70 | 0.00 | 0 | ✓ |
| hashtag_firehose | 125 ns | 208 ns | 58.90 | 0.00 | 0 | ✓ |
| profile_fanout | 4,541 ns | 193,500 ns | 46.38 | 0.00 | 0 | ✓ |
| thread_blowup | 375 ns | 292 ns | 55.64 | 0.00 | 0 | ✓ |
| account_switch | 1,542 ns | 1,333 ns | 1.00 | 0.00 | n/a | ✓ |
| working_set_100_views | 667 ns | 2,083 ns | 2.58 | 0.00 | 0 | ✓ |

Readings: every scenario stays ≤ 60 deltas/view/s (ADR-0002) and **0 false
wakes** (ADR-0001). `0` allocs post-warmup on every steady-state scenario
(ADR-0004; `n/a` where no per-event path runs). `profile_fanout` shows the
fan-out cost the design anticipated — one kind:0 touched 5,000 timeline views,
coalesced 168,978 → 115,308 raw deltas, still under per-view budget. Memory in
`working_set_100_views` (1M cached / 10k hot / 100 views) ≈ 19.8 MB,
well under the 100 MB ADR-0003 gate. (`run-002.md:7-50`.)

## How a ViewModule plugs in

A view kind is a Rust module with a `State` struct and free functions — **no
trait** (closed v1 set; enum dispatch is simpler to debug,
`view-deltas-and-projections.md:5-64`). On `open` it returns
`(State, Dependencies, payload)`; the registry derives the most-specific
composite registration from `Dependencies` — the view never enumerates the
cartesian product itself. `on_event_inserted` / `on_event_removed` /
`on_projection_changed` each return `Option<ViewDelta>`: return `None` when
nothing changed so the buffer stays empty.

## Anti-patterns

- **Broad single-axis keys for typed views.** Declaring `by_kind` /
  `by_author` for a view that is really kinds+authors trips the guardrail and
  reintroduces the v0 98% false-wake rate. Declare the conjunction.
- **Emitting `Some(delta)` when nothing changed.** A spurious delta is a
  false wakeup the buffer cannot coalesce away and a snapshot the host must
  diff. Return `None`; the actor already gates emit on `changed_since_emit`
  (see [04]).
- **Allocating in `on_event_inserted`.** The steady-state per-event path must
  be zero-alloc post-warmup (ADR-0004). A `Vec::push` past capacity, a
  `format!`, or a `clone()` in the hot path fails the bench gate before it
  lands.
- **`catch_all_filter` for anything indexable.** It forces the view onto the
  every-insert slow path and emits a debug guardrail warning. Reserve it for
  genuine full-text / regex / time-window scans.
- **Polling instead of observing — forbidden at every layer.** This is not
  just a UI rule; it applies to all code in the repo:
  - *UI → kernel*: consume `ViewBatch` / snapshots pushed by the actor; never
    call `getState()` on a timer. Polling defeats the rev guard and the 60 Hz
    pacing.
  - *Rust internals*: never `try_recv` + `thread::sleep` in a loop. Use
    blocking `recv()` / `recv_timeout()` so threads wake exactly when work
    arrives. `recv_timeout(Duration::ZERO)` is `try_recv()` — neither belongs
    in a spin loop.
  - *iOS background tasks*: no `Task { while !cancelled { sleep; doWork() } }`.
    Piggy-back on an existing periodic event (e.g. a `periodicTimeObserver`)
    with a wall-clock gate, or react to OS callbacks (`NWPathMonitor`,
    `AVFoundation` delegates, `NotificationCenter`).
  - *Test helpers*: no `poll()` + `sleep` loops. Use the blocking `.wait()`
    method on `SignerOp` or equivalent blocking primitives.
  Every sleep-poll loop is a latency tax, a CPU wake-lock, and a false-wake
  source. If you feel the urge to write one, find the event that replaces it.

See also: [05 — Kernel substrate — the 5 trait families](05-substrate-traits.md) ·
[07 — Subscription planner — Interest → CompiledPlan → wire](07-subscription-planner.md) ·
[18 — Testing — `nmp-testing`, benches, contract tests](18-testing.md) ·
[04 — Actor model (TEA on one thread)](04-actor-and-tea.md)
