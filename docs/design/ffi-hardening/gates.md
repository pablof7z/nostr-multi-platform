# FFI hardening — exit gates (§5) and doctrine review (§8)

Two tables in this file:
1. **§G** numeric exit gates per scenario per metric — the complete
   contract; every cell is a specific number with units. No "fast
   enough", no "acceptable".
2. **§D** doctrine D0–D5 review checklist — each line item maps to
   a scenario, an audit document, or an explicit code-comment task.

The values here are the **simulator** baseline (iPhone 16 Pro
simulator on Apple Silicon Mac). The iPhone 12 hardware baseline is
quoted as a separate column where scaling matters; the per-device
coefficient is documented in
`docs/perf/m10.5/iphone12-baseline.md` (produced by the M1–M10
firehose-bench rerun, not by this design doc).

---

## §G. Numeric exit gates

### G-S1. Mount/unmount churn (10 min)

| Metric | Op | Sim threshold | iPhone 12 threshold |
|---|---|---|---|
| Wall duration | `==` | 600 s ± 5 s | 600 s ± 5 s |
| Cycles completed | `>=` | 540,000 (90 % of 600k nominal) | 360,000 (60 %) |
| Final per-pubkey refcount | `==` | 0 for every key | 0 |
| Unmatched claim/release | `==` | 0 | 0 |
| Process RSS growth | `<=` | 5 MiB | 8 MiB |
| Counting allocator slope (post-warmup) | `<=` | 0 bytes/sec | 0 bytes/sec |
| Instruments retained-by-cycle leaks | `==` | 0 | 0 |
| Listener thread CPU avg | `<=` | 5 % | 8 % |

### G-S2. Dispatch flood (60 s, 10 000/s, 4 threads)

| Metric | Op | Sim threshold | iPhone 12 threshold |
|---|---|---|---|
| Dispatches submitted | `>=` | 600,000 | 600,000 |
| Per-dispatch Swift→Rust send latency p50 | `<=` | 100 µs | 200 µs |
| Per-dispatch Swift→Rust send latency p99 | `<=` | 1 ms | 2 ms |
| Actor mpsc backlog peak | `<=` | 10,000 | 10,000 |
| Main thread hitches > 16 ms (XCTHitchMetric) | `==` | 0 | 0 |
| Dropped messages (Sender::send failures) | `==` | 0 | 0 |
| RSS growth | `<=` | 20 MiB | 30 MiB |

### G-S3. Snapshot pressure (100 k events, 10 emits)

| Metric | Op | Sim threshold | iPhone 12 threshold |
|---|---|---|---|
| Per-emit JSON serialization wall-time p99 (Rust) | `<=` | 20 ms | 40 ms |
| Per-emit payload size | `<=` | 2 MiB | 2 MiB |
| Swift `apply_us` p99 | `<=` | 16 ms (16 000 µs) | 32 ms |
| End-to-end reconciler frequency | `<=` | 60 Hz | 60 Hz |
| Allocations per emit | `<=` | `2 × payload_bytes` | `2 × payload_bytes` |
| Counting allocator: temporary peak during emit | `<=` | `3 × payload_bytes` | `3 × payload_bytes` |
| `rev` monotonicity across the 10 emits | `==` | strictly increasing | strictly increasing |

### G-S4. Reconciler back-pressure (12 stalls × 250 ms, 60 s)

| Metric | Op | Sim threshold | iPhone 12 threshold |
|---|---|---|---|
| Stalls injected | `==` | 12 | 12 |
| Actor `actor_queue_depth` peak during any stall | `<=` | 50 | 100 |
| Listener `update_rx` backlog after each stall | `<=` | `ceil(0.25 × emit_hz) + 1` | same |
| Rev order on resume | `==` | strictly monotonic | strictly monotonic |
| Stale-rev filter drops counted | `>=` | 1 per stall (12 total) | same |
| Total emits dropped (listener-side) | `==` | 0 | 0 |
| Apply-after-resume burst max | `<=` | 33 ms | 50 ms |

### G-S5. Reentrancy (30 s, 50 events/s, callback dispatches)

| Metric | Op | Sim threshold | iPhone 12 threshold |
|---|---|---|---|
| Deadlocks (5 s watchdog) | `==` | 0 | 0 |
| Dispatch-from-callback emits processed | `>=` | 100 (over 30 s) | 100 |
| Out-of-order callback→dispatch pairs | `==` | 0 | 0 |
| Listener thread CPU per emit avg | `<=` | 2 ms | 4 ms |
| Dispatch loss | `==` | 0 | 0 |

### G-S6. Capability lifecycle storms (1 000 cycles, 5 min)

| Metric | Op | Sim threshold | iPhone 12 threshold |
|---|---|---|---|
| Cycles completed | `==` | 1,000 | 1,000 |
| Thread count delta from baseline | `<=` | 2 | 2 |
| Active `RelayControl` entries after final Stop | `==` | 0 | 0 |
| Relay-worker generation counter wrap | `==` | 0 (no wrap) | 0 |
| RSS growth | `<=` | 2 MiB | 4 MiB |
| Cycle wall-time p99 | `<=` | 250 ms | 500 ms |
| Deadlocks (5 s per-cycle watchdog) | `==` | 0 | 0 |

### G-S7. Error-shape exhaustion (full matrix)

| Metric | Op | Sim threshold | iPhone 12 threshold |
|---|---|---|---|
| Crashes / signals across full input matrix | `==` | 0 | 0 |
| Crashes on NULL app pointer calls | `==` | 0 | 0 |
| Toast field populated on every silent-no-op path | `==` | 100 % of validation-fail paths | 100 % |
| Toast strings non-empty + actionable (regex match) | `==` | 100 % | 100 % |
| Instruments-Allocations delta over full matrix | `==` | 0 (no leak per error path) | 0 |
| Symbols × invalid-input variants exercised | `>=` | 70 (14 symbols × 5 variants avg) | 70 |

### G-S8. Subscription planner DOS (5 × 10 k storm, 30 s gap)

| Metric | Op | Sim threshold | iPhone 12 threshold |
|---|---|---|---|
| Peak working-set RSS during storm | `<=` | 150 MiB | 200 MiB |
| Wire-REQ frame count per 10 k OpenViews | `<=` | 2,500 (4× dedup floor) | 2,500 |
| Planner state size after all closes | `<=` | baseline × 1.05 | baseline × 1.10 |
| Actor stalls > 50 ms during storm | `==` | 0 | 0 |
| Auto-close success rate (every release → close) | `==` | 100 % | 100 % |
| Wire subscription leaks after 5 storms | `==` | 0 | 0 |

### G-S9. Relay flap (100/min × 10 min = 1 000 flaps)

| Metric | Op | Sim threshold | iPhone 12 threshold |
|---|---|---|---|
| Flaps injected | `==` | 1,000 | 1,000 |
| Wire-subscription count after each reconnect | `==` | 8 (one per logical view) | 8 |
| `reconnect_count` field matches injected count | `==` | 1,000 | 1,000 |
| Reconnect→first-REQ-out p99 latency | `<=` | 500 ms | 1,000 ms |
| Total bytes-RX over 10 min | `<=` | 2 × baseline bandwidth | 2 × baseline |
| Deferred-outbound queue drain on reconnect | `==` | 100 % | 100 % |
| Subscription leaks (count after final reconnect) | `==` | 0 | 0 |

### G-S10. Long suspend (conditional on M3+M4)

| Metric | Op | Sim threshold | iPhone 12 threshold |
|---|---|---|---|
| Watermark persisted across suspend | `==` | true | true |
| Catch-up REQ uses `since` filter | `==` | true | true |
| Catch-up window wall-time | `<=` | 5 s | 8 s |
| Catch-up bandwidth vs steady-state | `<=` | 3× | 3× |
| Post-resume state snapshot equals control | `==` | byte-equal (relevant payloads) | byte-equal |

---

## §G.1 Aggregate gates (apply across all scenarios)

| Metric | Op | Threshold | Source |
|---|---|---|---|
| Total Instruments-Leaks across the suite | `==` | 0 | every scenario with `--instruments` |
| Total crashes / panics across the suite | `==` | 0 | every scenario |
| M1–M10 firehose-bench rerun regression (p99 ms, any scenario) | `<=` | +5 % vs baseline | plan.md §M10.5 exit-gate row 2 |
| Pre-merge runtime budget per PR | `<=` | 15 min wall-time on `macos-14` | ci.md §1 |
| Nightly runtime budget on Mac mini self-hosted | `<=` | 90 min | ci.md §2 |

---

## §D. Doctrine D0–D5 review checklist

Cardinal doctrines per `docs/product-spec/overview-and-dx.md` §1.5.
Each line item names the artifact that proves it.

> **Note.** The task brief mentioned "D0–D5". The canonical list in
> the spec **is exactly six items: D0, D1, D2, D3, D4, D5.** This
> file follows that list. (The aim.md §6 list of 12 items is a
> larger doctrine set the FFI hardening pass deliberately does
> not redundantly re-prove — items beyond D0–D5 are covered by
> earlier milestones' own gates.)

### D0. Kernel never grows app nouns

- ✅ **Proof:** [debt-inventory.md §3 D0 audit](../../perf/m10.5/debt-inventory.md) — verified
  no app-domain types in `nmp-core` substrate.
- ✅ **Stress proof:** S6 (capability lifecycle storms) does
  1 000 start/stop/restart cycles; the kernel's capability set is
  unchanged across all cycles (no dynamic registration of new
  noun types).
- 📝 **Sign-off:** auditor signature line in
  `docs/perf/m10.5/doctrine-review.md` § D0.

### D1. Best-effort rendering — render now, refine in place

- ✅ **Stress proof:** S3 (snapshot pressure) — every emit must
  satisfy the placeholder-then-refine contract: missing display
  names → shortened-npub placeholders are present (no `None`); when
  kind:0 arrives, the next emit updates in place. Assertion: for
  every author with placeholder text in emit N, emit M > N where
  the kind:0 arrived must contain the resolved text and an
  unchanged `id`.
- ✅ **Stress proof (back-pressure path):** S4 (reconciler
  back-pressure) — on stall release, the timeline renders with
  placeholders immediately, not spinner-blocked. Emit ordering is
  monotonic; no frame is dropped.
- ✅ **Code proof:** iOS `ProfileCard.placeholder(pubkey:)` renders
  immediately from `debt-inventory.md` §3 D1 audit; no spinner, no
  `nil` guard blocking render.
- 📝 **Sign-off:** doctrine-review.md § D1.

> **Note on S10.** S10 (long suspend) would provide additional D1
> evidence for the resume path, but S10 is conditional on M3+M4
> (persistence + watermarks), which are not complete at M10.5 close.
> S10 is deferred to M11.5 or whenever M3+M4 land; it is not used as
> doctrine sign-off evidence here. D1 is signed off on S3 + S4 + code
> proof above.

### D2. Reactivity contract — composite reverse index, ≤60Hz/view, working-set bound

- ✅ **Stress proof:** S2 (dispatch flood), S3 (snapshot pressure),
  S8 (planner DOS) all assert reconciler frequency stays bounded
  (currently capped at 12 Hz; the doctrine says ≤ 60 Hz; the gate
  is the 60 Hz ceiling, the configured cap is internal policy).
- ✅ **Stress proof (working set):** S8 asserts planner state
  returns to baseline ± 5 % after close storms.
- ✅ **Composite reverse index:** S8 asserts wire-REQ dedup
  (4× compaction floor); the reverse index drives the dedup.
- 📝 **Sign-off:** doctrine-review.md § D2.

### D3. Errors never cross FFI

- ⚠️ **Current state:** debt-inventory §3 D3 audit concludes
  errors-as-data crosses FFI correctly via `RelayStatus.last_error`
  / `last_notice`. App-action error paths are
  **silently no-op** (see parent doc §7.2).
- ✅ **Stress proof + remediation:** S7 (error-shape exhaustion)
  exercises every invalid-input path. The M10.5 deliverable
  adds a `toast: Option<String>` field to the JSON update
  payload and populates it from S7's failure paths. The
  schema change is additive and non-breaking.
- 📝 **Sign-off:** doctrine-review.md § D3, with the explicit
  note that this milestone *closes* the D3-incomplete state
  identified in the debt inventory.

### D4. Single writer per fact — caches derive

- ✅ **Proof:** debt-inventory §3 D4 audit — single-threaded actor
  is the only writer; `KernelModel` on the iOS side is
  `@MainActor`-isolated so derived state has a single writer per
  layer.
- ✅ **Stress proof:** S5 (reentrancy) — the reentrant
  dispatch-from-callback path does not violate single-writer
  because the dispatch enqueues a command for the actor; the
  callback does not mutate kernel state directly.
- ✅ **Stress proof:** S1 (mount/unmount) — refcount table is
  only mutated by the actor; verified by harness asserting no
  refcount-table mutation on any other thread (via
  `loom`-style instrumentation in the harness binary only, not
  prod).
- 📝 **Sign-off:** doctrine-review.md § D4.

### D5. Capabilities report, never decide

Canonical (plan.md:9): capabilities surface position events to the
iOS layer; **no policy decisions are made at the bridge**.

- ✅ **Code proof:** `CapabilityModule` trait (`substrate/capability.rs`)
  defines typed `Request`/`Result` pairs — modules *report* capability
  results back to the platform; they never decide what to do with them.
  The `callback_interface_name()` entry point delivers results to the
  iOS layer as data, not as control signals.
- ✅ **Capability evidence (M10.5 surface):** The relay-role capability
  (content + indexer) is the only active module today. `RelayStatus` is
  emitted as an update field — the kernel reports relay position; iOS
  renders it. No routing decisions are made at the bridge.
- ✅ **M11 prep evidence:** Both `AudioPlaybackCapability` and
  `EmbeddingCapability` (planned for M11) follow the same pattern: the
  kernel emits position/ready events back to the platform; the platform
  renders them. No capability module will acquire routing authority.
- ✅ **Stress proof:** S6 (capability lifecycle storms) — 1,000
  start/stop/restart cycles verify the relay capability lifecycle without
  any module gaining decision authority; all `RelayControl` entries are
  closed after every Stop.
- 📝 **Sign-off:** doctrine-review.md § D5.

---

## §D.1 Doctrine sign-off artifact

`docs/perf/m10.5/doctrine-review.md` is produced at the *end* of
M10.5, not as part of this design. The structure is:

```
# M10.5 Doctrine Review

| Doctrine | Status | Evidence | Reviewer | Date |
|---|---|---|---|---|
| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
| D1 | PASS | S3 + S4 metrics.json + S3/screenshots (placeholder-then-refine path) | <name> | <date> |
| D2 | PASS | S2/S3/S8 metrics.json | <name> | <date> |
| D3 | PASS | S7 metrics.json + toast-bridge merge SHA | <name> | <date> |
| D4 | PASS | debt-inventory §3 D4 + S5/S1 metrics.json | <name> | <date> |
| D5 | PASS | debt-inventory §3 D5 + S6 metrics.json + capability.rs code review | <name> | <date> |

## Notes
<any caveats, deferrals, follow-ups>
```

A PASS in every row + the §7.1 grep gate yielding 0 hits = M10.5
ready to close. Anything less = the milestone is open.
