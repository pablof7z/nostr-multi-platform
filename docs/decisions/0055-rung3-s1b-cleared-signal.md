# ADR-0055 Rung 3 S1b — Cleared-Signal Completeness

Extracted from [`0055-rung3.md`](0055-rung3.md) to keep each hand-authored ADR file under the repository file-size ceiling. This addendum is the design call for issue #1390 before incremental apply becomes default.

## 10. Rung 3 S1b — #1390 Cleared-signal completeness (HARD BLOCKER before incremental_apply default)

- **Status:** Proposed (2026-06-14). Resolution of GitHub issue #1390 — a 9-finding
  (5 HIGH / 3 MED / 1 LOW) re-audit of R3-S1 (#1388) and R3-S2 (#1389), both merged
  to master @ `c6f3486f5`. This is a **hard blocker before `incremental_apply` is
  enabled on any host** (it is OFF by default today, so nothing is live yet — the
  whole cluster is latent). The auditor explicitly left the Cleared-signal
  *mechanism* to the ADR-0055 owner as a design call; this section is that call.
- **Scope clarification:** the omit-Unchanged seam landed in S1 (`rung3_omit.rs`)
  and the rev/state stamping landed in S2. S1b is the *correctness completion* of
  that seam: it makes the producer emit a Cleared signal for the conditionally-present
  projections that currently go silent on a non-empty→empty transition.

### 10.1 The decisive finding — the manifest IS the full key universe (the fix is in the consumer, not the manifest builder)

The single question that determines the whole fix shape: *does the manifest contain
an entry for a key even when that key produced no row in `typed`?*

**Answer: YES — confirmed with file:line evidence. The manifest enumerates the FULL
universe of known projection keys, independent of `typed`.**

- `projection_rev/mod.rs:415` `build_manifest` iterates
  `KERNEL_BUILTIN_PROJECTION_KEYS` (the const array at
  `update/projections.rs:27-46`, all 18 Tier-2 keys) and emits one `ProjectionState`
  per key, every tick — *not* one per row present in `typed`.
- For the two drain keys, presence flips to `Cleared` on the genuine non-empty→empty
  edge **independent of `typed` insertion**: `take_action_results_projection` /
  `take_signed_events_projection` (`publish_cmd.rs:228`) call
  `note_drain_emit(key, nonempty)` *unconditionally* (the drain always runs; ADR-0053
  only gates the typed-row *capture*, not the drain). `note_drain_emit`
  (`mod.rs:266-290`) bumps `settlement_drain_ver` and parks
  `pending_presence[key] = Cleared` on the `!nonempty && was_nonempty` edge. That
  parked presence is read by `presence_for` (`mod.rs:333`) → the manifest entry is
  `Cleared` even though the accessor returned `Null` and **no typed row exists**.

**Therefore the finding's claim "the manifest correctly flips to Cleared" is
CONFIRMED for the drain keys, and the fix is in the *consumer* (`omit_unchanged`),
not the manifest builder.** The Cleared entry already exists in the manifest; the
transform simply never looks at manifest entries whose key is absent from `typed`
(`rung3_omit.rs:44-72` iterates `typed.into_iter()` only). This is the inverse-path
gap the finding (#3) names.

**Crucial caveat that splits finding 7 off from findings 1/2/3/8:** the manifest
flips to `Cleared` *only for the two true drain keys* (`action_results`,
`signed_events`), because only they run `note_drain_emit`. The two copy-with-TTL
keys (`action_stages`, `action_lifecycle`) use the **rev-vs-last-emit rule**, NOT the
drain state machine — so their non-empty→empty transitions do NOT produce `Cleared`
presence today. They produce `Changed` (if a counter bumped) or `Unchanged` (if not).
See §10.4.

### 10.2 The Cleared-synthesis contract (findings 1/2/3/8 — HIGH, same root)

`omit_unchanged` gains an **inverse pass** after the existing present-row filter:

```
fn omit_unchanged(typed, manifest, enabled) -> Vec<TypedProjectionData>:
  if !enabled: return typed                       # unchanged
  out = existing present-row filter (Changed keep / Cleared strip+flag / Unchanged drop)
  present: HashSet<&str> = out.keys()
  for ps in manifest.states:
      if ps.key in present: continue              # one row per key per frame (codex #2)
      if should_synthesize_clear(ps):             # see predicate below
          out.push(TypedProjectionData {
              key: ps.key.to_string(),
              state: WireProjectionState::Cleared,
              projection_rev: ps.rev,              # manifest rev (advanced on the edge)
              ..Default::default()                 # empty payload/schema_id/version/file_id
          })
  out
```

**The synthesized Cleared row** = `{ key, state: Cleared, projection_rev: ps.rev,
payload: empty, schema_id/version/file_identifier: default }`. This is correct
because the host's `Cleared` merge path (D3-3, `cache.remove(row.key)`) is keyed on
`row.key` ONLY and never decodes a Cleared payload — schema metadata is irrelevant
for a drop. `TypedProjectionData` already derives `Default` (`update_envelope.rs:217`)
and `WireProjectionState::Cleared` exists (`projection_state.rs`; the wire enum is
`{Changed, Cleared}` only — `Unchanged` == absence, never on the wire), so the
struct literal above compiles as written.

**The synthesis predicate `should_synthesize_clear(ps)` — narrowed per codex #5:**

```
const CONDITIONAL_EMPTY_MEANS_CLEAR: &[&str] =
    &["action_results", "signed_events", "action_stages", "action_lifecycle"];

should_synthesize_clear(ps) =
    match ps.presence {
        Cleared  => true,                                    # drains, post §10.4 also stages/lifecycle
        Changed  => CONDITIONAL_EMPTY_MEANS_CLEAR.contains(ps.key),  # belt-and-braces, see §10.4
        Unchanged => false,
    }
```

- **`presence == Cleared` → always synthesize.** This handles the two drain keys
  today, and `action_stages`/`action_lifecycle` once §10.4 gives them a Cleared edge.
- **`presence == Changed && key ∈ CONDITIONAL_EMPTY_MEANS_CLEAR && absent` →
  synthesize Cleared (defensive belt).** Codex #5: for *exactly these four keys*,
  absence is unambiguous — the accessor returns `Null` iff the tracker is empty
  (`projections.rs:157/175/189/209` — `!x.is_null()` gates the insert). So a
  `Changed`-but-absent conditional key always means "went empty," and the only safe
  signal is `Cleared`. This belt makes the fix robust even if §10.4's edge machine
  is incomplete or a future counter-bump path is added without an edge.
- **`presence == Changed && key ∉ CONDITIONAL_EMPTY_MEANS_CLEAR && absent` →
  INVARIANT VIOLATION, do NOT synthesize.** For an unconditional Tier-2 key, a
  `Changed`-but-absent manifest entry is a producer bug (the key should always emit a
  row). Synthesizing a Cleared here would silently delete live host state and mask the
  bug (codex #5). Instead: `debug_assert!` + a `tracing::warn!` (one-shot,
  rate-limited) and leave the row absent. This is the "sharp invariant for normal
  projections" codex demanded.
- **Tier-1 keys have NO manifest entry** (`rung3_omit.rs:50-55` default), so they are
  never iterated by the inverse pass — structurally impossible to synthesize
  (codex #5 requirement satisfied for free; the manifest only contains the 18 Tier-2
  keys).

### 10.3 Spurious-clear / repeated-clear safety — airtight, with the load-bearing reason (codex #1, #3)

Codex flagged that synthesis is only safe if `Cleared` is **edge-triggered, not
level-triggered**, and that the clear must fire **exactly once** (not every tick).
Both hold here, and the reason is `record_emitted` discipline:

- **Edge, not level (codex #1):** `note_drain_emit` (`mod.rs:275-286`) produces
  `Cleared` ONLY on `!nonempty && was_nonempty`; a never-present key sits at
  `!nonempty && was_empty` → `Unchanged` forever → never synthesized. Codex's "breaks
  if `was_nonempty` derives from host/cache state" failure mode does NOT apply:
  `drain_prev_nonempty` (`mod.rs:210/287`) is **producer-local snapshot state**
  (whether the *drain itself* carried content last tick), never host-observed state.
- **Exactly once (codex #3 — the dangerous detail):** codex's "repeated clears if
  `record_emitted` only scans `typed` rows" failure mode is AVOIDED because
  `record_emitted_for_manifest` (`rung2_stamp.rs:63`) iterates **`manifest.states`**
  (the full key universe) and calls `record_emitted(ps.key)` for *every* key —
  including the Cleared key that produced no typed row. So `last_emitted[cleared_key]`
  advances to the post-edge rev on the Cleared tick. Next tick: rev unchanged →
  `presence_for` → `Unchanged` → not synthesized. **The Cleared row fires exactly
  once on the edge.** This is verified at `update.rs:381` (production) and
  `test_helpers.rs:58/102` (test path) — both call `record_emitted_for_manifest`
  AFTER the encode. *No change to `record_emitted` is required for the drain keys.*
- **Codex's "advance before handoff" concern (#3) is a non-issue under D3-4:** the
  callback is synchronous in-process; "callback returned" == "applied." A host-side
  typed-decode failure latches `needsResync` (D3-4), and a `Cleared` row has no
  payload to fail-decode, so it always commits. No periodic baseline needed.

### 10.4 Finding 7 — the real fix is a Cleared edge machine, NOT a counter bump (HIGH, codex #4 overturned the naive fix)

The issue text proposed: *"bump `settlement_enqueue_ver` in the ack path"*
(`publish_cmd.rs:320` `ack_action_stage` removes the entry + sets
`changed_since_emit` but bumps **no** source-version counter, so after ack-of-last-
entry the snapshot is `Null` AND presence stays `Unchanged` → host keeps the stale
pre-ack stage forever).

**Codex #4 proves the naive bump is INSUFFICIENT and would mask the bug.**
`action_stages`/`action_lifecycle` presence is the **rev-vs-last-emit rule**
(`presence_for` → `changed_since_last_emit`), NOT the drain state machine. Bumping
`settlement_enqueue_ver` makes the rev advance → presence becomes **`Changed`**, not
`Cleared`. But the row is **absent** (snapshot `Null`). So we get a
*`Changed`-but-absent* key — which the pure `presence == Cleared` synthesis would NOT
catch. The host would still keep the stale stage.

**The correct fix (chosen, codex's "best architecture"):** give `action_stages` and
`action_lifecycle` the **same Cleared-edge state machine the drains have** — make the
manifest presence *semantically correct*, then synthesis stays primarily
`presence == Cleared`:

1. Add an edge tracker for these two keys analogous to `drain_prev_nonempty`
   (a `copy_prev_nonempty: HashMap<&'static str, bool>` on `ProjectionRevTracker`,
   or extend the existing map's contract to cover all four conditional keys), plus a
   `note_copy_emit(key, nonempty) -> ProjectionPresence` analogous to
   `note_drain_emit`: `nonempty → Changed (no extra bump — the rev already moved on
   enqueue/expiry)`; `!nonempty && was_nonempty → Cleared`; `!nonempty && was_empty
   → Unchanged`.
2. Call `note_copy_emit("action_stages", !snapshot.is_null())` inside
   `action_stages_projection` (`publish_cmd.rs:331`) and
   `note_copy_emit("action_lifecycle", !result.is_null())` inside
   `action_lifecycle_projection` (`publish_cmd.rs:302`), once per emit — the same
   "exactly once per emit per key" discipline the drains use. These accessors already
   run every tick (the TTL sweep must run regardless of declaration), so the edge is
   observed on the ack-of-last-entry tick.
3. **Result:** ack-of-last-entry → next emit, snapshot `Null`, `was_nonempty=true` →
   `note_copy_emit` parks `Cleared` → manifest presence `Cleared` → the §10.2
   synthesis emits the Cleared row → host drops the stale stage. The counter-bump in
   the ack path is then **unnecessary** for correctness (the edge machine carries the
   signal), but `ack_action_stage` should still set `changed_since_emit = true` (it
   already does) so the emit happens promptly.
4. **`DRAIN_PROJECTION_KEYS` (`mod.rs:225`) is generalized** to a
   `CONDITIONAL_PRESENCE_KEYS` covering all four, OR a second sibling const for the
   copy-with-TTL pair, so `presence_for`'s `pending_presence` lookup covers them.

**The `Changed`-but-absent belt in §10.2's predicate is the backstop** for any
conditional key whose edge machine is momentarily wrong: it converts a stray
`Changed`-but-absent (for the four whitelisted keys only) into a Cleared, so the host
is never stuck even if the edge machine has a gap. This is defense-in-depth, not the
primary mechanism (codex: "best architecture: make presence correct, keep synthesis
== Cleared, add Changed+absent only for the explicit set as a belt").

### 10.5 Per-finding fix specs (the remaining MED/LOW)

**Finding 5 (MED) — `declare_incremental_apply` pre-start invariant is
`debug_assert` only** (`nmp-ffi/src/lib.rs:1572`; silent in release; a post-start
call silently half-enables):

- Change `NmpApp::declare_incremental_apply(&self)` →
  `declare_incremental_apply(&self) -> Result<(), IncrementalApplyError>` returning
  `Err(AlreadyStarted)` when `self.started.load(SeqCst)` (replacing the
  `debug_assert!`). The poisoned-mutex arm returns `Err(RegistryUnavailable)`.
- The C-ABI `nmp_app_declare_incremental_apply` (`nmp-ffi/src/snapshot.rs:194`)
  changes `extern "C" fn(...) ` → returns an `i32` return-code (`0` = ok,
  `1` = already-started, `2` = registry-unavailable, `-1` = null app) — NOT a silent
  no-op on the started case (the null-app case stays a defined return code per D6).
  **No compat shim** (memory: hard-break + upgrade all callers in one PR; downstream
  pins by git rev).
- Update the `AppHost` trait method (`substrate/app_host.rs:134`) signature to match,
  and the impl (`nmp-ffi/src/app_host_impl.rs:57`).
- **Callers to update (the whole set):** `ios/Chirp/.../NmpCore.h:351` (regenerate
  the header — return type `int`); any S6 harness call
  (`s6_single_projection_churn.rs`) must check the return; the Android `.kt` binding
  if/when it lands. iOS does not yet *call* it (finding 4 latent), so the Swift call
  site is the header + the future R3-S3 cache-merge wiring.

**Finding 6 (LOW) — two `snapshot_projections` mutex `lock()`s per 4 Hz tick**
(`update.rs:323` `incremental_apply_enabled()` + the `take_…_baseline_pending()`
call; each locks the slot even when the capability is off):

- Coalesce into ONE acquisition. Add a single
  `incremental_apply_state(&mut self) -> (bool /*enabled*/, bool /*baseline_pending*/)`
  on the `kernel_access.rs` seam that locks once and reads both
  `is_incremental_apply_enabled()` + `take_incremental_apply_baseline_pending()`
  under the same guard. Replace the two call sites at `update.rs:323-326` with the
  single call. Net-neutral on line count. (`kernel_access.rs:115/134` keep their
  individual methods for the test path, or are folded — either way one lock per tick
  in `make_update`.)

**Finding 4 (MED) — iOS apply blanks omitted slots:** explicitly **NOT fixed in this
producer PR.** It is resolved by the R3-S3 `ProjectionCache` interposer (keep-prior
on omit, drop on Cleared — §3 D3-3). **Contract-compatibility confirmation:** the
synthesized Cleared row this design emits is `{ key, state: Cleared, payload: empty }`
— exactly the shape the D3-3 merge consumes (`Cleared: cache.remove(row.key)`,
keyed on `row.key`, no decode). The R3-S3 interposer needs no special-casing for
synthesized vs naturally-Cleared rows; they are byte-identical on the wire. Codex #2
also recommends a **host-side clear reorder guard** (`if cached.rev >= row.rev: ignore
clear`) for future async-transport robustness — note this for R3-S3 as a belt
(not required under today's synchronous in-process delivery).

### 10.6 Finding 9 — the regression gate (MED, this is WHY the cluster went uncaught)

The existing tests structurally skip the exact four keys: `rung3_baseline_tests.rs`
(`:138`, `:257`, `:289`) has `if matches!(key, "action_results" | "signed_events" |
"action_stages" | "action_lifecycle") { continue; }`, and the
`make_update_*_for_test` helpers (`test_helpers.rs`) do drive the full Rung-3 path
*for the keys they assert* — but the assertions never exercise a genuine
non-empty→empty transition on a conditional key. So the silent-stale-cache class was
invisible.

**The new regression test (`rung3_omit.rs` integration-level, or a new
`rung3_cleared_signal_tests.rs` sibling under the 500-LOC ceiling), driving the FULL
incremental path:**

1. Construct a kernel with a bound `snapshot_projections` slot;
   `declare_incremental_apply()`. Declare all four conditional keys consumed.
2. **Drain keys:** dispatch an action that lands a terminal →
   `record_action_success`/`record_action_failure` → emit frame A → assert
   `action_results` present as `Changed` with non-empty payload (host caches it).
   Emit frame B (no new settlement) → **assert a `Cleared` row for `action_results`
   is present in frame B** (the synthesized row: `state == Cleared`, payload empty).
   Emit frame C → assert `action_results` ABSENT (settled to `Unchanged` — fires
   exactly once). Repeat for `signed_events` via `record_signed_event_return`.
3. **Copy-with-TTL keys (finding 7 gate):** `record_action_stage("c1", Requested)` →
   emit frame A → assert `action_stages` present `Changed`. `ack_action_stage("c1")`
   (removes the last entry) → emit frame B → **assert a `Cleared` row for
   `action_stages` is present in frame B** (this is the finding-7 assertion — it
   FAILS on current master, where presence stays `Unchanged`/`Changed`-but-absent and
   NO row is emitted). Emit frame C → assert absent (`Unchanged`). Repeat for
   `action_lifecycle` via TTL expiry (advance a `FixedClock` past the terminal TTL,
   emit, assert the prune produced a `Cleared`).
4. **Simulated host assertion (the "reaches the host" half):** thread the emitted
   `typed` rows through a tiny in-test cache-merge stand-in (the same algorithm as
   D3-3: `Changed` → insert, `Cleared` → remove, absent → keep-prior) and assert the
   stand-in cache **no longer contains** the cleared key after frame B. This proves
   the end-to-end self-healing the real R3-S3 interposer will deliver, without
   depending on Swift/Kotlin.
5. **Spurious-clear negative test:** a key that is empty for the whole run
   (`action_results` with zero settlements) must NEVER produce a `Cleared` row in any
   frame (it sits at `Unchanged`).

**This test MUST FAIL against current master** (the drain-key half fails because
`omit_unchanged` never synthesizes; the copy-with-TTL half fails because presence
never becomes `Cleared`) **and PASS after §10.2 + §10.4.** It is the rung's
correctness gate.

### 10.7 Codex verdict + what changed

`codex exec` (gpt-5.5, xhigh) pressure-tested the Cleared-synthesis design.
**Verdict: sound-with-changes.** Two material changes folded in:

1. **Finding 7 is NOT a counter bump (codex #4).** A `settlement_enqueue_ver` bump
   makes `action_stages` presence `Changed`, not `Cleared`, leaving a
   `Changed`-but-absent key the pure synthesis misses. The fix is the **Cleared edge
   machine** (`note_copy_emit`) for `action_stages`/`action_lifecycle` — §10.4. This
   overturned the issue's proposed fix.
2. **The synthesis predicate is narrowed (codex #5).** `presence == Cleared` →
   always; `presence == Changed && key ∈ {the four conditional keys}` → synthesize
   (defensive belt); `Changed`-but-absent for any OTHER key → `debug_assert!` +
   `warn!`, never synthesize (preserves a sharp invariant so producer bugs aren't
   masked). Tier-1 keys (no manifest entry) are structurally un-synthesizable.

Confirmed airtight by codex review + file:line evidence:

- **Spurious/repeated clear (codex #1, #3):** edge-triggered via producer-local
  `drain_prev_nonempty`; fires exactly once because `record_emitted_for_manifest`
  advances `last_emitted` for the full key universe (§10.3). No change to
  `record_emitted` needed.
- **Rev on the synthesized row (codex #2):** carry `manifest.rev` (advanced on the
  edge). One row per key per frame (the `present` skip), so no same-frame `Changed`
  can be shadowed by a synthesized `Cleared`. Host clear reorder-guard recommended
  for R3-S3 as future-async belt (§10.5 finding 4).

### 10.8 Sub-step ladder (Sonnet-codeable, each with its own gate)

| Step | Crate | Change | Gate |
|---|---|---|---|
| **S1b-a** | `nmp-core` | Cleared edge machine for `action_stages`/`action_lifecycle`: add `note_copy_emit` + edge map in `projection_rev/mod.rs`; call it once per emit in `action_stages_projection` / `action_lifecycle_projection` (`publish_cmd.rs`). | `-p nmp-core`: unit test in `projection_rev/tests_unit.rs` — non-empty→empty edge yields `Cleared`, stably-empty yields `Unchanged`, ack-of-last-entry yields `Cleared`. |
| **S1b-b** | `nmp-core` | Cleared-synthesis inverse pass in `rung3_omit::omit_unchanged` + the narrowed predicate + the `CONDITIONAL_EMPTY_MEANS_CLEAR` const + the `debug_assert!`/`warn!` on the bad-invariant case. | `-p nmp-core`: extend `rung3_omit::tests` — manifest-Cleared-absent ⇒ synthesized Cleared row; manifest-Changed-absent (conditional key) ⇒ synthesized Cleared; manifest-Changed-absent (Tier-2 unconditional) ⇒ NOT synthesized + assert fires in debug; present key never double-emitted. |
| **S1b-c** | `nmp-core` (+ `nmp-testing`) | The full-path regression test (§10.6) incl. the simulated host cache-merge stand-in. | `-p nmp-core`: the §10.6 test — **fails on master HEAD `c6f3486f5`, passes after S1b-a+b**. |
| **S1b-d** | `nmp-ffi` (+ `ios` header) | Finding 5: `declare_incremental_apply` → `Result`/return-code; update `AppHost` trait + impl + C-ABI + `NmpCore.h` + harness callers. No compat shim. | `-p nmp-ffi` + `-p nmp-core`; doctrine-lint smoke; header regenerated; S6 harness call checks the code. |
| **S1b-e** | `nmp-core` | Finding 6: coalesce the two per-tick `snapshot_projections` locks into one `incremental_apply_state()` acquisition in `make_update`. | `-p nmp-core`: existing tick tests green; (optional) a lock-count probe asserts one acquisition per tick. |

Always also run `cargo test -p nmp-testing --test doctrine_lint_smoke` (memory:
agent test scope).

### 10.9 File-size pre-plan (300 warn / 500 hard)

- `projection_rev/mod.rs` is **453 LOC** — S1b-a adds `note_copy_emit` + an edge map +
  doc (~40-60 LOC) → would cross 500. **Pre-plan extraction:** move the
  `note_drain_emit` + `note_copy_emit` + `presence_for` + edge-map cluster into a new
  sibling **`projection_rev/presence.rs`** (a zero-behavior refactor as S1b-a's
  opening commit), dropping `mod.rs` well under 300 and giving the new machine room.
- `rung3_omit.rs` is **242 LOC** — S1b-b's inverse pass + predicate + tests (~80 LOC)
  → ~320, crossing the 300 *warn*. **Pre-plan:** keep the production transform in
  `rung3_omit.rs` and move the `#[cfg(test)] mod tests` into a sibling
  **`rung3_omit/tests.rs`** (or `rung3_omit_tests.rs`) so the production file stays
  ~140 LOC and the test file carries the new cases. (Tests are not codegen-exempt, so
  this split is required, not optional.)
- `publish_cmd.rs` is **406 LOC** — S1b-a adds two one-line `note_copy_emit` calls;
  no extraction needed.
- `update.rs` is **430 LOC** — S1b-e is net-neutral (two locks → one call); no
  extraction needed.
- `nmp-ffi/src/lib.rs` (~1572) — S1b-d swaps `debug_assert!` for a `Result` return;
  net-neutral. `snapshot.rs` return-type change is net-neutral.

Run the gate exactly as memory dictates: `check-file-size.sh --from-ref
origin/master --to-ref HEAD --baseline-ref origin/master` (NOT `--changed-only`).
Never bump the baseline; split instead.

### 10.10 Summary of the design call (the auditor's deferred decision)

**Cleared = an explicit payload-less typed row (`state = Cleared`), synthesized by
`omit_unchanged` from the manifest for conditional keys absent from `typed` —
consistent with §3 D3-1.** NOT a manifest-only tombstone (which would require the host
to also read the manifest, re-introducing the per-frame manifest D3-4 deliberately
removed). The manifest already carries the Cleared presence for drain keys (§10.1);
S1b makes the copy-with-TTL keys carry it too (§10.4) and makes the transform act on
it (§10.2). This keeps the host's apply path a pure cache-merge over the typed row
set — no second wire surface — which is the D3-3/D3-4 invariant.
