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

## Rung 3 — Producer omits Unchanged keys; NMP-owned rev-aware host apply (the floor)

> **Full design + sub-step ladder + codex review: [`0055-rung3.md`](0055-rung3.md).**
> That addendum is the single source of truth for this rung; the summary below is
> the ladder-level pointer only.

> **STATUS: LANDED (2026-06-14).** All sub-steps merged:
> S1 #1388 (producer omit + capability) · S1b #1393 (Cleared-signal completeness,
> fix #1390) · S2 #1389 (encoder buffer reuse) · S3 #1409 (iOS ProjectionCache
> interposer, iOS advertises) · S4 #1410 (Android interposer, Android advertises) ·
> S5 #1413 (S6 capstone + empirical PASS/FAIL gate). Doc PR (this) = S6.
> **Measured (S6 capstone, opus-reproduced 5×):** ~18% frame-byte reduction
> (9640→7928 B p50) + 68.8% Tier-2 row suppression (1600→500 rows/window), zero
> data loss (byte-identity oracle, end-state fail-closed). The capstone gate metric
> is `row_suppression_ratio ≥ 0.50`, not `waste_ratio < 0.05` — the latter is
> unachievable while Tier-1 feed-class projections stay always-Changed (D3-7), and
> those two keys (`claimed_event_embeds`, `nip46_onboarding`) are the entire 40%
> residual hash-waste. **The larger remaining byte win is Tier-1 / feed gating
> (row-deltas) — a future rung (see §"Rung 6 / D8").**

**Goal:** the actual O(changed) win. The producer stops serializing projections
whose `projection_rev` is unchanged since the last emit; an NMP-owned (generated)
host cache-merge layer retains the prior decoded value for omitted keys so the app
keeps using the same accessors, oblivious to the delta mechanics.

- **Producer (R3-S1/S2, `nmp-core`):** new `update/rung3_omit.rs` drops
  `Unchanged` rows entirely (omit the whole row, not an empty payload — D3-1),
  keeps an explicit payload-less `Cleared` row, keeps full `Changed` rows.
  Gated on a per-instance `nmp_app_declare_incremental_apply()` capability
  (D3-2): the kernel emits full rows until a host advertises the cache-merge
  layer. Advertising it (or any epoch bump) clears `last_emitted_revs` so the
  next frame is a full baseline (D3-5). Plus FlatBufferBuilder reuse across ticks
  (D3-6). This gates the Tier-2 built-ins (the ADR-0037-ungated hot path).
- **Host (R3-S3 iOS, R3-S4 Android):** a GENERATED `ProjectionCache` interposer
  (`ProjectionCache.generated.swift` / `ProjectionCache.kt`) sits between the raw
  FlatBuffers decode and the existing per-key typed decoders. It keys a persistent
  `key → (rev, raw bytes)` cache; `Changed` overwrites, `Cleared` drops,
  omitted/`Unchanged` retained; then re-feeds the *merged full envelope set* to
  the unchanged decoders — so app code and accessors do not change (D3-3). It
  surfaces the changed-key set so `apply()` re-assigns only changed slots (D7
  win). `TypedProjectionEnvelope` gains `projectionRev` + `state`.
- **Self-healing floor (D3-4, codex-driven):** the channel is a synchronous
  in-process callback (no in-transit loss/reorder), so **decode-before-commit +
  a sticky `needsResync` latch is sufficient** — the host advances per-key
  applied-rev only after a successful typed decode + commit; on failure it keeps
  the prior value, does not advance rev, and latches `needsResync`. **No
  per-frame full manifest in Rung 3.** The manifest-gap detector + the resync FFI
  that drains `needsResync` are Rung 4.
- **Tier-1 stays always-Changed (D3-7):** unregistered host projections (feed,
  wallet, dm_inbox, …) are emitted `Changed` when live, explicit `Cleared` when
  absent — never silently omitted. Their rev gating is a later rung; the S6 metric
  is therefore labeled "Tier-2 / claimed_profiles churn," not whole-frame waste.
- **Gallery:** Android/iOS shells now pass production `UpdateFrame` bytes to the
  Rust `nmp-app-gallery` helper, which decodes the typed envelope and Gallery
  sidecars into the existing Gallery JSON model. The native curated
  `payload:Value` transport subset is gone. See `0055-rung3.md` §6.
- **Capstone gate (R3-S5):** re-run ffi-stress S6 with incremental apply ON vs
  OFF; assert Tier-2 `waste_ratio < 0.05`, frame bytes ON < OFF, no `serialize_us`
  regression, and an incremental-vs-full byte-identity oracle. Plus the Rung-1
  biconditional oracle extended to the omission case (omitted ⟺ cache-unit
  unchanged) and per-platform `ProjectionCache` unit tests.

**Reviewable value:** this rung delivers the owner's MINIMUM bar (O(changed
projections)) AND the host-apply win, with the self-healing invariant under test
and the 81.2 % Tier-2 waste empirically driven to ~0.

---

## Rung 4 — Host-requested resync FFI (escape hatch) + epoch on reset paths

**Goal:** make the model unconditionally self-healing (D5).

- Add `nmp_app_request_full_snapshot()` FFI (bumps epoch → next frame is a full
  baseline). Host calls it on first attach, unrecognized epoch, and **to drain the
  `needsResync` latch the Rung-3 cache-merge layer sets on a typed-decode failure**
  (`0055-rung3.md` D3-4). Until this FFI lands, Rung 3 only logs `needsResync` and
  relies on the next genuine rev bump to re-emit the degraded key.
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
