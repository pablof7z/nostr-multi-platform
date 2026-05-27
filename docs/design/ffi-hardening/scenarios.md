# FFI hardening — scenarios (§3)

Eleven named failure modes. Each entry: setup, assertion, threading target,
runner (Rust harness / XCUITest / Sonnet-agent), and numeric threshold.
Numeric thresholds are quoted here for clarity; the canonical exit-gate
table lives in [`gates.md`](./gates.md).

Conventions:
- "dispatch" = a single Swift→Rust call through any `nmp_app_*` symbol.
- "emit" = one full update payload delivered to the registered callback.
- "view-handle wrapper" = the iOS-side refcounted entity created when a
  SwiftUI component mounts a `useProfile(pubkey)`-style observer (in the
  current `NmpStress` codebase this maps to `KernelModel.claimProfile` +
  `releaseProfile`; ADR-0005).

---

## S1. Mount/unmount churn — view-handle wrapper refcount

**The bug shape.** Wrappers (`claim_profile` / `release_profile`) leak
internal references; the kernel's per-pubkey refcount table grows
without bound; eventually the platform shadow holds entries no
component still needs.

**Setup.** Drive the kernel with a closed-loop of 1,000
claim/release pairs per second across 100 unique pubkeys (rotating
LRU). Sustain for **10 minutes** (600,000 cycles). Each cycle:
`claim_profile(pk_i, consumer_id_i) → 1ms wait → release_profile(pk_i, consumer_id_i)`.

**Threading.** Driver thread = Swift main (XCUITest) **or** the
single-threaded executor in the Rust harness. Actor thread receives
the storm; listener emits batched updates.

**Assertions.**
1. Final refcount for every pubkey = 0.
2. `Kernel::interest_refcount(pk).is_none()` for every pubkey after a
   grace-period (configurable, default 30 s — ADR-0005).
3. Process RSS after 10 min ≤ baseline + **5 MB**.
4. Instruments → Leaks → 0 retained-by-cycle leaks.
5. Allocator (counting allocator from `reactivity-bench`) shows
   slope ≤ 0 bytes/sec post-warmup (first 30 s).

**Runner.** Primary: Rust harness `ffi-stress mount-unmount-churn`.
Secondary (nightly only): XCUITest `S1MountUnmountChurn.swift` driving
real `NmpStress` with Instruments-Leaks attached.

**Numeric gate.** See gates.md §G-S1.

---

## S2. Dispatch flood

**The bug shape.** Swift dispatches faster than the actor can drain;
the mpsc channel grows unbounded; OOM eventually. Or: the FFI symbol
itself takes a lock that blocks the main thread.

**Setup.** **10,000 dispatches per second** for 60 s across **N=4**
caller threads (matching iOS's typical concurrency budget). Mix:
30 % `open_author` (valid pubkey from a pool of 50), 30 %
`close_author`, 20 % `claim_profile`, 20 % `release_profile`. All
inputs valid (no validation-path flooding — that's S7).

**Threading.** Four caller OS threads (in the Rust harness, four
`std::thread`; in the XCUITest variant, four `DispatchQueue.global()`
queues). Actor thread is the bottleneck under test.

**Assertions.**
1. No dispatch call takes > **1 ms** at p99 (Swift→Rust channel send
   latency; this is the "fire-and-forget" guarantee
   quantified — `dispatch()` never blocks).
2. Main thread (XCUITest variant) is never blocked > **16 ms**
   (measured via XCTest's `XCTHitchMetric`).
3. Actor mpsc backlog never exceeds **10,000 messages** (configurable
   harness-side check via `actor_queue_depth` field already exposed in
   `KernelMetrics`).
4. Zero dropped messages: count of `_ = tx.send(...)` failures = 0.
5. RSS growth over 60 s ≤ 20 MB.

**Runner.** Rust harness primary. XCUITest secondary because the
real-world iOS main-thread budget is the load-bearing constraint.

---

## S3. Snapshot pressure — `AppUpdate::FullState` with 100k events

**The bug shape.** Marshal cost for a full-state snapshot grows
super-linearly; the listener thread spends > 1 emit-interval
serializing JSON; the reconciler exceeds 60 Hz; the iOS main thread
spends every frame applying snapshots.

**Setup.** Pre-load kernel state via a captured firehose-bench trace
(`docs/perf/firehose-bench/traces/m10.5-snapshot.cap`, captured
expressly for this scenario) containing **100,000 stored events**
across 3,000 authors. Force a full-state emit by calling
`nmp_app_configure` (which currently triggers `emit_now`) ten times
in a row to amortize the JSON cost over ten observations.

**Threading.** Actor builds the update; listener serializes; Swift
main applies. The bottleneck under test is the **listener
serialization + main-thread apply** path.

**Assertions.**
1. Per-emit JSON serialization wall-time p99 ≤ **20 ms** (Rust side).
2. Payload size ≤ **2 MiB** (if it exceeds, the harness fails and
   asks for bible-#10 granular-update variants).
3. Swift `apply_us` (already instrumented in `KernelModel.apply`) p99
   ≤ **16 ms**.
4. End-to-end reconciler frequency stays ≤ **60 Hz** (configured cap
   is 12 Hz today; the harness verifies the cap is honored under
   pressure rather than asserting the cap value itself).
5. Allocations per emit ≤ **payload_bytes × 2** (no quadratic-copy
   regression).

**Runner.** Rust harness primary (measures Rust-side); XCUITest
secondary (measures Swift-side).

---

## S4. Reconciler back-pressure — main thread stalled 250 ms

**The bug shape.** When the iOS main thread blocks (e.g.,
file-picker, modal sheet, large layout pass), the listener-thread
emits accumulate. On resume, the app applies a flood and visibly
hitches, or worse, the actor stalls because its `update_tx` channel
fills.

**Setup.** Start the kernel, open following + author + thread views,
drive 100 events/sec for 60 s. Periodically inject a **250 ms
synchronous sleep on the main thread** (XCUITest:
`Thread.sleep(forTimeInterval: 0.25)` inside the reconciler closure).
Repeat every 5 s for the 60-s window → **12 stalls total**.

**Threading.** Main thread (artificially stalled); listener thread
(accumulates emits); actor (must not block on `update_tx.send`).

**Assertions.**
1. Actor `actor_queue_depth` never grows during a stall (the listener
   thread is the queue under back-pressure, not the actor's command
   queue).
2. Listener `update_rx` backlog after each 250 ms stall ≤ ⌈ 250 ms
   × emit_hz ⌉ + 1 = **2** messages (at 4 Hz default) or **4** (at 12
   Hz max).
3. On stall release, the main thread applies all backlogged emits in
   monotonic `rev` order (bible #1).
4. Stale-rev filter in `KernelModel.apply` correctly drops any
   intermediate revs (already implemented at KernelModel.swift:139;
   harness validates).
5. No emit is dropped — the listener delivers each one in order.

**Runner.** XCUITest only (the bug shape is iOS-main-thread specific).

---

## S5. Reentrancy — dispatch from inside reconciler callback

**The bug shape.** A SwiftUI `.onChange` handler observes a kernel
update and immediately calls `kernel.openAuthor(...)`. If the FFI
symbol takes a lock the listener thread also takes, deadlock. If it
re-enters synchronously, message ordering can invert.

**Setup.** Register a callback that, on every emit where
`update.metrics.eventsSinceLastUpdate > 0`, immediately dispatches
`open_author` for the first item in the timeline. Sustain for 30 s
with a 50 events/sec inflow.

**Threading.** Callback runs on **listener thread**; dispatch
enqueues to the actor's command channel which the **actor thread**
drains. The bug shape requires the actor to be processing a message
when the callback fires (race window).

**Assertions.**
1. Zero deadlocks (harness times out after 5 s = fail).
2. Message order preserved — for every emit-then-dispatch pair, the
   resulting `OpenAuthor` action is processed strictly **after** the
   emit that triggered it (verified via `rev` monotonicity in the
   subsequent emit referencing that author view).
3. No dispatch loss under reentrant pressure.
4. Listener thread CPU time per emit ≤ **2 ms** even with the
   reentrant dispatch path active.

**Runner.** Rust harness primary, XCUITest secondary.

---

Scenarios S6–S11 (capability storms, error exhaustion, planner DOS,
relay flap, long suspend, memory RSS instrumentation) are in
[`scenarios-detail.md`](./scenarios-detail.md).
