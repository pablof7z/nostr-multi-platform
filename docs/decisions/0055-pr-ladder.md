# ADR-0055 implementation PR ladder

> Companion to `docs/decisions/0055-incremental-projection-emission.md`. Each rung
> is independently reviewable, respects crate boundaries + the 500-LOC ceiling +
> the snapshot/rev correctness invariant + D8 (no polling). Rungs are ordered so
> the build stays green and each adds verifiable value without the next. The
> migration order follows the SOTA pattern (instrument → producer rev manifest →
> rev-aware host apply → built-in gating → compose with interest → retire dual
> emission) corroborated by the research and the independent codex review.
>
> **Do not merge as one mega-PR.** The owner reviews each rung.

---

## Rung 0 — Instrumentation (measure before optimizing)

**Goal:** make the win measurable and prevent regressions before changing behavior.

**LANDED** — this PR. Adds:
- Per-emit `NMP_PERF` log line extended (in `test-support` builds only) with a
  per-projection key breakdown (`projection_count`, `changed_projection_count`,
  `wasted_bytes`) computed by the `churn` submodule in
  `crates/nmp-core/src/kernel/update/helpers.rs`.
- **Instrumentation is entirely `cfg(any(test, feature = "test-support"))`-gated**
  — a production build does ZERO measurement work on the emit path: no payload
  hashing, no per-key store, no counters. The measurement pass (a `DefaultHasher`
  fingerprint of each projection's payload, O(payload bytes)) runs only in
  test-support builds (ffi-stress enables `test-support`). Rung 1's real O(1) rev
  manifest supersedes this measurement; it is never carried into production.
- State (`PREV_PAYLOAD_HASHES`) lives in a `test-support`-only thread-local plus
  two process-global `AtomicU64`s (`PROCESS_PROJECTIONS_SERIALIZED` /
  `PROCESS_PROJECTIONS_CHANGED`), NOT on `Kernel` — the production `Kernel`
  struct carries zero instrumentation fields.
- New ffi-stress scenario `S6-single-projection-churn` in
  `crates/nmp-testing/bin/ffi-stress/s6_single_projection_churn.rs`: drives a
  workload where **only one projection family changes per emit cycle** (a profile
  claim cycles through `claim_profile`/`release_profile`, dirtying the
  `claimed_profiles` + `resolved_profiles` cluster) while the other built-ins are
  static — measures `projections_serialized / projections_changed` ratio and the
  wasted-bytes ratio.
- The flood baseline (`snapshot-pressure` S3) re-run for comparison.

**Test:** `cargo test -p nmp-core --lib kernel` + doctrine smoke.

**Reviewable value:** empirical before-numbers; the churn measurement anchors
whether Rung 1 (per-projection gating, O(changed)) is sufficient or feed
row-deltas are also needed. Concrete figures in the PR body.

---

## Rung 1 — Kernel-owned per-projection revision manifest (no wire change yet)

**Goal:** the kernel knows, per projection key, a monotonic `projection_rev`,
bumped at the mutation site. No transport change — this rung just establishes and
unit-tests the rev source of truth.

- Add `projection_revs: HashMap<&'static str, u64>` (or a typed counter struct
  for the 18 built-ins) to the kernel.
- Bump the rev at each mutation site that dirties a projection's source. Reuse the
  existing `changed_since_emit` dirtying discipline as the bump anchor — wherever
  a projection's backing state changes, bump its rev. For the built-ins, the bump
  sites are the reducer paths that touch `profiles`, `profile_claims`,
  `event_claims`, `configured_relays`, accounts, publish queue, relay diagnostics
  source, etc.
- Drain projections (`action_results` / `signed_events` / `action_stages` /
  `action_lifecycle`): bump on enqueue of a settled item.
- Crates: `nmp-core` only.
- Test (`-p nmp-core`): rev monotonic; rev advances iff source changed;
  unchanged source across two ticks → same rev. A table-driven test per built-in
  key asserting "mutate source ⇒ rev++, no-op tick ⇒ rev stable."

**Reviewable value:** the change-detection primitive, fully tested, before any
wire or host risk. Rejects content-hashing (D2) — this is the O(1) gate.

---

## Rung 2 — Wire contract: add `projection_rev` + `state` + `snapshot_epoch`

**Goal:** extend the transport carrier; producer emits the rev/state; full
baseline on epoch change. Still emits all changed projections (the host can't
skip yet — that's Rung 3), so this rung is byte-additive and backward-safe.

- `crates/nmp-core/schema/nmp_update.fbs`:
  - `TypedProjection` gains `projection_rev:ulong` and
    `state:ProjectionState = Changed` (enum `Changed`/`Cleared`; absence ==
    Unchanged per D3). Appended at the tail.
  - `SnapshotFrame` gains `snapshot_epoch:ulong` and `session_id:ulong` at the
    tail (D4 — `session_id` = `TimingMilestones::started_unix_ms` so a host can
    detect a process restart vs a stale frame; `snapshot_epoch` monotonic
    within a session).
  - Regenerate checked-in bindings (Rust + Swift + Kotlin); the
    `rust-flatc-drift` gate enforces regeneration.
- Producer (`update.rs` / `update_envelope.rs`): stamp each typed projection with
  its `projection_rev`; stamp the frame's `snapshot_epoch` + `session_id`; set
  `session_id` once at init from `TimingMilestones::started_unix_ms`; bump epoch
  on account-switch/schema-change (D4).
- This rung does NOT yet omit unchanged keys (keeps emitting all changed
  projections every tick exactly as today) — it only adds the metadata. Old hosts
  ignore the new tail fields (graceful degradation).
- Crates: `nmp-core` + generated bindings in each host crate (codegen only).
- Test (`-p nmp-core`): frame carries correct rev per key; epoch bumps on the
  three reset triggers; round-trip decode of the new fields.

**Reviewable value:** the wire is ready; nothing can desync because behavior is
unchanged. Pure additive metadata.

---

## Rung 3 — Producer omits Unchanged keys; host reuses prior buffer (the floor)

**Goal:** the actual O(changed) win. The producer stops serializing projections
whose `projection_rev` is unchanged since the last emit; the host reuses its
cached decoded value for omitted keys.

- Producer: maintain `last_emitted_revs: HashMap<&'static str, u64>`. In
  `make_update`, a projection is serialized iff its rev advanced (or epoch reset).
  Unchanged → omitted (D1/D3). `Cleared` emitted explicitly when a projection
  goes absent (e.g. a view closes).
- This is where the built-in typed projections + Tier-3 built-ins get gated —
  the hot path that ADR-0037 left ungated.
- Host (one PR per platform, can land independently — start iOS):
  - `KernelModel.swift` / `KernelBridge.swift`: keep a per-key applied-rev map +
    cached decoded value. On a frame: apply `Changed` keys, reuse cache for
    omitted/`Unchanged`, drop on `Cleared`. **Only assign the slot whose rev
    advanced** (D7) — this kills the SwiftUI broad-invalidation churn.
  - **Host-side fine-graining caveat (UI-bridge research):** the slot must be a
    per-projection `@Observable` property (iOS 17+ `ObservationRegistrar`), NOT a
    single coarse `ObservableObject` whose `objectWillChange` fires for any
    field — that would defeat the host-side win (every view re-renders regardless
    of rev). A platform still on `ObservableObject` gets the producer-side win
    (smaller frames) but not the host-apply win until it migrates.
  - Per-key monotonic guard: ignore a `projection_rev ≤` already-applied (reorder
    safety, D3).
  - On `session_id` change OR `snapshot_epoch` change: reset the applied-rev map
    and re-baseline (D4).
- Crates: `nmp-core` (producer) + `ios/Chirp` (host). Android/desktop/TUI hosts
  follow in sibling PRs; until they're rev-aware they still work (a host that
  treats every present key as authoritative and ignores omission still renders
  correctly — it just doesn't get the host-side win).
- Test: **the core correctness property test** — generate a random sequence of
  source mutations; assert that applying the incremental stream (with random
  drops/reorders injected, each followed by the self-healing rules) yields a host
  state byte-identical to a full snapshot of the final kernel state. Plus iOS
  unit tests for the apply cache.

**Reviewable value:** this rung delivers the owner's MINIMUM bar (O(changed
projections)) AND the host-apply win, with the self-healing invariant under test.

---

## Rung 4 — Host-requested resync FFI (escape hatch) + epoch on reset paths

**Goal:** make the model unconditionally self-healing (D5).

- Add `nmp_app_request_full_snapshot()` FFI (bumps epoch → next frame is a full
  baseline). Host calls it on first attach, decode failure, unrecognized epoch.
- Wire the epoch bump into every reset path not already covered in Rung 2
  (background/foreground re-attach if relevant, store reopen).
- Crates: `nmp-ffi` + `nmp-core` + host call sites.
- Test: resync call forces a full baseline next tick; decode-failure path
  triggers resync; post-resync host state == full snapshot.

**Reviewable value:** the worst case for ANY future delta/gating bug becomes "one
extra full snapshot," never a permanent desync. This is the safety capstone.

---

## Rung 5 — Compose with ADR-0053 (host-declared projection interest)

**Goal:** interest (which projections) × rev-gating (how much of each) compose.

- ADR-0053 (host-declared projection subscriptions) is already on master. A
  projection the host has NOT declared interest in is never serialized at all
  (set narrowing); a declared one is rev-gated (per-tick narrowing). The two are
  orthogonal: interest filters the key set before rev-gating runs.
- Epoch interaction: declaring/withdrawing interest is an epoch-class event for
  the affected key (host gets a fresh baseline for newly-declared keys).
- Crates: coordinate with Rung 2's epoch machinery.
- Test: declared+changed → emitted; declared+unchanged → omitted;
  undeclared → never emitted; newly-declared → baseline next tick.

**Reviewable value:** proves the two redesigns multiply rather than fight.

---

## Rung 6 (deferred, separate ADR) — Intra-projection row deltas for the feed

**Goal:** O(changed rows) for the one projection that justifies it.

- Only after Rung 3 is measured in production AND Rung 0 measurements confirm
  the feed projection is a dominant remaining cost. Target: `nmp-feed` viewport
  projection (ADR-0033), which already has cursor/viewport mechanics.
- `Changed` payload for the feed becomes a row-delta (added/modified/removed by
  stable row key — event id / nostrdb note key, never positional) against the
  prior rev, with a full-row-set fallback when the host's rev is stale/absent or
  epoch reset.
- This is additive to the Rung 2 wire contract (a per-schema encoding choice
  inside a `Changed` frame); it does not touch the rev/epoch contract.
- Gated on a follow-up ADR + production profile evidence that the feed is the hot
  append stream. **Do NOT build speculatively.**

---

## Cross-cutting: coordinate, don't conflict

- `fix/relay-diagnostics-raw-timestamps` (#1332, merged): the relay-diagnostics
  projection was pre-formatting "Xs ago" labels against wall-clock `now`, which
  would make its rev bump every second (poisoning the gate). That PR ships raw
  Unix-ms timestamps instead (host formats). **Rung 1/3 now correctly gates
  `relay_diagnostics` on real change, not on the clock ticking.** The Rung 0
  measurement scenario exercises this directly.
- `perf/drop-flatbuffers-verifier-on-trusted-decode`: orthogonal host-decode
  speedup; complementary, no conflict.
- `op_feed_defaults.rs:262` documents a duplicate feed materialization on the same
  tick — named for Rung 6 scope; not a blocker for the floor.

## Doc PR (lands with Rung 3)

Amend aim.md §10 + Doctrine #12 to "incremental-by-default for projections; full
snapshot is the baseline/resync shape," pointing at ADR-0055. Resolve aim.md §7.1
open question ("state granularity across FFI") by reference to this ADR.
