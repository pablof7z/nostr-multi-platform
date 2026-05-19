# M10.5 — S2 Drain Analysis (leak-vs-transient tiebreaker)

- **Date:** 2026-05-18
- **Question:** is S2's working-set overrun a *transient* backpressure spike
  (backlog that drains → §G-S2 peak threshold could be honestly revised,
  "Option B") or *retained* heap that is never reclaimed (a real unbounded-growth
  bug → a bounded-channel/backpressure fix is mandatory, "Option A")?
- **Method:** added a post-flood drain phase to
  `crates/nmp-testing/bin/ffi-stress/s2_dispatch_flood.rs`. After the 30 s /
  10k-dispatch-per-s / 4-thread flood, the actor is left running and the
  **counting allocator's NET live heap** (alloc-minus-free — immune to the OS
  not returning freed pages, unlike RSS) is polled every 500 ms until it
  stabilises (3 samples within a 256 KiB band) or a 30 s drain budget elapses.
  Baseline / peak / drained snapshots are diffed.

## Result — RETAINED, not transient

| Metric (counting allocator, NET live heap) | Value |
|---|---|
| `peak_net_heap_bytes` (flood end) | **39,918,768 B (≈ 38.07 MiB)** |
| `retained_heap_after_drain_bytes` | **39,866,908 B (≈ 38.02 MiB)** |
| `reclaimed_by_drain_bytes` | **51,860 B (≈ 51 KiB — 0.13 %)** |
| `drain_seconds` (time to stabilise) | **1.51 s** |
| `retained_heap_after_drain_bytes` gate (≤ 1 MiB) | **FAIL** |

The net heap was **already flat** at flood end (stabilised in 1.5 s — the
minimum the 3×500 ms stability detector can report — meaning nothing was
draining). Of ~38 MiB allocated during the flood, **0.13 % was reclaimed**.
This is **genuine unbounded retention, not a recoverable backpressure spike.**

### RSS is noisy; the allocator metric is the truth

This run's `rss_growth_bytes` *passed* (17.1 MiB ≤ 20 MiB) — **lower** than D2's
original 45.9 MiB. RSS is run-dependent (page-return timing, allocator arena
state) and is a poor gate signal. The counting-allocator NET-heap metric is
deterministic and shows the real picture regardless of what RSS reports: **~38
MiB allocated and never freed** under a 30 s flood. The original D2 "S2 RSS
FAIL" finding is therefore not just confirmed but **sharpened** — the defect is
retained net heap, not noisy peak RSS.

### Honest caveat (does not change the verdict)

The counting allocator is process-global, so harness-side allocations are
included in `peak`/`drained`: chiefly the latency `Vec` (~300k × 8 B ≈ 2.4 MiB)
plus the drain curve and JSON. Netting out a generous ~3–5 MiB of harness
overhead still leaves **≥ ~33 MiB of kernel-retained heap** that scales with
*dispatch count*, not with the *50-pubkey working set*. The verdict is
overwhelming with or without the caveat.

## Diagnosis

The S2 mix is 30 % `open_author` / 30 % `close_author` / 20 % `claim_profile`
/ 20 % `release_profile` over a **50-pubkey pool** — net subscription/claim
intent is balanced on 50 distinct keys. A working-set-bounded kernel (D8)
should retain *O(50 pubkeys)* of state — kilobytes. Instead it retains ~38 MiB
that scales with the **~300,000 total dispatches** (~127 B/dispatch). Heap
growing with operation count rather than working-set size is the signature of
**unbounded per-operation accumulation in the actor** (an ever-growing queue
backlog held until shutdown, and/or an unbounded log / dedup-set / metrics
vector / per-command record that is never evicted). This is a direct **D8**
violation ("working set bounded · allocations linear in active-view count,
never in cached-event count").

## Decision (made by the data)

- **Option B (revise the §G-S2 threshold) is foreclosed.** It was only
  defensible if the memory drained; it does not (0.13 % reclaimed). Revising
  the threshold would paper over a real unbounded-growth bug — exactly what the
  M10.5 honesty mandate forbids.
- **Option A (bounded channel / backpressure + bounded actor-side state) is
  mandatory** before §G-S2 and the D8 working-set sub-clause can honestly close.

### Ownership / next step

The fix is in `crates/nmp-core/**` (actor command channel + whatever per-op
structure accumulates) — **out of this FFI-hardening workstream's scope**
(scoped away from `nmp-core` to avoid collision with the kernel-owning
session). This analysis hands the kernel session a precise, reproducible
target:

1. Make the actor command channel **bounded** with a try-send + drop/coalesce
   policy (must stay D6 fire-and-forget: never block the FFI caller; never
   surface an error across FFI). Add a `dropped_dispatches` metric.
2. Audit the actor for any structure that grows per-dispatch rather than
   per-working-set-entry (log, dedup set, metrics vec, unevicted records) and
   bound/evict it.
3. Re-run `ffi-stress dispatch-flood --write-report`; the
   `retained_heap_after_drain_bytes` gate (now in the harness) is the
   regression check — it must drop to ≤ 1 MiB.

Tracked as the headline open M10.5 finding; see `doctrine-review.md` §D8 and
the re-scope addendum. M10.5 closes on the achievable subset **with this
explicitly open** — it is not waived, not threshold-revised, and routed to the
kernel owner with evidence.

*Harness change: `s2_dispatch_flood.rs` +drain phase (327 LOC, > 300 soft cap,
< 500 hard — cohesive measurement block, repo precedent per PD-003). Raw run:
`docs/perf/m10.5/S2/metrics.json`.*
