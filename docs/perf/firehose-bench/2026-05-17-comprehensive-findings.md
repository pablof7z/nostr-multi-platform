# Nostr Multi-Platform Build, Run, And Test Findings

Date: 2026-05-17

## Executive Summary

The workspace builds, formats, and passes the Rust test suite. The reactivity benchmark passes the current standard gates. The new firehose benchmark harness builds and runs in replay and capture modes, and those modeled scenarios pass the proposed gates.

This is not yet proof that the product runtime is fast enough under real relay traffic. The current firehose harness is a deterministic prototype that models relay sockets, durable storage, UniFFI marshaling, generated platform wrappers, and notification-service-extension behavior. Its value today is that it turns the proposed architecture into executable budgets and exposes exactly where the next real implementation must plug in.

The live path correctly reports blocked rather than pretending to test a runtime that does not exist yet. Live firehose validation is blocked on the real actor, relay adapter, storage backend, and FFI/platform wrapper integration.

## What Was Built

Added a `firehose-bench` binary under `crates/nmp-testing`.

The harness supports three modes:

- `replay`: deterministic CI-shaped modeled workload.
- `capture`: deterministic modeled workload that also writes a synthetic trace manifest.
- `live`: currently reports blocked because the real runtime adapters do not exist yet.

The harness currently covers eight scenario families:

- Cold start.
- Sustained firehose.
- Profile/avatar subscription thrashing.
- Relay disconnect storm.
- Multi-account isolation.
- NIP-77/negentropy efficiency.
- Background decryption.
- 24-hour soak model.

Generated artifacts:

- `docs/perf/firehose-bench/1779046827-replay.md`
- `docs/perf/firehose-bench/1779046827-replay.json`
- `docs/perf/firehose-bench/1779046847-capture.md`
- `docs/perf/firehose-bench/1779046847-capture.json`
- `docs/perf/firehose-bench/1779046850-live.md`
- `docs/perf/firehose-bench/1779046850-live.json`
- `docs/perf/firehose-bench/traces/1779046847-synthetic.json`
- `docs/perf/reactivity-bench/1779046753-run-002.md`
- `docs/perf/reactivity-bench/1779046753-run-002.json`

## Verification Commands

All commands below were run from the repository root.

| Command | Result | Notes |
|---|---|---|
| `cargo fmt --all` | Passed | Formatting was applied before benchmark execution. |
| `cargo test --workspace` | Passed | 3 `nmp_core` tests passed; benchmark crates compile. |
| `cargo run -p nmp-testing --bin reactivity-bench --release -- --standard --fail-on-gate` | Passed | Standard reactivity gates passed. |
| `cargo run -p nmp-testing --bin firehose-bench --release -- replay --standard --fail-on-gate` | Passed | Prototype replay gates passed. |
| `cargo run -p nmp-testing --bin firehose-bench --release -- capture --quick --fail-on-gate` | Passed | Prototype capture gates passed and wrote a synthetic trace manifest. |
| `cargo run -p nmp-testing --bin firehose-bench --release -- live --quick` | Completed with `overall_passed=false` | Expected limitation: live mode is blocked on real runtime adapters. |
| `git diff --check` | Passed | No whitespace/conflict-marker issues in the current diff. |

## Reactivity Benchmark Findings

Latest standard run: `docs/perf/reactivity-bench/1779046753-run-002.md`.

Overall result: passed.

Key results:

- `quiet_idle`: 0 false wakeups, 1.00 candidates per delta, 125 ns lookup p99.
- `following_timeline_scroll`: 4,963 raw deltas coalesced to 4,070; max 40.70 deltas/view/sec.
- `hashtag_firehose`: 20,000 raw deltas coalesced to 589; max 58.90 deltas/view/sec.
- `profile_fanout`: 168,978 raw deltas coalesced to 115,308; max 46.38 deltas/view/sec.
- `thread_blowup`: 10,500 raw deltas coalesced to 1,224; max 55.64 deltas/view/sec.
- `working_set_100_views`: 1,000,000 cached events, 10,000 hot events, 100 open views, about 19.79 MB modeled hot working-set memory.
- Steady-state allocation measurement is 0 for the main post-warmup insert, lookup, recompute, and delta-buffer paths in the measured scenarios.

Interpretation:

The reactivity model is directionally strong. Composite reverse indexes, per-view delta gates, batching, coalescing, and a hot/cold working-set split are the right defaults. The benchmark specifically supports the Applesauce/NDK-derived lesson that the UI should express interest declaratively while the core dedupes work and emits bounded deltas.

Important caveat:

The memory figures are modeled as actor hot working set plus indexes and open views. Cold event bodies are treated as disk-resident. The allocation measurement uses a process-wide counting allocator and only samples the post-warmup hot path.

## Firehose Replay Findings

Latest standard replay report: `docs/perf/firehose-bench/1779046827-replay.md`.

Overall result: passed.

Scenario summary:

| Scenario | Workload | Result | Key Observation |
|---|---:|---|---|
| Cold start | 10,000 events | Passed | First item modeled at 60 ms; filled timeline modeled at 740 ms. |
| Sustained firehose | 900,000 events over 30 minutes | Passed | 25.60 ms ingest-to-emit p99, 58 Hz batching, 57 deltas/view/sec. |
| Profile thrashing | 30,000 events | Passed | Domain-keyed wrapper dedupe keeps open/close dispatch at 22.5/sec. |
| Relay disconnect storm | 64 events over 30 minutes | Passed | Disconnect/reconnect numbers are policy targets, not socket measurements. |
| Multi-account | 10,000 events | Passed | Reinforces that account scope should exist in the app-kernel API from v1. |
| Negentropy efficiency | 10,000 events | Passed | NIP-77 byte ratio modeled at 3.6 percent of REQ-only bytes. |
| Background decryption | 100 events | Passed | Fake decrypt p99 modeled at about 3.02 ms. |
| Soak | 2,160,000 events over 24 virtual hours | Passed | Memory growth modeled at 38 MB; no fd growth or panics modeled. |

Interpretation:

The proposed budgets are internally consistent if the runtime actually implements ADR-0002 style batching/coalescing and ADR-0005 domain-keyed platform shadows. The sustained firehose scenario only passes because the model assumes per-view coalescing and bounded delta delivery. Without that, the FFI/platform cache boundary would be too chatty.

Important caveat:

Replay mode validates the shape of the budget and the harness, not real transport, storage, or platform behavior.

## Firehose Capture Findings

Latest quick capture report: `docs/perf/firehose-bench/1779046847-capture.md`.

Overall result: passed.

The capture run wrote `docs/perf/firehose-bench/traces/1779046847-synthetic.json`.

Manifest summary:

- Format: `synthetic-firehose-trace-manifest-v1`.
- Scenario count: 8.
- Total records: 251,028.
- Note: replace with frame-level relay capture once relay adapters exist.

Interpretation:

Capture mode is useful as a placeholder contract for future trace ingestion. It should not be treated as live relay capture yet.

## Firehose Live Findings

Latest live report: `docs/perf/firehose-bench/1779046850-live.md`.

Overall result: false.

This is expected and correct. Live mode currently has no scenarios because the core pieces it needs do not exist yet:

- Real relay adapter.
- Actor integration.
- Durable storage backend.
- FFI bridge.
- Generated platform wrappers or platform shadow cache.

Interpretation:

The next meaningful firehose milestone is not to add more modeled numbers. It is to replace the modeled relay/storage/FFI segments with real adapters one by one and keep the same gates.

## Architecture Findings

We should not build a new Nostr protocol stack from scratch. The architecture should use existing Rust Nostr protocol/client primitives where they are reliable, then build our own app-kernel layer above them.

The thing we are building is closer to a multi-platform Nostr application runtime than a replacement for `nostr-rs`, NDK, or Applesauce. The core responsibilities are:

- Own durable event storage and query indexes.
- Track app interests as explicit view/domain subscriptions.
- Deduplicate frontend interests into efficient relay subscriptions.
- Materialize bounded deltas from Rust into platform shadow caches.
- Preserve platform-native rendering and platform-native cache reads.
- Keep signer/account scope explicit from the beginning.
- Provide testable performance budgets for reactivity, FFI churn, storage, relay behavior, and background extensions.

The frontend-side shadow cache idea still makes sense. The pattern should be:

```text
relay <-> rust actor <-> durable store / hot working set
                      |
                      +-> FFI deltas -> platform shadow cache -> UI components
```

The platform cache should not be the source of truth and should not be durable by default. Its purpose is to avoid crossing FFI for every render and to let components synchronously render from local platform memory while Rust keeps that memory warm for active interests.

ADR-0005's domain-keyed wrapper model is important here. A rendered avatar should not create a unique Rust subscription solely because a component instance appeared. It should express interest in a domain key such as profile metadata for pubkey X. The Rust side can then dedupe multiple component instances, decide whether relay work is necessary, serve cached state immediately, and close relay interest only after lifecycle/grace-period policy says the domain is no longer needed.

## Applesauce And NDK Lessons Reflected In The Results

The benchmark results reinforce these lessons:

- Query/reactivity must be store-centered, not relay-subscription-centered.
- UI interest should be declarative and disposable.
- The core must dedupe equivalent interests across components and screens.
- Replaceable events such as kind 0 need first-class latest-value projection.
- Coalescing is not an optimization later; it is part of the architecture.
- Frontend caches are useful, but only as platform shadows maintained by core deltas.
- Subscription lifetime should include grace periods and hysteresis to avoid scroll-induced relay churn.
- The bridge should move domain deltas, not raw relay event streams.

## Main Risks Still Unproven

- Real durable-store latency under write bursts.
- Real relay burstiness, duplicates, EOSE behavior, disconnects, and backfill gaps.
- UniFFI serialization and memory cost for high-frequency deltas.
- Generated Swift/Kotlin/TypeScript wrapper memory behavior.
- Platform shadow invalidation correctness.
- Real NIP-77 availability and fallback behavior.
- Real NIP-44/NIP-59/NSE decryption time and memory on device.
- Multi-account signer isolation and accidental cross-account event bleed.
- Long-running file descriptor and subscription cleanup under app backgrounding.

## Recommendations

1. Keep the current architecture direction: Rust app-kernel plus platform shadow caches, not a new Nostr protocol implementation.
2. Promote `reactivity-bench --standard --fail-on-gate` into the regular pre-merge check once these crates are tracked.
3. Add focused unit tests around the composite reverse index, coalescer, and domain-keyed wrapper lifecycle.
4. Implement the real `OpenView`/`CloseView` or equivalent interest API next, including account scope and domain keys from the start.
5. Generate the first platform wrapper/shadow-cache path for profile metadata because avatar/name rendering is the simplest real proof point.
6. Replace `firehose-bench` modeled storage with the real storage adapter before trusting memory or latency numbers.
7. Replace synthetic capture with frame-level relay capture as soon as a relay adapter exists.
8. Keep live mode failing until it is actually connected to relay, storage, actor, and FFI runtime pieces.
9. Treat NIP-77 and NSE numbers as planning gates only until tested against real relays and real device extensions.

## Bottom Line

The reactivity plan is coherent and currently passes the executable model. The firehose plan is now executable as a benchmark scaffold and budget contract, but not yet as real production evidence.

The next milestone should be a narrow vertical slice: kind 0 profile metadata interest from platform component, through generated wrapper and FFI, into Rust actor/store, back as platform cache deltas, with relay work deduped by domain key. That slice would directly validate the avatar/name example and turn the current architecture from a model into a working runtime path.
