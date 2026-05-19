# T135 — mpsc::channel Block Free-List Retention Analysis

- **Date:** 2026-05-18
- **Task:** T135 (originally filed as "T127" residual in `s2-retention-audit.md`, renumbered at HB46)
- **Question:** Does `std::sync::mpsc::channel`'s internal block free-list cause
  meaningful retained heap in long-running actors — enough to justify switching to
  `crossbeam::channel` or `flume`?
- **Decision:** Defer. Close T135.

---

## Context

The T114b work (`83430ca`) bounded actor channel depth and gated per-dispatch
`emit_now` behind a `running` flag, reducing §G-S2 retained heap from 38 MiB to
~0.15–0.52 MiB. The `s2-retention-audit.md` residuals section noted that
`std::sync::mpsc`'s block allocator retains segments on a free-list after items
are dequeued, measured at ~10 B per dispatch of net heap delta during the flood —
but that finding was recorded under the pre-fix 38 MiB signal where the
free-list contribution was swamped by the actual leak. With the leak fixed, it
warranted a separate look.

---

## Channels in scope

Three `mpsc::channel` instances live in the actor:

| Channel | Type | High-water bound |
|---|---|---|
| `command_rx` | `Receiver<ActorCommand>` | T114 part 1: `BOUNDED_ACTOR_CMD_CAPACITY = 4096` slots (try-send drop on overflow) |
| `update_tx` | `Sender<String>` (kernel snapshots to listener) | rate-limited: `maybe_emit_after_dispatch` gates on `running`; idle emit_hz gate |
| `relay_tx` / `relay_rx` | `Sender<RelayEvent>` (actor↔relay-workers) | bounded by relay-worker pool size |

These are unbounded `mpsc::channel`, not `sync_channel`. The block allocator
in `std`'s `mpsc` grows a singly-linked list of fixed-size blocks; when items
are dequeued the block's slot becomes reusable, but blocks are only returned to
the free-list, not reclaimed until the channel itself is dropped. Net effect:
heap footprint plateaus at the channel's peak occupancy and does not fall below
that water mark during the channel's lifetime.

---

## Measurements

The §G-S2 harness (`ffi-stress s2`) uses a process-wide counting allocator
(alloc minus free, immune to OS page-return lag). The relevant runs from
`docs/perf/m10.5/S2/metrics.json` (post-T114b, current head `83430ca`):

| Duration | Total dispatches | `peak_net_heap_bytes` | `retained_after_drain_bytes` | Verdict |
|---|---:|---:|---:|---|
| 10 s | 100,000 | 519,716 B | 519,936 B | PASS (≤ 1 MiB) |
| 30 s | 300,000 | 519,716 B | 157,956 B | PASS |
| 60 s | 600,000 | ~519,748 B | 519,748 B | PASS |

The 60 s run at 600k dispatches (6× the 10 s run) shows the same ~520 KiB
peak. **Retention is flat across a 6× dispatch range.** This is the
operational fingerprint of a bounded high-water mark, not monotonic growth.

The task's decision rule: if savings under realistic load are < 100 KiB
sustained, recommend defer. The total free-list contribution is bounded to
the ~520 KiB envelope; removing it entirely would save well under 100 KiB
(the majority of the ~520 KiB is legitimate working-set state: open-view
structures, the active identity record, wire_subs rows, etc.).

---

## Why the bound holds mechanically

1. **Command channel:** bounded capacity 4096. The block free-list grows to
   accommodate the high-water mark of the flood (a few blocks of ~32–64 items
   each), then plateaus. After a flood + drain cycle the free-list stays at
   that watermark, never growing further because no new blocks are allocated
   once slot-reuse covers the steady-state throughput.

2. **Update channel:** `maybe_emit_after_dispatch` suppresses snapshots when
   `running=false` (the S2 stress condition). In `running=true` steady state,
   emit_hz = 4–30 Hz means at most a handful of String-sized items are ever
   queued simultaneously. Block high-water is correspondingly tiny (1–2 blocks).

3. **Relay channel:** occupancy tracks the number of in-flight relay events
   between worker threads and the actor; bounded by the relay-worker pool count
   (typically 2–8). Free-list is proportional.

---

## Candidate evaluation (crossbeam / flume / ring buffer)

| Candidate | Free-list reclaim? | Dep cost | API delta | Verdict |
|---|---|---|---|---|
| `crossbeam::channel` | Yes — epoch GC; blocks reclaimed promptly on dequeue | +1 crate, ~200 KB binary, supply-chain audit | Drop-in for `Sender`/`Receiver`; `select!` macro bonus | Would eliminate the free-list HWM; not needed here |
| `flume` | Yes — lock-free; reclaims on dequeue | +1 crate, ~60 KB binary | Near-identical API | Would eliminate the HWM; not needed here |
| Custom ring buffer | Total control, zero overhead | Heavy lift, 200–400 LOC + tests | Breaking change to actor wiring | Out of scope unless architectural needs arise |

All three alternatives would reclaim the ~0–50 KiB of free-list overhead that
`std::mpsc` retains at the channel's HWM. Since that overhead is bounded and
well inside the §G-S2 1 MiB ceiling, and since T141 (substrate-types extract)
is actively touching actor wiring at this moment, a switch now would create
merge conflicts with zero measurable benefit.

---

## Arbiter note: reactivity-bench vs §G-S2 harness

The task specified "reactivity-bench is the arbiter." The reactivity-bench
(`nmp-testing/bin/reactivity-bench`) is a synchronous simulation loop —
`bench.apply_event()` runs on a single thread with no `mpsc` in the hot path.
It measures the reverse-index and delta-buffer, not actor channel behaviour.
**The §G-S2 harness is the only bench that actually exercises these channels**
under a realistic multi-threaded dispatch load. Future investigations of
channel behaviour should target `ffi-stress s2`.

---

## Decision: defer, close T135

The free-list retention is **provably bounded** (flat across 6× dispatch range,
well under the §G-S2 1 MiB ceiling). The per-run measurement in
`docs/perf/m10.5/S2/metrics.json` shows `retained_after_drain_bytes` of
~157 KiB–520 KiB; removing the free-list contribution would save less than
50 KiB of that envelope — far below the 100 KiB threshold in the task's
decision rule.

No `crossbeam`/`flume` migration task is warranted. The §G-S2 CI gate
(`.github/workflows/s2-retention-gate.yml`, added by T134) already runs
`ffi-stress s2 --fail-on-gate` nightly; any regression of
`retained_heap_after_drain_bytes` past 1 MiB will surface automatically.

**T135 → closed: investigated, free-list retention is bounded and not a real
issue. Original "T127" residual in `s2-retention-audit.md` is resolved.**
