# Design: Firehose-bench (end-to-end stress harness)

> **Audience:** Framework contributors. Complements `reactivity-bench` (synthetic, Rust-only); this harness exercises the full system end-to-end.

> **Status:** Draft. Lives in `crates/nmp-testing/bin/firehose-bench/`. Gated as a Phase 9 deliverable in `plan.md`; capture-mode scenarios run as early as Phase 2.

> **Prerequisites:** `product-spec.md` (especially §7.1 EventStore, §7.2 planner, §7.3 outbox, §7.8 sync engine, §7.16 metrics); `reactivity.md`; ADRs 0001–0005.

---

## 1. Why this exists

`reactivity-bench` proved the algorithmic core scales: reverse-index lookup, view recompute, delta emission. It is synthetic — single Rust process, no relays, no disk, no real-world event distribution, no FFI.

That's not enough. The framework's promise extends through:

- **Tier 1 (durable storage):** LMDB / SQLite / IndexedDB writes under sustained inbound rate, fsync cost, compaction, query latency against a primed DB, cold-start time.
- **Real Nostr traffic:** bursty arrival, multi-relay duplicate fan-in, signature verification cost at scale, mixed-kind streams (kind:1 + kind:7 + kind:9735 + kind:5 deletes interleaved), real-world relay reliability.
- **Outbox routing at scale:** discovering 1k authors' mailboxes, fanning out across N relays, handling slow/dead relays gracefully.
- **Negentropy against real relays:** capability negotiation, bytes-saved measurement, partial-coverage behavior.
- **FFI marshaling under steady-state pressure:** are we actually staying ≤ 60Hz with real event arrival patterns?
- **Memory over time:** leaks, fragmentation, LMDB DB-size growth, projection cache eviction working under churn.
- **Cross-platform realness:** does the same captured trace produce the same `AppState` on iOS / Android / desktop / web?

A test that exercises all of this is mandatory for any honest "v1 is ready" claim.

---

## 2. The harness shape

Three modes, one binary:

```
crates/nmp-testing/bin/firehose-bench/
├── main.rs                       # mode dispatch via subcommand
├── live.rs                       # mode: live — real relays, real network
├── capture.rs                    # mode: capture — record live session to trace file
├── replay.rs                     # mode: replay — deterministic re-execution from trace
├── scenarios/
│   ├── cold_start.rs
│   ├── sustained_firehose.rs
│   ├── profile_thrashing.rs
│   ├── relay_disconnect_storm.rs
│   ├── multi_account.rs
│   ├── negentropy_efficiency.rs
│   ├── background_decryption.rs
│   └── soak.rs
├── traces/                       # checked-in captured traces for CI replay
└── reports/                      # JSON output + markdown summaries
```

### 2.1 Live mode

```
nmp-testing firehose-bench live --scenario sustained_firehose \
    --relays wss://relay.damus.io,wss://nos.lol,wss://relay.snort.social \
    --duration 30m \
    --storage lmdb --db-path /tmp/firehose-bench-db
```

Connects to real relays. Drives the actor through real subscriptions (typically scenario-defined, e.g. "follow these 500 pubkeys, open following timeline, scroll periodically"). Records all incoming WebSocket frames to a capture file if `--capture <path>` is also passed. Used for soak testing and for capturing new traces.

### 2.2 Capture mode

A specialization of live mode that does nothing but capture: connects, records, exits when a scenario's exit condition fires (duration elapsed, N events received, manual signal).

Capture format: a binary stream of `{relay_url, timestamp_ms, direction, frame_bytes}` records. Replayable losslessly.

### 2.3 Replay mode

```
nmp-testing firehose-bench replay --trace traces/2026-05-17-sustained-firehose.cap \
    --scenario sustained_firehose --speed 1.0
```

No network. The trace file is replayed against an in-process `MockRelay` (from `nostr-relay-builder`) that emits captured frames at captured timestamps. Same actor, same storage backend, same subscriptions. Deterministic and reproducible.

`--speed 2.0` plays back at 2× wall-clock — useful for soak testing in compressed time.

**Replay is what CI runs.** Capture is run periodically (weekly?) to refresh traces; live is for manual investigation.

---

## 3. Scenarios

### 3.1 Cold start with a heavy follow list

**Setup.** Empty LMDB, account following 1,000 real pubkeys. App boot.

**Measure.**

- Time from process start to first `ViewBatch` containing ≥ 1 timeline item.
- Time from process start to timeline view filled to its limit (200 items).
- Bytes received from relays during ramp-up.
- DB writes during ramp-up.
- Memory at ramp-up peak.

**Gates.**

- First-item p99 ≤ 800 ms (over a good connection).
- Filled-timeline p99 ≤ 5 s.
- Peak memory ≤ 200 MB during ramp-up.

### 3.2 Sustained timeline + hashtag firehose (30 minutes)

**Setup.** Primed LMDB (from a prior cold-start run). Account following 1,000. Open: following timeline + `#nostr` hashtag view + DM inbox.

**Measure.**

- Events ingested per second (steady state, p50/p99).
- Per-event ingest-to-emit p99 (relay frame received → `ViewBatch` emitted with the corresponding delta).
- LMDB write throughput, fsync cost.
- DB size growth rate.
- ViewBatch FFI emission rate.
- Working-set memory drift (start → 5min → 15min → 30min).
- Reverse-index size growth.
- Projection cache hit/miss rate.
- Subscription planner REQ count on the wire (should be far less than the open view count).

**Gates.**

- Memory drift over 30 min ≤ 50 MB (excluding LMDB-resident pages; measured by counting allocator).
- Dropped events: 0.
- Ingest-to-emit p99 ≤ 50 ms.
- ViewBatch frequency steady-state ≤ 60 Hz.
- Per-view delta rate ≤ 60/sec/view (matches ADR-0002).
- Outbox routing: relay connections = `|union of write-relays-of-1000-authors|` (typically 30–80), no more.

### 3.3 Profile thrashing

**Setup.** Replay-mode against a captured trace of timeline activity. Simulate UI behavior: components mount `useProfile(pubkey)` per cell as the timeline scrolls, unmount as cells exit viewport. Mount/unmount rate: ~50/sec for 10 minutes.

**Measure.**

- Wrapper refcount transitions per second.
- `OpenView` / `CloseView` dispatch rate.
- Projection cache LRU evictions per second.
- Memory in the platform shadow (simulated) over time.
- Net FFI dispatch rate.

**Gates.**

- No memory growth across the 10-minute window — eviction keeps platform shadow bounded.
- `OpenView`/`CloseView` dispatch rate ≤ 60% of mount/unmount rate (dedup + grace-period absorption).
- Projection cache hit rate ≥ 50% (same author seen in multiple cells).
- Zero subscription leaks — every `OpenView` has a matching `CloseView` after the grace period.

### 3.4 Relay disconnect storm

**Setup.** Live mode, 10 relays connected. Periodically kill 30% of connections (random selection, exponential inter-arrival times, mean 30s). Continue subscriptions and measurements.

**Measure.**

- Time to detect disconnection.
- Time to reconnect (per relay).
- Sync watermark resume behavior — does the gap fill via negentropy?
- Event loss across the disconnect window.
- Outbox-routing degradation when an author's primary relay is unreachable.

**Gates.**

- Detect-disconnect p99 ≤ 10 s.
- Reconnect p99 ≤ 30 s (assuming relay actually comes back).
- Zero event loss for events the relay still has when reconnected.
- Subscriptions automatically resubscribed on reconnect.

### 3.5 Multi-account concurrent

**Setup.** 5 accounts (test keys generated for the harness). Each account follows 100 distinct pubkeys with ~20% overlap across accounts. All 5 sessions active; each opens a following timeline + profile view. Periodic sends from each account.

**Measure.**

- Per-account state isolation (verify by snapshot diffing).
- Signing operation interleaving correctness (no leaked nsecs across sessions).
- Action atomicity under concurrent sends (publish + insert + relay-OK as one unit).
- FFI dispatch rate.
- Memory per account.

**Gates.**

- Zero cross-account state bleed (`AppState` snapshot for account A never contains account B's data).
- Action atomicity holds under interleaving — no half-committed actions.
- Per-account memory ≤ 100 MB.

### 3.6 Negentropy efficiency

**Setup.** Two configurations against the same NIP-77-capable relay (e.g. relay.damus.io if it supports NIP-77; otherwise spin up `LocalRelay` from `nostr-relay-builder` with negentropy enabled).

- **Config A:** v1 framework with sync engine enabled.
- **Config B:** v1 framework with sync engine disabled (REQ-only fallback).

Backfill 30 days of timeline (~10k events for a chatty user) on each.

**Measure.**

- Bytes-on-wire (Config A vs Config B).
- Wall-clock time to backfill.
- Number of round trips.
- DB write rate.

**Gates.**

- Config A bytes-on-wire ≤ 5% of Config B bytes-on-wire (the 95% bytes-saved target from spec §7.16).
- Config A wall-clock comparable to or faster than Config B.

### 3.7 Background decryption load

**Setup.** Simulate iOS NSE behavior: spawn `nmp-nse`'s `decrypt_push()` 100 times in quick succession (one per simulated push notification), each with a different encrypted DM. Then "foreground" the app and verify the conversation views populate without re-fetching.

**Measure.**

- Per-decrypt p99 latency.
- Peak memory during the burst.
- DB write conflicts between NSE writes and foreground actor reads.
- Conversation view state after foreground — does it match a known-good state derived from a pure-foreground run?

**Gates.**

- Per-decrypt p99 ≤ 200 ms.
- Peak memory ≤ 24 MB (iOS NSE budget).
- Zero DB conflicts.
- Conversation view byte-identical to pure-foreground reference.

### 3.8 Soak (24-hour live)

**Setup.** Live mode, 10 relays, account following 500 pubkeys, scenario alternating between: 5min following timeline, 2min hashtag firehose, 1min DM inbox, 30s profile views, 1min idle.

**Measure.**

- Same metrics as sustained firehose (§3.2) over 24 hours.
- DB size growth curve.
- File descriptor count over time.
- Reconnection events (relays will flap naturally).
- Memory at start, 1h, 6h, 12h, 24h.

**Gates.**

- Memory growth over 24h ≤ 100 MB.
- DB size growth within event-rate × avg-size × overhead (no runaway).
- No file descriptor leaks.
- Zero panics.
- ViewBatch frequency stays ≤ 60 Hz across the full window.

---

## 4. What this harness measures that reactivity-bench does not

| Concern | reactivity-bench | firehose-bench |
|---|---|---|
| Reverse-index lookup latency | ✓ | ✓ (under real distribution) |
| View recompute latency | ✓ | ✓ |
| Delta volume / coalescing | ✓ (synthetic streams) | ✓ (real bursty traffic) |
| LMDB / SQLite write throughput | — | ✓ |
| LMDB compaction overhead | — | ✓ |
| Cold-start time | — | ✓ |
| Real relay RTT effects | — | ✓ |
| Multi-relay deduplication | — | ✓ |
| Signature verification cost | — | ✓ (at scale) |
| Outbox routing fan-out | — | ✓ |
| Reconnect / watermark resume | — | ✓ |
| Negentropy bytes-saved | — | ✓ |
| FFI marshaling under realistic patterns | — | ✓ |
| Multi-account concurrency | — | ✓ |
| Memory drift over hours | — | ✓ |
| NSE decryption budget | — | ✓ |

The two are complementary. Reactivity-bench answers "does the algorithm scale?"; firehose-bench answers "does the system work under realistic conditions?"

---

## 5. CI integration

- **Per-commit** (CI): replay-mode against the checked-in traces. Fails the build if any gate regresses. Fast — uses MockRelay, no network.
- **Nightly**: full soak run in replay mode at 24×, plus a 1-hour live run against the public relay set (best-effort; ignored on relay flakes if the deterministic replay passed).
- **Pre-release**: full 24h live soak on each reference device class (mid-range mobile, desktop, web). Manual sign-off on the report.

Trace files live in `crates/nmp-testing/bin/firehose-bench/traces/`. They are LFS-tracked (binary, multi-MB). A trace is invalidated when:

- The serialized event format changes (rare).
- The framework's expected `AppState` for that trace changes due to a code change (test maintenance).

Traces are re-captured quarterly against current relay traffic to keep them representative.

---

## 6. Implementation phasing

The harness is a Phase 9 deliverable per `plan.md`, but pieces ship earlier:

| Capability | Lands in phase | Why |
|---|---|---|
| `live` mode minimal (open a sub, dump events) | Phase 1 | Validates the planner against real relays |
| `capture` + `replay` infrastructure | Phase 2 | Validates the sync engine against captured traffic |
| `cold_start` scenario | Phase 2 | Validates negentropy-first backfill |
| `sustained_firehose` scenario | Phase 4 | Validates view-end-to-end under realistic load |
| `profile_thrashing` scenario | Phase 4 | Validates ADR-0005 wrapper lifecycle |
| `relay_disconnect_storm` | Phase 2 | Validates reconnect + watermark resume |
| `multi_account` | Phase 3 | Validates session model |
| `negentropy_efficiency` | Phase 2 | Validates sync engine |
| `background_decryption` | Phase 5 | Validates NSE crate |
| `soak` | Phase 9 | Final-pass long-running |

By Phase 9 the full scenario set runs on real devices for the final perf sign-off.

---

## 7. What's not in scope (yet)

- **GUI rendering benchmarks.** The harness drives the framework headlessly. UI render times on iPhone are measured separately during Phase 9's proof-app pass with Xcode Instruments / Android profiler / Chrome DevTools.
- **Cross-platform consistency tests via this harness.** Trace replay produces deterministic `AppState` snapshots; whether iOS/Android/desktop/web produce identical snapshots is verified by the per-platform consistency tests in `nmp-testing/scenarios/` (separate, also Phase 9).
- **Adversarial workloads.** Malicious events (oversized content, kind:5 storms, sig-validation bombs). Worth adding later; not v1.
- **Network simulation.** Latency / packet-loss injection between the harness and a real or mock relay. Could use `tc netem` externally; not built into the harness yet.

---

## 8. Next step

After Phase 1 lands (event store + planner + reactivity machinery), build `live` and `capture` modes minimally, capture the first trace, and run `cold_start` + `relay_disconnect_storm` scenarios. Use the results to validate ADR-0001/0002/0003 against real network patterns (the synthetic harness can't show, e.g., what happens when 5 relays send the same event within 50ms of each other).

The findings get a write-up in `docs/perf/firehose-bench/<date>-run-NNN.md` and trigger ADRs for any design revisions, exactly the same pattern as reactivity-bench run 001.
