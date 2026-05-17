# FFI hardening — scenarios (§3)

Ten named failure modes. Each entry: setup, assertion, threading target,
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
   latency; this is the bible-#3 "fire-and-forget" guarantee
   quantified).
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

## S6. Capability lifecycle storms — start/stop/restart

**The bug shape.** `start` → `stop` → `start` cycles leave thread
handles dangling; relay-worker generations get confused; the second
`start` brings up a phantom relay worker.

**Setup.** For each capability handle (today: the relay role pair
Content + Indexer, plus the actor itself), run **1,000
start/stop/restart cycles** in 5 minutes. Cycle:
`nmp_app_start → wait 100ms → nmp_app_stop → wait 100ms → nmp_app_start → wait 100ms`.

**Threading.** Caller drives cycles; actor receives Start/Stop;
relay worker threads spawn and join repeatedly.

**Assertions.**
1. Thread count after 1,000 cycles ≤ baseline + 2 (actor + listener
   only; relay workers gone after every Stop).
2. **Idempotency (bible #7):** two consecutive Starts produce one
   set of relay workers (verified by counting `RelayControl` entries).
3. Generation counter `next_relay_generation` strictly monotonic, no
   wrap (`u64` is fine for 1,000 cycles; harness verifies anyway).
4. RSS growth ≤ **2 MB** over 1,000 cycles.
5. No deadlocks (harness 5-s timeout per cycle).

**Runner.** Rust harness only — these are pure FFI symbol calls.

---

## S7. Error-shape exhaustion — every typed FFI error path

**The bug shape.** A typed FFI error path produces an uncaught
exception, a crash, or — per the §7.2 finding in the parent doc —
*silent loss*: invalid input is dropped without any state field
surfacing the problem.

**Setup.** For every `nmp_app_*` symbol that takes a `*const c_char`
input, exercise the full set of invalid inputs:

| Symbol | Invalid inputs to test |
|---|---|
| `nmp_app_open_author` | NULL, "", " ", "not-hex", 63-char hex, 65-char hex, UTF-8 with non-hex chars |
| `nmp_app_open_thread` | same shapes |
| `nmp_app_open_firehose_tag` | NULL, "" (others valid; tag is unconstrained) |
| `nmp_app_claim_profile` | NULL/empty/non-hex pubkey × {NULL, "", "valid"} consumer_id |
| `nmp_app_release_profile` | same matrix; also: release without prior claim |
| `nmp_app_close_author` / `_thread` | same |
| any `_app` arg | NULL |

Plus: dispatch each symbol with `*mut NmpApp` pointing to a
**freed** allocation (use-after-free probe; must not crash —
ideally hits the null check after `nmp_app_free` zeroes; harness
documents observed behavior).

**Threading.** Caller. Pure FFI exercise.

**Assertions.**
1. Zero crashes / SIGSEGV / SIGABRT across the full matrix.
2. Every silent-no-op input produces a **toast field** in the next
   emit (post §7.2 toast-bridge addition) — current behavior fails
   this assertion; the harness publishes the failing diff and the
   M10.5 fix adds the toast field.
3. Every typed error path's toast string is non-empty and
   actionable (regex match against the catalog in
   `docs/perf/m10.5/error-catalog.md`, generated by this scenario).
4. No error path leaks heap memory (Instruments-Allocations delta = 0
   across the matrix).

**Runner.** Rust harness primary, XCUITest secondary (UI assertion
on toast banner rendering).

---

## S8. Subscription planner DOS

**The bug shape.** A pathological app opens 10,000 distinct views
in 1 s; the planner compiles 10,000 wire filters; the actor's
working set explodes; relay workers can't send fast enough.

**Setup.** Pre-generate 10,000 unique pubkeys. Mix of operations:
5,000 `open_author` + 5,000 `claim_profile` in a 1-s burst, then
5,000 `close_author` + 5,000 `release_profile` over the next 1 s.
Repeat 5 × at 30-s intervals (5 storms total).

**Threading.** Caller bursts; actor compiles; relay workers
serialize REQ frames.

**Assertions.**
1. Peak working-set memory during storm ≤ **150 MB** (planner is the
   dominant allocator).
2. Wire-subscription dedup: subscriber-planner REQ frame count on
   the wire ≤ **2,000** (4× compaction floor — adjust based on
   measured baseline; the assertion is that compaction *happens*,
   not zero).
3. After all closes, planner state size returns to baseline ± **5 %**.
4. No actor-thread stall > **50 ms** during storm.
5. Auto-close mechanism (close on EOSE for one-shot subs, close on
   refcount 0 for view subs) operates correctly under storm.

**Runner.** Rust harness only — this exercises planner internals.

---

## S9. Relay flap — simulated WebSocket disconnect/reconnect storm

**The bug shape.** Each disconnect-reconnect leaks a wire
subscription; bandwidth balloons because every reconnect re-issues
all subs without dedup; the planner doesn't reconcile state.

**Setup.** Use the harness's mock relay (forked from
`crates/nmp-testing` — see harness.md §3.3). Kill and restore the
relay connection at **100 cycles per minute for 10 minutes** (1,000
flaps total). Maintain a fixed open-view set: 1 timeline + 5 authors
+ 2 threads = 8 logical views throughout.

**Threading.** Mock relay (test harness); relay-worker reconnect
loop (`run_relay_worker` in `relay_worker.rs`); actor.

**Assertions.**
1. Wire-subscription count after each reconnect = **8** (one per
   open logical view, no growth).
2. Total bytes-RX over 10 min bounded by **2 × baseline bandwidth**
   (some retransmit is unavoidable; 2× is the budget).
3. Reconnect p99 latency from disconnect-detection to first
   re-issued REQ ≤ **500 ms**.
4. `reconnect_count` field in `RelayStatus` matches the harness's
   injection count exactly.
5. No `OutboundMessage` is silently lost — the kernel's
   `defer_outbound` path captures any send during disconnect, and
   the harness validates the deferred queue drains on reconnect.

**Runner.** Rust harness primary (mock relay), XCUITest secondary
(uses the real `NmpStress` against a control-plane that toggles
network reachability — nightly only).

---

## S10. Long suspend simulation — 60-second background

**Status: conditional on M3 (event store) + M4 (sync watermarks).**
This scenario is specified now and **scheduled to land in M10.5
only if M3+M4 are complete by then.** If not, S10 graduates to
M11.5 and the M10.5 gate excludes it explicitly.

**The bug shape.** iOS suspends the app for 60 s (background). On
resume, the kernel actor's main loop has paused; relay sockets
have timed out; sync watermarks need to drive the catch-up. If the
watermark logic is wrong, the app over-fetches (bandwidth waste) or
under-fetches (missed events).

**Setup.** Open kernel, drive 20 events/sec for 30 s to establish
baseline watermarks. Inject a 60-s synthetic main-loop pause via
`SIGSTOP` on the actor thread (Rust harness only; XCUITest cannot
inject this). On resume:
1. Verify watermark-driven REQ uses `since = last_event_at_ms`.
2. Verify replay completes within **5 s** of resume.
3. Verify state reconciles to the same snapshot a never-suspended
   run would produce (byte-equality on the relevant view payloads).

**Threading.** Actor (suspended); relay workers (sockets time out
during pause); listener (idle during pause).

**Assertions.** (Conditional on M3+M4)
1. Watermark `last_event_at_ms` persists across suspend.
2. Catch-up REQ uses `since` correctly (no full re-fetch).
3. Bandwidth over the 5-s catch-up window ≤ **3 × steady-state
   bandwidth**.
4. Final state snapshot identical to the non-suspended control run.

**Runner.** Rust harness only (XCUITest cannot SIGSTOP the actor).
