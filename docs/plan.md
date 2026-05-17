# Build & Validation Plan

> Companion to `docs/product-spec.md`. The spec defines **what we ship**; this plan defines **how we get there and how we know it works**.

> **Two arcs:** Build the infrastructure → build a stress-proof app on top → measure on real devices → tune → release.

> **The plan is gated.** Each phase has an automated exit gate plus a manual sign-off. Subsequent phases must not regress prior gates. The proof app (Arc 2) is the load-bearing artifact — it is how we know the framework actually works at scale, not just in isolation.

---

## 0. Principles of execution

1. **Infrastructure before features.** Get the actor model, FFI marshaling, planner, event store, and sync engine right and verified before layering wallet, WoT, messaging, etc. on top.

2. **A real app validates the framework.** Unit tests prove pieces work; the proof app proves they work together at scale. The proof app is not optional — it is the v1 release gate.

3. **Measure on the device.** Synthetic benchmarks lie. Performance budgets (§7.16 of the spec) are validated against the proof app running on real mid-range phones, real desktops, real browsers — not on a developer's M-series laptop.

4. **No phase ends silently.** Each phase ends with: regression tests added to `nmp-testing`, a brief write-up in `docs/perf/phaseN.md` if relevant, and an explicit gate sign-off.

5. **The doctrine list (D1–D5) is the rubric.** Every PR is reviewed against the cardinal doctrines. If a change makes any doctrine harder to enforce, it gets rewritten or rejected.

---

## Arc 1 — Infrastructure (Phases 0–7)

### Phase 0 — Foundations

**Scope.**

- Cargo workspace with the crate roster from spec §4.1.
- `nmp-core` actor skeleton: single OS thread + flume channel + tokio runtime for async I/O.
- Empty `AppState`, empty `AppAction`, empty `AppUpdate` types with `rev: u64`.
- `nmp-ffi` with `FfiApp` `uniffi::Object`, `AppReconciler` callback interface, generated Swift + Kotlin bindings checked in.
- `nmp-wasm` skeleton with wasm-bindgen wrapper.
- `nmp-testing` skeleton with `MockRelay` re-export and snapshot helpers.
- `justfile` recipes: `rust-build-host`, `gen-bindings`, `run-ios`, `run-android`, `run-desktop`, `run-web`, `test`.
- CI lane on GitHub Actions: build all four targets, run unit tests, regenerate bindings and diff.
- Nix flake.

**Out of scope.** No event handling, no relay code, no views, no actions.

**Exit gate.**

- A no-op `FullState(AppState::default())` snapshot round-trips through Swift, Kotlin, TypeScript without panic.
- `rev` ordering test: emit 10 updates rapidly; receiver applies in order, drops stales.
- `cargo test --workspace` passes on Linux + macOS.
- `just gen-bindings` produces deterministic output (committed bindings match regenerated bindings).
- Cold build of all four platforms ≤ 5 minutes on CI.

**Regression test added.** `tests/ffi_round_trip.rs`: rev ordering + bindings drift check.

### Phase 1 — Event store + planner

Per ADR-0006, Phase 1 opens with a **vertical slice** before the broader scope lands. The slice proves the architecture end-to-end against a real relay with running code, not modeled budgets.

**1a. Vertical slice (the walking skeleton).** Per ADR-0006 §"The vertical slice":

- Desktop iced shell with an `Avatar { pubkey }` component using `useProfile(pubkey)`.
- Manually-written wrapper implementing the ADR-0005 refcounted domain-keyed pattern.
- Minimal nmp-core actor handling `OpenView`/`CloseView` for the Profile view kind only.
- In-memory EventStore with kind:0 replaceable supersession + composite reverse index keyed by `(kind, author)`.
- One WebSocket connection via `nostr-sdk` to one hardcoded relay; REQ/CLOSE driven by view lifecycle.
- No LMDB, no FFI (desktop links rlib directly), no outbox, no negentropy, no other view kinds — those layer in after.

Exit gate for the slice: per ADR-0006 §"Exit gate for the slice" — manual demo + slice-scoped firehose-bench `live` runs.

**1b. Broader Phase 1 scope, layered on top of the slice.**

- LMDB and IndexedDB backends; swap from in-memory via `Box<dyn EventStore>`.
- Full insert invariants from spec §7.1 (parameterized replaceable, kind-5 delete, NIP-40 expiration, dedup with provenance merge).
- Claim-based GC.
- Sync watermarks table (read/write API; populated by Phase 2 sync engine).
- `nmp-gossip` outbox routing for both reads and writes per spec §7.3.
- Subscription planner with coalescing, auto-close, EOSE detection, ≤60Hz buffering, reconnect re-establishment.
- Live REQ tail working end-to-end against `MockRelay`.
- Reverse index + projections architecture from `docs/design/reactivity.md` (§3–§6) — the slice already includes composite keys; broader Phase 1 fills in the projection caches.
- **`reactivity-bench` stress harness** — already built (run 002 validated the model). Promoted to pre-merge CI per recommendations.
- **`firehose-bench` capture + replay infrastructure** — already built; live mode unblocks scenario by scenario as adapters land.

**Prerequisite design docs.**

- `docs/design/reactivity.md` — reviewed and accepted (rev 1, post run 001).
- `docs/design/view-catalog.md` — reviewed for Phase 1 view kinds.
- `docs/design/firehose-bench.md` — reviewed; runtime adapters track against §6 phasing.

**Exit gate (full Phase 1, not just the slice).**

- All bug-extinction tests pass (per `product-spec.md` §3.3).
- Replaceable event correctness verified.
- Provenance correctness verified.
- Reactivity harness gates (per `reactivity.md` §10.3 rev 1):
  - Lookup p99 ≤ 100 µs (run 002: passed).
  - Per-view recompute p99 ≤ 1 ms (run 002: passed).
  - ≤ 60 deltas/sec/view (run 002: passed across all scenarios).
  - False-wakeup rate ≤ 0.10 (run 002: 0 in quiet_idle, 1.00 candidates/delta).
  - Working-set memory ≤ 100 MB at 100 active views (run 002: ~20 MB modeled).
  - Zero per-event allocations after warmup (run 002: passed via counting allocator).
- **Firehose-bench live mode unblocked for cold_start + profile_thrashing** against a real relay, measured numbers (not modeled) within budgets.
- Unit-test coverage on composite reverse index, coalescer, and domain-keyed wrapper lifecycle (per Phase 1 recommendation from firehose-bench run 001).

**Regression tests added.** `tests/event_store_invariants.rs`, `tests/planner_coalesce.rs`, `tests/outbox_routing.rs`, `tests/reverse_index.rs`, `tests/coalescer.rs`, `tests/wrapper_lifecycle.rs`, `bin/reactivity-bench/scenarios/*`, `bin/firehose-bench/scenarios/cold_start.rs`, `bin/firehose-bench/scenarios/profile_thrashing.rs`.

### Phase 2 — Sync engine (negentropy first-class)

**Scope.** Per spec §7.8.

- NIP-77 negentropy reconciliation implementation (or integration with `nostr-sdk`'s if available).
- Watermark read/write — the table from Phase 1 now actively populated.
- Planner consults watermarks before issuing REQ for historical data.
- Three built-in triggers: app foreground, view open with gap, relay reconnect.
- `RunSync` manual action.
- Per-relay capability negotiation (probe for NIP-77 support; cache result).
- Bytes-on-wire vs equivalent-REQ-bytes measurement plumbed into `nmp-metrics`.
- `SyncState` field of `AppState` populated and visible.

**Exit gate.**

- Cold open of a profile against a NIP-77-supporting relay completes sync via negentropy, not REQ. Bytes saved ≥ 95% vs equivalent REQ on a 10k-event backfill.
- Relay reconnect after 10 minutes resumes from the watermark; the gap is filled by sync, not by a fresh REQ scan.
- Mixed-capability relay set: of N relays, those that support NIP-77 use sync; those that don't fall back to REQ; both populate the same store correctly.
- Negentropy bytes-saved counter visible in `AppState.debug` in debug builds.
- Cache-miss-against-fully-synced-relay answers authoritatively (no fallback fetch).

**Regression tests added.** `tests/sync_engine.rs`, `tests/sync_fallback.rs`, `tests/watermarks.rs`.

### Phase 3 — Sessions + signers + actions

**Scope.** Per spec §7.4, §7.5, §7.15.

- `SessionState` and the multi-account model.
- Signer trait with two initial implementations: local key (raw nsec) and NIP-46 bunker.
- NIP-49 encrypted private key support.
- Full action catalog from spec §6.3.
- Action atomicity guarantee: publish + store-insert happen as one actor message.
- Offline action queue with durable persistence; replay on reconnect.
- Capability bridge for `KeyringCapability` defined (native shims come in Phase 4 platform shells).

**Exit gate.**

- Bug-extinction tests pass:
  - #5 (account-context overlap): two accounts active, switch between them, assert no state bleed.
  - #7 (action partial-success): inject "publish succeeds, store-insert fails" and "store-insert succeeds, publish fails" — both atomically rolled back.
  - #9 (NIP-46 lost on suspend/resume): simulate suspend mid-action; assert resume restores pending state and either retries or surfaces failure as toast.
  - #10 (re-publish keeps event id): re-publish of an event preserves its original `id` and `sig`.
- All actions in spec §6.3 dispatched and verified against `MockRelay`.
- Offline queue: 100 actions dispatched while offline, all replay correctly on reconnect in scheduled order.

**Regression tests added.** `tests/actions_catalog.rs`, `tests/atomicity.rs`, `tests/offline_queue.rs`, `tests/multi_account.rs`.

### Phase 4 — Views end-to-end through FFI

**Scope.** Phase 1 built the reactive machinery and Phase-1 view kinds Rust-side. Phase 4 completes the loop through FFI to platforms, fills in the remaining view kinds, and runs the view-catalog scenarios end-to-end.

- All 15 view kinds from `docs/design/view-catalog.md` §2 (the Phase 5/6-deferred ones still get stubs that compile).
- `dispatch(OpenView)` / `dispatch(CloseView)` / `dispatch(RefreshView)` / `dispatch(AdvanceCursor)` action handling.
- `ViewBatch` emission across FFI; per-view-kind `ViewDelta` variants serialized via UniFFI.
- View warmth (30s cache after last claim drop).
- Platform shims (generated by Phase 10's CLI, manually written for now) for iOS + Android + desktop + web: refcounted domain-keyed wrappers per ADR-0005 (`useProfile`, `@Profile`, `rememberProfile`); reconciler routes `ViewBatch` deltas into typed domain-keyed dictionaries; wrappers translate component mount/unmount into `OpenView`/`CloseView` with 30s eviction grace period.
- The five view-catalog scenarios from `docs/design/view-catalog.md` §11 run against the harness with the Phase 4 implementation.

**Prerequisite design docs.**

- `docs/design/view-catalog.md` — the per-view-kind spec. The five fully-detailed kinds (Profile, Timeline, Thread, Reactions, Conversation) are the template; stubs get filled in here.

**Exit gate.**

- Best-effort doctrine enforced: timeline view renders posts whose authors have no kind:0 yet; placeholders are shown; when kind:0 arrives, in-place update.
- Cached-data-never-withheld: any kind:0 in store is served immediately by profile view; background refresh does not gate.
- LLM-friendliness test (§3.4 of spec): a developer or LLM given only docs implements a new "hashtag screen" view kind in ≤ 1 hour, with no edits to `nmp-core`, that passes outbox routing + GC + lifecycle correctness on first compile.
- `ViewBatch` reduces per-event marshaling vs equivalent `FullState`: under hashtag firehose, `ViewBatch`/sec ≤ 60 and `FullState`/sec ≤ 0.1.
- All five view-catalog scenarios from `view-catalog.md` §11 pass:
  - Profile fan-out p99 ≤ 5ms end-to-end.
  - Hashtag firehose stays ≤ 60Hz / ≤ 1000 deltas/sec.
  - Thread orphan storm builds correctly in ≤ 50ms.
  - Reactions aggregation coalesces to ≤ 60 deltas/sec.
  - Conversation paging interleaves without actor starvation.

**Regression tests added.** `tests/views.rs`, `tests/best_effort_rendering.rs`, `tests/view_warmth.rs`, `tests/view_catalog/*` (one per kind), `bin/reactivity-bench/scenarios/view_catalog_*`.

### Phase 5 — Messaging (NIP-17 + NSE)

**Scope.** Per spec §7.10, §7.14.

- NIP-17 conversation layer over NIP-44 + NIP-59.
- 1:1 and group DMs.
- Conversation list + conversation views (using Phase 4 view machinery).
- Action atomicity for `SendDm`: gift-wrap, publish to each recipient's inbox relays, insert locally — atomic.
- `nmp-nse` crate: `decrypt_push()` with bounded memory; reads from shared keychain + shared storage; no actor.
- iOS NSE shim demonstrating it.
- Android `FirebaseMessagingService.onMessageReceived` shim demonstrating it.

**Exit gate.**

- DM round-trip in `MockRelay`: alice sends, bob receives, content matches, no plaintext crossing FFI other than as conversation view payload field.
- NSE crate decrypts a push event in ≤ 200 ms with ≤ 24 MB peak memory.
- Bug-extinction test #4 (DM to public): cannot send a DM to a non-inbox relay through any public API path.
- Background-decryption test: app backgrounded, push arrives, NSE decrypts, notification posted, app foregrounded — conversation view shows the message without re-fetching.

**Regression tests added.** `tests/messaging.rs`, `tests/nse_memory.rs`, `tests/dm_routing.rs`.

### Phase 6 — Wallet + WoT + Blossom

**Scope.** Per spec §7.9, §7.7, §7.11.

- NWC client; pay/receive lightning.
- LUD-16 zaps; zap receipt verification automatic.
- Cashu (NIP-60) + nutzaps (NIP-61).
- Web-of-trust subsystem with default scoring (in-degree depth-weighted); pluggable trait.
- Blossom client (BUD-01/02); upload + download; reactive `MediaState`.

**Exit gate.**

- Pay a zap end-to-end against a mock LN node; receipt verifies; balance updates.
- WoT toggle visibly reorders timeline based on score; off-toggle restores chronological order.
- Blossom upload progresses through `MediaState`; cancellation works.

**Regression tests added.** `tests/wallet.rs`, `tests/wot.rs`, `tests/blossom.rs`.

### Phase 7 — Web target

**Scope.** Per spec §6 (web), §10 (open questions resolved here).

- `nmp-wasm` mature: full `FfiApp` equivalent over wasm-bindgen.
- IndexedDB storage backend; OPFS for browsers that support it.
- NIP-07 capability bridge for web signing.
- Web shell with TypeScript types and a reactive store.

**Exit gate.**

- Cross-platform consistency tests (§3.5 of spec) pass on web: same action sequence produces byte-identical `AppState` JSON as on iOS/Android/desktop.
- Web cold-start to first painted timeline ≤ 2s on a modern browser.
- Web works in incognito (no persistent storage) by falling back to in-memory store with a visible warning.

**Regression tests added.** `tests/web_consistency.rs`, `tests/web_storage_fallback.rs`.

---

## Arc 2 — Proof app + performance pass (Phases 8–9)

### Phase 8 — Build the proof app

**Scope.** Per spec §4.5.

Build `nmp-proof` on all four platforms. Feature set in the spec; the goal here is **wiring**, not new framework features. If a feature is hard to wire, that's a framework defect to be fixed back in Arc 1.

- iOS: SwiftUI app with all proof-app screens.
- Android: Compose app with all proof-app screens.
- Desktop: iced app with all proof-app screens.
- Web: TS/React or Solid shell with all proof-app screens.
- Performance overlay implemented per-platform reading from `AppState.debug`.
- Scripted scenario harness in `nmp-testing` driving the proof app through canonical flows.

**Exit gate.**

- Proof app launches on all four platforms and successfully exercises every framework subsystem.
- The cross-platform consistency test script runs against the proof app on all four platforms; `AppState` JSON snapshots match byte-for-byte at each checkpoint.
- The performance overlay renders all counters from spec §7.16 live.
- Total proof-app platform code stays within the budgets from spec §3.2.

**Regression test added.** `tests/proof_app_consistency.rs` — the canonical scenario script.

### Phase 9 — Performance pass (firehose-bench + device measurements)

**Scope.** Take measurements on real hardware end-to-end. Fix budget regressions. Tune.

The `firehose-bench` harness (per `docs/design/firehose-bench.md`) is the load-bearing tool here. It runs in three modes: **live** (real relays, real network), **capture** (records live to a trace), **replay** (deterministic re-execution against `MockRelay`). Replay is what CI uses; live + capture are for soak testing and trace refresh.

Eight scenarios target distinct concerns (`firehose-bench.md` §3): cold_start, sustained_firehose, profile_thrashing, relay_disconnect_storm, multi_account, negentropy_efficiency, background_decryption, soak (24h live).

The harness ships pieces earlier (per `firehose-bench.md` §6): `live` + `capture` infrastructure in Phase 1; cold_start + relay_disconnect_storm + negentropy_efficiency scenarios in Phase 2 (gating the sync engine); sustained_firehose + profile_thrashing in Phase 4 (gating views end-to-end + ADR-0005 wrappers); multi_account in Phase 3; background_decryption in Phase 5; full soak in Phase 9.

**Reference devices:**

- **iOS:** iPhone 12 (mid-range, ~5 years old at v1 ship).
- **Android:** Pixel 6a or equivalent.
- **Desktop:** Linux laptop with integrated graphics; macOS M1.
- **Web:** Firefox + Chrome + Safari on the above desktop.

**Measurements** (collected by `nmp-metrics`, dumped via `EmitDiagnosticSnapshot`):

- All counters from spec §7.16 under three workloads:
  - **Idle** — app open, nothing happening.
  - **Following timeline scroll** — user with 1k follows, scrolling at typical mobile flick speed.
  - **Hashtag firehose** — `#nostr` or similar; 200+ events/sec.
- Cold-start to first painted frame.
- Memory footprint at idle, after 5 minutes of activity, after 1 hour.
- Battery proxy (mobile): wakelock duration, CPU time.

**Budgets** (spec §7.16) are the targets. Failures are tracked as bugs and fixed in-arc.

**Outputs:**

- `docs/perf/v1.md`: written report with measurements, comparisons across platforms, identified bottlenecks, decisions made.
- Revised budgets if reality dictates (with rationale).
- Open issues for any deferrable bottlenecks.

**Exit gate.**

- All §7.16 budgets met on reference devices, OR explicitly waived with rationale documented.
- No platform shows visible jank under the three workloads on reference devices.
- `docs/perf/v1.md` published.
- The **SQLite-as-shared-store hybrid** (spec §A2) decision is made on data: either v2 path declared, or marshaling pattern declared sufficient.

**Regression test added.** `tests/perf_replay.rs` runs a canned workload in CI and asserts on the always-on counters in the proof app's reported snapshot. Catches regression between releases.

---

## Arc 3 — Release (Phases 10–11)

### Phase 10 — CLI, starter app, docs

**Scope.** Per spec §8, §4.3, §4.5.

- `nmp init` with all platform options.
- `nmp add ios|android|desktop|web`.
- `nmp gen bindings|view|action|screen`.
- `nmp doctor`.
- `nmp upgrade`.
- The **starter app** (distinct from proof app; minimal): login + timeline + compose + profile + DMs. Stays under the platform LOC budgets from spec §3.2.
- Documentation set: recipe book (`docs/recipes/`), NIP support matrix (`docs/nips.md`), migration guide (`docs/migration.md`).

**Exit gate.**

- A developer with no prior framework knowledge can `nmp init`, follow recipes, and have a working hashtag-feed app on all four platforms in ≤ 2 hours.
- §3 of the spec (success criteria) is reproducible from published docs alone — no insider knowledge required.

### Phase 11 — v1 release

**Scope.**

- Resolve naming (`aim.md` §7.7).
- Publish crates to crates.io.
- Publish CLI to npm as `@nmp/cli` (with final name substituted).
- Tag release; publish bindings; deploy example apps; announce.

**Exit gate.**

- Public availability.
- Three external developers ship a real app within 30 days of release.

---

## Test pyramid

| Level | Tooling | What it covers | Where it lives |
|---|---|---|---|
| Unit | `cargo test` per crate | Pure-function correctness | Each crate's `tests/` |
| Subsystem integration | `cargo test --test '*'` in `nmp-testing` | EventStore + planner + sync against MockRelay | `nmp-testing/tests/` |
| Cross-FFI | `cargo test --features ffi` | Bindings round-trip, rev ordering, callback delivery | `nmp-ffi/tests/` |
| Cross-platform consistency | Script harness | Same scenario on iOS sim + Android emu + desktop + headless web; assert `AppState` JSON equality | `nmp-testing/scenarios/` |
| Proof-app smoke | XCUITest + Espresso + iced UI test + Playwright | End-to-end flows render without error | `nmp-proof/<platform>/tests/` |
| Performance | `nmp-metrics` replay | Counters under canned workloads | `nmp-testing/perf/` |
| Manual exploratory | Humans on reference devices | What the metrics can't catch | Phase 9 |

The cross-platform consistency tests are the highest-value layer: they catch every drift between platforms and force the doctrine (Rust owns everything but rendering) to remain real.

---

## Decision log (where we'll keep deviation receipts)

`docs/decisions/` will hold one short markdown per non-trivial decision made during Arcs 1–3. Format:

```
# ADR N: <title>
Date: YYYY-MM-DD
Status: proposed | accepted | superseded

## Context
## Decision
## Consequences
## Alternatives considered
```

Initial ADRs to write at the start of Phase 0 (from the spec itself):

1. Snapshots + ViewBatch from day one (vs snapshot-only MVP).
2. Negentropy promoted to engine, not feature.
3. View payloads are non-optional with placeholders (D1).
4. SQLite-shared-store explicitly deferred to v2 pending Phase 9 data.
5. Proof app is a v1 release gate.
6. Starter app stays minimal even though we have a richer proof app.

ADRs already adopted:

- **ADR-0001:** Composite dependency keys (composite-first reverse index, broad axes guardrailed). Adopted 2026-05-17 from reactivity-bench run 001.
- **ADR-0002:** Delta-volume budget is per-view (60/view/sec), not absolute. Adopted 2026-05-17 from reactivity-bench run 001.
- **ADR-0003:** Memory budget is working-set, not total cached events. Adopted 2026-05-17 from reactivity-bench run 001.
- **ADR-0004:** Allocation measurement plumbed via counting allocator (verifies zero-per-event invariant). Adopted 2026-05-17 from reactivity-bench run 001.
- **ADR-0005:** Platform shadow is domain-keyed, not `ViewId`-keyed. Refcounted component wrappers (`useProfile`, `@Profile`, `rememberProfile`) generated per platform manage subscription lifecycle behind the domain-keyed API. `ViewId` remains an internal FFI token only.
- **ADR-0006:** Vertical-slice-first delivery for Phase 1. Kind:0 profile-metadata path runs end-to-end (desktop component → wrapper → actor → in-memory store → real relay → back) before the broader Phase 1 scope (LMDB, outbox, full view kinds, FFI to iOS/Android) layers on top. Adopted 2026-05-17 from the firehose-bench run that revealed the live mode was blocked on real runtime adapters.
- **ADR-0007:** Relay/subscription diagnostics and non-Nostr data use the same actor-owned `AppUpdate` bridge, but with explicit diagnostic/domain records instead of raw callbacks or fake Nostr events. Adopted 2026-05-17 to clarify network visibility and capability/domain-data flow before expanding the vertical slice.

The ADRs are the durable record of why design decisions exist. New ADRs land alongside any new harness run that revises a design.

### The harness-first pattern

Every design doc has measurable gates. Gates run on the reactivity-bench harness (or a sibling for non-reactivity subsystems). Failures revise the design *before* implementation. Pre-implementation measurement is cheaper than post-implementation rework. Run 001 of reactivity-bench established the pattern: the reverse-index direction was validated (100×–1000× headroom), one design refinement landed (composite keys), and two budget bugs surfaced (per-view delta, working-set memory) — all before any view-kind code shipped.

### Modeled budget contract vs runtime evidence

Two distinct claims about the same harness:

- **Modeled budget contract.** Replay mode runs deterministic synthetic workloads through a model of the runtime (modeled relay sockets, modeled storage, modeled UniFFI marshaling). Passing here proves the budgets are internally consistent and the harness scaffolding is sound. It does **not** prove the real runtime hits those budgets.
- **Runtime evidence.** Live mode (or replay mode against a real adapter substituted for a modeled segment) runs against actual LMDB, actual WebSockets, actual UniFFI marshaling. Passing here is real evidence.

Today's firehose-bench replay passes establish the contract. The vertical slice (ADR-0006) is what produces the first runtime evidence. Each subsequent phase replaces another modeled segment with a real adapter and graduates the corresponding firehose-bench scenarios from "modeled" to "measured."

Reports in `docs/perf/firehose-bench/` track which scenarios are measured vs modeled at each run. Live runs are explicitly tagged. CI runs replay against the current set of real adapters plus models for the rest; the boundary moves rightward as phases land.

### CI / pre-merge hygiene

The recommended CI gates as of Phase 1:

- `cargo fmt --all -- --check` (formatting).
- `cargo test --workspace` (all crates pass unit + integration).
- `cargo run -p nmp-testing --bin reactivity-bench --release -- --standard --fail-on-gate` (reactivity gates).
- `cargo run -p nmp-testing --bin firehose-bench --release -- replay --standard --fail-on-gate` (firehose gates against the current model+adapter mix).
- `git diff --check` (whitespace / conflict markers).

Live firehose runs are not in pre-merge CI (they would block on relay flakes); they run nightly or on-demand and produce reports tagged `live` in `docs/perf/firehose-bench/`.

### Unit-test guidance from firehose-bench run 001

Beyond integration tests, Phase 1 explicitly carries unit-test coverage for:

- **Composite reverse index** — composite-key matching, false-wakeup rate measurement, broad-axis guardrail warnings.
- **Coalescer** — per-view-kind merge rules (`UpdatedMany`, range-merged `Inserted`, `EmojiAdjusted` summing, etc.) preserve semantic equivalence to N un-coalesced deltas.
- **Domain-keyed wrapper lifecycle** — refcount transitions, grace-period cancellation, eviction correctness, idempotent `Open`/`Close` calls.

These three areas had the highest concentration of subtle invariants in the design docs; they get the highest test density.

---

## What this plan is not

- It is not a schedule. No dates, no person-months. Phases are sequential; their durations depend on team size and surface complexity.
- It is not a marketing roadmap. v1 ships when the gates are met, not on a calendar.
- It is not exhaustive. Sub-tasks within each phase will be planned in design docs as we enter the phase.
- It does not commit to specific dependency versions; those go in `docs/nips.md` and `Cargo.toml` and evolve.

The plan exists so that any single phase can be picked up cold by someone reading this doc + the spec + the relevant ADRs, and they can execute without bothering the rest of the team.
