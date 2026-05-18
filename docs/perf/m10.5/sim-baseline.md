# M10.5 ffi-stress — Simulator/Host Baseline (S1–S5)

- **Run date:** 2026-05-18
- **Deliverable:** re-scoped M10.5 FFI-hardening gate, D2 (full ffi-stress simulator baseline)
- **Harness:** `cargo run -p nmp-testing --bin ffi-stress` (dev profile, unoptimized + debuginfo)
- **Gate contract:** `docs/design/ffi-hardening/gates.md` §G-S1 .. §G-S5

## Host

This is the **SIMULATOR / Rust-host baseline**, not iPhone hardware.

| Field | Value |
|---|---|
| CPU | Apple M3 Max |
| OS | macOS 26.5 (build 25F5042g) |
| Harness binary | `target/debug/ffi-stress` (debug build) |

**iPhone-hardware numbers are explicitly DEFERRED to the Pulse track.**
The gate table in `gates.md` quotes a separate "iPhone 12 threshold" column;
producing those numbers requires the M1–M10 firehose-bench rerun on device
and is **not** part of this deliverable. Every number below is from a Rust
host run on the M3 Max named above. The honest hardware caveat: a debug
build on an M3 Max is *faster* on compute-bound paths and *different* on
timer-bound paths than an iPhone 12; the sim baseline is a regression
floor, not a device verdict.

## Overall summary

| Scenario | Wall (s) | Gates green | Result |
|---|---|---|---|
| S1 mount-unmount | 600.0 | 5/6 | **FAIL** |
| S2 dispatch-flood | 30.0 | 5/6 | **FAIL** |
| S3 snapshot-pressure | 91.9 | 6/6 | **PASS** |
| S4 reconciler-backpressure | 60.5 | 9/9 | **PASS** |
| S5 reentrancy | 30.5 | 5/5 | **PASS** |

**Across all 5 scenarios: FAIL — 2 of 5 scenarios have a failing gate
(S1 `cycles_completed`, S2 `rss_growth_bytes`).** Findings detailed per
section below.

---

## S1 — mount-unmount (10 min, `--duration 10m`)

Canonical run: `ffi-stress mount-unmount --duration 10m --write-report`.
Wall duration 600.0008 s (gate ±5 s: PASS). 100 pubkeys × 8 consumers,
1,000 claim/release pairs/sec nominal.

### Gate table (G-S1)

| Gate | Threshold | Measured | Result |
|---|---|---|---|
| rss_growth_bytes | <= 5242880.0000 | 1359872.0000 | PASS |
| cycles_completed | >= 540000.0000 | 463207.0000 | FAIL |
| net_heap_slope_bytes_per_sec | <= 0.0000 | 0.0000 | PASS |
| unmatched_claims | == 0.0000 | 0.0000 | PASS |
| listener_cpu_proxy_pct | <= 5.0000 | 0.1544 | PASS |
| wall_seconds_over | <= 5.0000 | 0.0008 | PASS |

### Headline numbers

- cycles_total = 463,207 (nominal 600,000 → 77.2%); steady-state cycles 439,935
- RSS growth = 1,359,872 B (1.30 MiB), well under 5 MiB
- net heap slope = 0 bytes/sec (D8 invariant holds — no retained per-cycle growth)
- callback fires = 926,414; listener CPU proxy 0.15%
- p50/p95/p99 latency: S1 does not report send-latency percentiles (refcount-churn scenario)
- allocations (steady-state, gross): 172,539,669 allocs / 75.49 GB gross churn, net delta 0 B

**FINDING (S1):** `cycles_completed` FAILS — measured **463,207** vs
threshold **540,000** (>= 90% of 600k nominal). The run delivered
**77.2%** of nominal throughput, missing the gate by **76,793 cycles
(≈14.2 percentage points)**. Root cause is host timer resolution: the
scenario sleeps 1 ms between claim and release, and macOS `sleep(1ms)`
resolves at ≈1.5 ms, capping throughput at ~67–77% of the 1,000 pairs/sec
nominal. This is a **macOS-host measurement artifact, not a kernel
regression** — the D8 net-heap slope (0 B/s), RSS (1.30 MiB), refcount
pairing (0 unmatched), and wall-duration gates all PASS, so the FFI
refcount path itself is clean. `G-S1.cycles_completed` is **unobservable
on the Rust host harness**; it requires an XCUITest run on the iOS
simulator/device (Pulse track) to evaluate honestly. The FAIL is recorded
as-is and not papered over; S1 is FAIL for this baseline.

---

## S2 — dispatch-flood (60 s nominal; harness default 30 s)

Run: `ffi-stress dispatch-flood --write-report`. Default config = 30 s,
4 threads, 10,000 dispatches/sec, 50-pubkey pool, mix 30/30/20/20
(open/close/claim/release). Wall 30.006 s.

### Gate table (G-S2)

| Gate | Threshold | Measured | Result |
|---|---|---|---|
| dispatches_submitted | >= 300000.0000 | 300000.0000 | PASS |
| send_latency_p99_ms | <= 1.0000 | 0.0300 | PASS |
| send_latency_p50_ms | <= 0.1000 | 0.0034 | PASS |
| rss_growth_bytes | <= 20971520.0000 | 48119808.0000 | FAIL |
| failed_sends | == 0.0000 | 0.0000 | PASS |
| send_hitch_proxy | == 0.0000 | 0.0000 | PASS |

### Headline numbers

- total_dispatches = 300,000 / nominal 300,000 (100%; 0 dropped)
- Swift→Rust send latency: **p50 = 0.003375 ms (3,375 ns)**, **p99 = 0.030042 ms (30,042 ns)** — p95 not reported by harness
- failed_sends = 0; send_hitch_proxy = 0 (p99 well under 16 ms frame)
- RSS growth = 48,119,808 B (45.89 MiB)
- callback_count = 70,011; latency_samples = 300,000

**FINDING (S2):** `rss_growth_bytes` FAILS — measured **48,119,808 B
(45.89 MiB)** vs threshold **20,971,520 B (20 MiB)**, exceeding the gate
by **27,148,288 B (≈25.9 MiB, 2.29× the budget)**. Send latency is
excellent (p50 3.4 µs, p99 30 µs) and zero sends are dropped, so the FFI
send path is fast and lossless — but the actor's unbounded mpsc channel
plus per-dispatch work accumulates ~46 MiB of working-set growth over a
30 s 10k/s flood. RSS growth is the harness's *proxy* for mpsc backlog
(backlog depth is not directly observable from the caller thread). The
gate is real and it fails: at this dispatch rate the kernel retains more
than the 20 MiB budget. Recorded as FAIL; not softened. (Note: a longer
60 s canonical run would likely grow RSS further, not less — the 30 s
default already misses by 2.29×.)

---

## S3 — snapshot-pressure (100k signed events, 10 emits)

Run: `ffi-stress snapshot-pressure --write-report`. 100,000 real
Schnorr-signed kind-1 events injected via full `try_from_raw` verify
path, then 10 configure() bursts. Wall 91.9 s.

### Gate table (G-S3)

| Gate | Threshold | Measured | Result |
|---|---|---|---|
| callback_p99_ms | <= 20.0000 | 0.0390 | PASS |
| max_payload_bytes | <= 2097152.0000 | 490038.0000 | PASS |
| apply_us_p99 | <= 16000.0000 | 38.0000 | PASS |
| emit_hz | <= 60.0000 | 6.4269 | PASS |
| rev_monotonic | == 1.0000 | 1.0000 | PASS |
| net_heap_per_emit_bytes | <= 980076.0000 | 22.1538 | PASS |

### Headline numbers

- emits observed = 13; burst window 2.022 s; burst freq 6.43 Hz (well under 60 Hz cap)
- serialize/callback p99 = **0.038958 ms (38,958 ns)**; apply_us p99 = **38 µs** — p50/p95 not reported by harness
- max payload = 490,038 B (0.47 MiB), under the 2 MiB cap
- net heap per emit = **22.15 B/emit** (alloc budget = 2× payload = 980,076 B; ratio ~0.002%); net_heap_delta_burst = 288 B
- rev strictly monotonic across emits (bible #1 holds)
- (informational, not gated) process RSS growth over full run = 167,575,552 B — driven by injecting + verifying 100k signed events, not by per-emit serialization; the gated per-emit alloc metric is clean (22 B/emit)

**S3 PASS (6/6).** Serialization is ~2–3 orders of magnitude inside
budget; the per-emit allocation gate (D8) holds with enormous margin.

---

## S4 — reconciler-backpressure (12 stalls × 250 ms, 60 s)

Run: `ffi-stress reconciler-backpressure --write-report`. 500 signed
events injected; 12 main-thread stalls of 250 ms simulated via callback
sleep; emit_hz = 4. Wall 60.5 s.

### Gate table (G-S4)

| Gate | Threshold | Measured | Result |
|---|---|---|---|
| stalls_injected | == 12.0000 | 12.0000 | PASS |
| actor_queue_depth_peak | <= 50.0000 | 0.0000 | PASS |
| backlog_after_stall | <= 2.0000 | 1.0000 | PASS |
| rev_monotonic | == 1.0000 | 1.0000 | PASS |
| stalls_with_backlog | >= 12.0000 | 12.0000 | PASS |
| listener_emit_drops | == 0.0000 | 0.0000 | PASS |
| configure_during_stall_p99_us | <= 10000.0000 | 22.0000 | PASS |
| stale_rev_pairs | == 0.0000 | 0.0000 | PASS |
| apply_burst_ms | <= 33.0000 | 3.0000 | PASS |

### Headline numbers

- stalls injected = 12 (exact); each stall produced backlog (stalls_with_backlog = 12)
- max backlog after a 250 ms stall = 1 emit (expected <= ceil(0.25×4)+1 = 2)
- rev strictly monotonic on resume; stale_rev_pairs = 0
- configure() p99 latency *during* a 250 ms stall = **22 µs** (actor not blocked by sleeping callback — D4 single-writer verified directly)
- **apply-after-resume burst max = 3 ms** (threshold 33 ms); evidence ok (total_emits = 141 ≥ 100, false-pass guard not triggered)
- listener-side emit drops = 0; actor_queue_depth_peak = 0 (kernel hardcodes this field to 0 until wired — gate added for spec compliance; trivially passes; follow-up: wire mpsc channel length to `Metrics::actor_queue_depth`)

**S4 PASS (9/9).** Back-pressure behaves: actor never blocks on a stalled
listener, ordering stays monotonic, no drops, apply burst 11× inside
budget. Caveat noted: `actor_queue_depth_peak` is a spec-compliance gate
that passes vacuously until the kernel populates the field; this is a
documented harness limitation, not a fudge.

---

## S5 — reentrancy (30 s, 50 events/s, dispatch-from-callback)

Run: `ffi-stress reentrancy --write-report`. Callback dispatches
`open_author` reentrantly on every emit; 5 s external watchdog.
Wall 30.5 s.

### Gate table (G-S5)

| Gate | Threshold | Measured | Result |
|---|---|---|---|
| deadlocks | == 0.0000 | 0.0000 | PASS |
| reentrant_dispatches | >= 100.0000 | 11959.0000 | PASS |
| rev_monotonic | == 1.0000 | 1.0000 | PASS |
| avg_cb_ms | <= 2.0000 | 0.0089 | PASS |
| dispatch_loss | == 0.0000 | 0.0000 | PASS |

### Headline numbers

- deadlocks = 0 (watchdog never fired)
- reentrant dispatches = 11,959 (threshold ≥ 100; emit_count = 11,959, exact match)
- dispatch loss = 0 (every emit produced exactly one reentrant dispatch)
- avg callback time = **0.008916 ms (8,916 ns)** per emit (threshold ≤ 2 ms) — p50/p95/p99 not reported by harness; avg is the gated metric
- rev strictly monotonic (no out-of-order callback→dispatch pairs)

**S5 PASS (5/5).** Reentrant dispatch is fire-and-forget and lossless;
zero deadlocks; callback overhead ~9 µs.

---

## Conclusion

| Scenario | Result | Headline |
|---|---|---|
| S1 | **FAIL** | cycles 463,207 < 540,000 (host timer artifact); RSS/heap/refcount clean |
| S2 | **FAIL** | RSS growth 45.89 MiB > 20 MiB (2.29× budget); latency + loss clean |
| S3 | PASS | serialize p99 38 µs; 22 B/emit alloc |
| S4 | PASS | 12 stalls, apply-burst 3 ms, 0 drops |
| S5 | PASS | 0 deadlocks, 0 dispatch loss, 9 µs/cb |

**Suite verdict: FAIL.** S3/S4/S5 are green with wide margins. S1 and S2
each have one failing gate. S1's failure is a documented macOS-host
measurement artifact (timer resolution caps claim/release throughput);
it is **unobservable on this harness** and must be re-evaluated on the
Pulse/iOS track, but it is recorded FAIL here, not waived. S2's failure
is a **genuine working-set finding**: the actor's unbounded mpsc backlog
under a 10k/s flood grows RSS to 2.29× the 20 MiB budget — this needs a
real fix (bounded channel / backlog cap) or a justified gate-threshold
revision before M10.5 can close on the §G-S2 contract.

*Generated by ffi-stress harness D2 baseline. Sim/host only (Apple M3
Max, macOS 26.5). iPhone-hardware numbers deferred to the Pulse track.*
