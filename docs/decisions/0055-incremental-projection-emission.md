# ADR-0055 — Incremental projection emission (per-projection revision transport)

- **Status:** Proposed (2026-06-13); renumbered from design/0053 (2026-06-14)
- **Supersedes / amends:** aim.md §10 ("State crosses FFI as a full `Clone`d
  snapshot by default; granular updates are an optimization, not a default") and
  aim.md Doctrine #12 ("Snapshots by default, granular updates as optimization").
  This ADR inverts that default for the projection set: the transport becomes
  **incremental by default, with a full snapshot as the resync fallback** — the
  full snapshot stops being the steady-state shape and becomes the cold-start /
  gap-recovery shape.
- **Relates to / composes with:**
  - ADR-0037 (typed FlatBuffers sidecar) — this ADR adds a revision/state field
    to the existing `TypedProjection` carrier; it does not change the sidecar's
    typing model.
  - ADR-0044 (Tier-3 snapshot envelope typing) — the kernel-owned built-ins
    (metrics, relay_status, …) are Tier-3 envelope fields; this ADR brings them
    under the same per-projection rev contract as the sidecar projections.
  - ADR-0053 (host-declared projection subscriptions) — that work decides
    **which** projections a host consumes; this ADR decides **how much of each**
    ships per tick. They compose multiplicatively: interest narrows the *set*,
    rev-gating narrows the *per-tick churn within the set*.
  - In-flight perf PRs: `fix/relay-diagnostics-raw-timestamps` (#1332, merged —
    removes the Rust-side "Xs ago" formatting that would otherwise poison
    change-detection) and `perf/drop-flatbuffers-verifier-on-trusted-decode`
    (cuts host decode cost; orthogonal and complementary).
- **Prior art already on master:** `SnapshotRegistry::register_gated` +
  `ChangeGate` (`crates/nmp-core/src/kernel/snapshot_registry.rs`,
  `.../snapshot_registry/entry.rs`). This ADR generalizes that idea from
  *closure memoization* (skip re-running the host closure) to a *transport
  contract* (skip re-serializing AND re-emitting the unchanged projection,
  host reuses its prior decoded value), and extends it to the typed sidecar +
  kernel built-ins (the actual hot path).

---

## Context

### The defect (owner verdict: "completely stupid and unacceptable")

`Kernel::make_update` (`crates/nmp-core/src/kernel/update.rs`) runs at up to
`DEFAULT_EMIT_HZ = 4` and fires on essentially every accepted relay event. On
every tick it:

1. Builds the **entire** `KernelSnapshot` struct, including
   `snapshot_projections_with_publish_cluster()` which unconditionally computes
   *all 18 built-in projection keys* (`profile`, `accounts`, `claimed_profiles`,
   `relay_diagnostics`, `resolved_profiles`, …) — see
   `crates/nmp-core/src/kernel/update/projections.rs`.
2. Runs every host-registered typed projection (`run_typed_projections`) and
   merges the kernel-owned built-in typed sidecars
   (`merge_builtin_typed_projections`).
3. Serializes **the whole set** to FlatBuffers via
   `encode_snapshot_with_envelope`.

The cost is **O(total-state) per tick, not O(change)**. The host then decodes
the whole frame and re-diffs it through its UI graph (SwiftUI AttributeGraph /
Compose), reassigning every `@Published` slot each frame and triggering broad
invalidation regardless of what actually changed. On-device Time Profiler
confirms the serialize → decode → re-diff cycle dominates CPU.

### Why the full-snapshot model was chosen (the constraint we must keep)

The full snapshot has one genuine virtue, and it is load-bearing: **host state
is a pure, self-healing function of the stream.** Every tick carries the
complete truth, so a dropped, delayed, reordered, or misapplied frame cannot
permanently desync the UI from the kernel — the next frame heals it
automatically, with no detection logic and no recovery protocol. This is the
correctness invariant any replacement MUST preserve. The cost we are killing
must not take this safety with it.

### What already exists (do not reinvent)

- **Generic-JSON change-gating** (`register_gated`): a host passes an
  `Arc<AtomicU64>` rev; the registry memoizes `(gate_value, last_value)` and
  skips re-invoking the closure when the gate is unchanged. **Limitation 1:** it
  is opt-in (default registration is always-run). **Limitation 2 (the big one):**
  it only skips the *closure*; the memoized value is still inserted into the
  snapshot map and **still serialized + emitted every tick**. The host still
  decodes it and still re-assigns it. So today's gating saves projection-closure
  CPU but **not** transport serialization, **not** wire bytes, and **not**
  host-apply churn — which is where the profiler says the cost is.
- **Typed sidecar** (ADR-0037): `TypedProjection { key, TypedPayload{ schema_id,
  schema_version, file_identifier, payload:[ubyte] } }`. The hot-path projections
  (feed, profile clusters) ride here, and **none of them are gated at all.**

So the part that is gated is cold; the part that is hot is ungated. That is the
exact inversion this ADR corrects.

---

## Research synthesis (how other systems navigate the same tension)

Five parallel research investigations were run against the SOTA. The complete
per-investigation digests + sources are in the final report; the load-bearing
conclusions are:

### 1. Local-first / sync engines (Replicache, ElectricSQL, Linear, Figma)

- **Replicache** ships a per-client-view diff via an opaque *cookie* + a Client
  View Record (`{key → version}`); on cookie loss it emits `{op: clear}` + full
  repopulate — a self-healing fallback. **NMP steal:** the `clear` fallback maps
  to NMP's epoch baseline; NMP needs *no* mutation rebase (no client writes).
- **Linear** uses one **globally monotonic sync-id**: bootstrap (full) then
  delta packets; on reconnect the client compares its `lastSyncId` to the
  server's and either catches up or re-bootstraps. **NMP steal:** the
  monotonic-rev + reconnect-comparison = NMP's epoch/rev gap detection. Linear's
  weakness ("delta carries full model state") is exactly NMP's current bug —
  per-projection rev fixes it.
- **Figma** does **property-level LWW** with a journal (fine-grained ops) +
  periodic checkpoints (full state); recovery replays journal from last
  checkpoint. **NMP steal:** the journal/checkpoint split = "deltas in steady
  state, baseline on reset" — but Figma's LWW conflict machinery is unneeded
  (one writer).
- **ElectricSQL** = append-only shape log + offset cursor; `409 must-refetch` on
  invalidation. **NMP steal:** schema-version in the frame → host detects stale
  decoder → resync.
- Common thread: **monotonic position token + baseline-then-delta + explicit
  resync on gap.** Every mature system converges on this shape.

### 2. CRDTs / op-logs (Automerge, Yjs, delta-state theory)

- The verdict is **emphatic**: state-vectors / op-logs / bloom-filter sync are
  **overkill for NMP**. They solve multi-writer convergence under unreliable
  networks with no central authority — NMP has *one producer, one consumer,
  in-process, trusted, derived views*. NMP's state vector "collapses to a single
  `u64` revision."
- The one transferable insight: **ship state, not ops.** A state-carrying
  `Changed` frame is idempotent and order-independent — a missed frame is
  *superseded*, not lost. This is why NMP needs no causal ordering, no
  tombstones, no delta buffer + ACK. And NMP's full-snapshot fallback — expensive
  in a real CRDT — is **free** here because the kernel always holds ground truth.

### 3. UI-runtime state bridges (React, RN bridge→JSI, SwiftUI/Compose, signals)

- **The core analogy:** NMP's Rust→FlatBuffers→native FFI is React Native's old
  JSON **bridge**; the documented fix (JSI/Fabric) was *stop serializing what
  didn't change* and share typed refs (≈100× on round-trips). NMP cannot adopt
  JSI literally (different boundary) but the lesson is identical: **shrink what
  crosses.**
- **Verdict on the two levers:** (a) shrink-what-crosses (gating/deltas) has
  **primary** leverage — FlatBuffers serialize is synchronous on the actor
  thread, cannot be patched incrementally, and burns kernel cycles even on quiet
  ticks; (b) shrink-host-diff (per-projection reactive slots) is **multiplicative**
  but only pays off *after* (a). This validates the rung order: producer gating
  first, rev-aware host apply second.
- **Critical host caveat (feeds D7):** SwiftUI/Compose already do fine-grained
  invalidation internally — but only if the host exposes each projection as a
  *separate* observable. A single `ObservableObject` + `objectWillChange`
  invalidates everything on any change, defeating the win. **The iOS host must
  use `@Observable` (iOS 17+) per-projection slots**, not one coarse object.
- **Tantalus** (a real Rust→Swift delta-state system over UniFFI — a near-exact
  NMP analog) is the existence proof for D7/Rung 6: a generated per-field-change
  enum, applied host-side via `switch` into `@Observable` properties, no
  host-side re-diff at all.

### 4. DB change feeds / CDC (Postgres logical repl, Firestore, CDC, IVM)

- **Postgres logical replication:** initial snapshot, then stream changes since
  an LSN; a subscriber that falls behind resumes from its confirmed LSN, or — if
  the slot is invalidated — does a full resync. This is precisely NMP's
  "baseline on (re)connect, deltas from rev, full resync on epoch gap."
- **Firestore `docChanges()`** is the closest precedent of all: the first
  listener callback is a full snapshot, subsequent callbacks carry per-document
  added/modified/removed deltas; reconnection re-syncs. This is the template for
  the deferred **Rung 6** intra-projection row deltas (added/modified/removed by
  stable row key).
- **Incremental view maintenance:** computing a view delta from base-table deltas
  beats full recompute only when the view is large and the change is small —
  which is exactly why NMP defers row-deltas to the *feed* and not to scalar
  projections.

### 5. Versioned snapshots / structural sharing / content-addressed diffing

- **The crux finding (backs D2):** content-hashing a projection to detect change
  is **O(n)** and "can cost as much as serializing it" — XXH3 at ~31 GB/s on a
  10 KB projection ≈ 0.3 µs/tick × N projections, vs an **O(1)** rev-counter
  compare at ~1–2 ns. The rev counter is ~600× cheaper per tick and the gap
  widens with projection size. **Reject content-hash as the default gate.**
- **Who owns the rev?** The research independently flags that the in-flight
  `register_gated` design has the **host** bump the `Arc<AtomicU64>` — which is
  *inverted*: the host only knows when it *serialized*, not when the kernel
  *mutated*. For the kernel to skip serialization the **kernel** must own the
  bump at the mutation site (the Bevy ECS `Mut<T>` auto-bump / Linux seqlock /
  HTTP ETag model). This directly drives D2 and the corresponding Rejected
  Alternative.
- **ETag / HTTP-304** is the validated protocol analog: per-key validator +
  "Not Modified ⇒ reuse cached copy" = omit-unchanged-key + host-reuses-buffer.
- **Structural sharing (`im`/Arc::ptr_eq)** is the *ideal* long-term mechanism
  (O(1) pointer compare, no manual bump discipline) but requires rewriting
  projections as persistent structures — deferred; the rev counter is the right
  immediate play.

---

## Decision

### D1. Per-projection monotonic revision is the unit of change

Every projection (built-in typed, host typed sidecar, and generic JSON) gains a
**monotonic `projection_rev: u64`** owned by the **kernel** (not the host — see
Rejected Alternatives). The kernel bumps a projection's rev **only** when that
projection's *source state* mutates. The transport contract becomes:

> A `TypedProjection` (and its generic-JSON analog) carries `{ key,
> projection_rev, state, payload? }`. The host keys its decoded value cache by
> `key` and remembers the last `projection_rev` it applied. On each frame:
> - `state = Changed` + `payload` present → decode, apply, store
>   `(key → projection_rev)`.
> - `state = Unchanged` (payload omitted) → the host **reuses its prior decoded
>   value**; it does not decode, does not re-assign its UI slot.
> - `state = Cleared` (payload omitted, explicit) → the host drops the value
>   (the projection went absent — e.g. a view closed). **Cleared is a distinct
>   state, never conflated with Unchanged or with omission.**
> - **Key entirely absent from the frame** → also "unchanged at last rev" — see
>   D3 for why absence and `Unchanged` must mean the same thing and how a
>   missing key can never be read as "cleared".

This is the ETag / HTTP-304 pattern (per-key validator + "Not Modified ⇒ reuse
cached copy") and the Firestore `docChanges()` pattern (first callback = full
set, then per-item added/modified/removed) translated to NMP's FFI push.

### D2. Change detection is O(1), driven by mutation sites — NOT content hashing

The kernel bumps `projection_rev` at the **mutation site** that dirties the
projection's source (the same places that already set `changed_since_emit`). A
projection's rev advancing is a cheap counter increment; an unchanged projection
costs a single `u64` compare on the emit path. We explicitly REJECT
content-hashing the projection to detect change: hashing a multi-MB projection
every tick can cost as much as serializing it, defeating the purpose (see
Rejected Alternatives → "content-hash gate").

Mechanically the kernel maintains a `projection_revs: HashMap<&'static str, u64>`
(or a small typed struct of named counters for the 18 built-ins) and a
`dirty: HashSet<&'static str>` cleared each emit. `make_update` consults the
rev to decide `Changed` vs `Unchanged` per key and only serializes the `Changed`
ones.

### D3. The correctness invariant: self-healing is preserved by carrying the rev

The full-snapshot model self-heals because every frame carries complete truth.
The incremental model preserves the **same** property through a different
mechanism: **every frame carries, for every changed key, the authoritative
current `projection_rev`**, and the host's applied-rev is monotonic. Therefore:

- **Dropped frame:** the next frame that touches the key carries a *higher* rev
  with the *current* payload. The host applies the latest state — it never
  applies a stale intermediate. (Unlike a CRDT op-log, NMP ships **state, not
  ops**: a `Changed` frame is the *whole current value of that projection*, not
  a delta to be replayed in order. So a missed `Changed` frame is simply
  superseded, not lost. This is why NMP does **not** need state vectors / op
  ordering — see the CRDT research digest.)
- **Reordered frames:** the host ignores any frame whose `projection_rev ≤` the
  rev it already applied for that key (per-key monotonic guard). Last-rev-wins.
- **Gap / desync detection:** the kernel also stamps a global monotonic
  `snapshot_epoch` (D4). If the host ever observes an epoch change it does not
  recognize, or fails to decode a `Changed` payload, it requests a fresh
  baseline (D5) — a cheap in-process FFI call, since there is exactly one
  trusted producer.
- **Absence == Unchanged, never Cleared:** a key omitted from a frame means
  "unchanged at your last applied rev." Clearing is **always explicit**
  (`state = Cleared`). This removes the classic delta-protocol footgun where
  "I didn't send it" is ambiguous with "it's gone." The host can therefore never
  silently lose a projection because a frame happened not to mention it.

### D4. Epoch + session_id reset the baseline on cold-start / account-switch / schema change

**DECIDED (fork 3 + fork 1):** ONE global `snapshot_epoch` (not per-projection
epochs — account-switch/restart invalidate everything anyway; per-key epochs are
unjustified complexity). `session_id` reuses an existing init-time value (the
kernel-start wall-clock stamp from `TimingMilestones::started_unix_ms`) rather
than adding new state — any value that differs across process restarts suffices.

Two frame-level identity fields work together:

- **`session_id: u64`** — the kernel-start wall-clock stamp
  (`TimingMilestones::started_unix_ms`). It identifies "this run of the kernel."
  It does NOT increase monotonically; it just *differs* across restarts. This is
  the Firestore-listener / CDC fix for the restart-ambiguity footgun: after a
  kernel restart the per-projection revs reset to 0, and a host that only
  compared revs could mistake a restarted kernel's low rev for a stale/duplicate
  frame. A changed `session_id` tells the host unambiguously "this is a new run
  — discard all cached revs and re-baseline," with no reliance on epoch
  monotonicity surviving a process restart.
- **`snapshot_epoch: u64`** — a *within-session* monotonic counter bumped on any
  event that invalidates the host's entire cached projection set **without** a
  process restart:
  - account switch / sign-out (the projection set's *meaning* changes wholesale),
  - `KERNEL_SCHEMA_VERSION` change (host compiled against a different shape),
  - an explicit host resync request (D5).

On a `session_id` change **or** an `snapshot_epoch` bump the **next frame is a
full baseline**: every live projection is emitted with `state = Changed`
regardless of its rev, so the host re-seeds its cache from scratch. The host
resets its per-key applied-rev map whenever either the `session_id` or the
`snapshot_epoch` it sees differs from what it last applied. This is Postgres
logical replication's "initial snapshot, then stream from LSN" / Debezium's
"snapshot phase then streaming phase" / Firestore's "first callback = full
snapshot, `RESET` ⇒ re-deliver all" pattern: **baseline on (re)connect, deltas
in steady state.**

### D5. Host-requested resync is the universal escape hatch

Because there is exactly one trusted, in-process producer, "give me a fresh
baseline" is a single FFI call (`nmp_app_request_full_snapshot` or equivalent),
not a network round-trip. The host calls it on: first attach, decode failure,
unrecognized epoch, or any host-side invariant violation. The kernel responds by
bumping the epoch (D4), forcing a full baseline next tick. This makes the model
**unconditionally self-healing**: the worst case for any conceivable
delta/gating bug is one extra full snapshot, never a permanent desync.

### D6. Drain-on-emit projections keep their existing semantics, expressed in the state machine

`action_results`, `signed_events` (true drains), and `action_stages` /
`action_lifecycle` (copy-with-TTL) are special: they are *present only on ticks
where something settled*. Under this ADR:

- A drain projection that has content this tick → `state = Changed` (rev bumped).
- A drain projection that is empty this tick → it is **omitted** (absence ==
  unchanged), and because the host *consumes* drain entries on apply, "unchanged"
  correctly means "nothing new to consume." These projections must NOT be modeled
  as a long-lived cached value the host reuses — they are append/consume streams.
  The state machine handles this naturally: each non-empty tick is a `Changed`
  with a fresh rev carrying *only the newly-settled entries*; the host applies
  (consumes) and never "reuses prior buffer" for these keys. This is the one
  place where intra-projection append semantics already exist and must be
  preserved verbatim.

### D7. Host-side apply becomes rev-aware (the other half of the win)

Stopping the kernel from re-serializing is necessary but not sufficient: today
the host re-assigns every `@Published` slot every frame (KernelModel.swift /
KernelBridge.swift), so even an unchanged projection triggers SwiftUI
invalidation. Under this ADR the host **only re-assigns the slot whose
`projection_rev` advanced** (or whose key arrived `Changed`). Unchanged/omitted
keys are skipped entirely — no decode, no assignment, no invalidation. This is
the fine-grained-reactivity insight (signals / SolidJS): feed change at the
granularity the UI subscribes to, so the framework recomputes only the changed
subtree instead of re-diffing the whole frame.

### D8. Verdict — per-projection rev-gating now; intra-projection row deltas later, only where proven

**DECIDED (fork 4):** Feed row-deltas are deferred-to-measurement, not
deferred-indefinitely. Once Rung 3 is in production and the per-projection
rev-gating is measured, if the feed projection remains a dominant cost it
graduates to row-deltas in a follow-up ADR. The measurement gate (Rung 0) is
explicitly scoped to produce the empirical evidence that decides this.

**This ADR commits to per-projection revision transport (O(changed
projections)) as the floor, and makes it the default, not an opt-in.** It does
**not** commit to intra-projection row deltas (added/modified/removed rows
within a list projection, Firestore `docChanges`-style) in the first iteration,
for these reasons:

1. Per-projection gating captures the dominant win at a fraction of the
   complexity and risk. Most projections are *scalars or small maps* (profile,
   active_account, settings_hub, configured_relays); for these, "send the whole
   projection when it changes" is already optimal — a row-delta layer would be
   pure overhead.
2. Row deltas reintroduce exactly the ordering/causality fragility the
   full-snapshot model was chosen to avoid. They are only worth it for a *large,
   high-frequency, append-dominated* projection — in NMP that is **the feed**
   (and the feed is `nmp-feed`'s viewport projection, ADR-0033, which already has
   cursor/viewport mechanics that are the natural home for row deltas).
3. The clean composition: **rev-gating is the universal transport contract;
   row-delta is an optional per-projection encoding *inside* a `Changed` frame**,
   negotiated per schema. A projection can later opt into "my `Changed` payload
   is a row-delta against rev N-1, with a full-row fallback when the host's rev
   is stale or absent." That is a strictly additive evolution that does not touch
   the rev contract. We specify the *hook* now and defer the *feed row-delta
   implementation* to a follow-up ADR once the rev floor is measured in
   production.

> Recommendation to the reviewer: ship the rev floor (kill the O(total) cost,
> make incremental the default, preserve self-healing), instrument it, and let
> the production feed profile decide whether the feed graduates to row deltas.
> Do not block the floor on the delta design.

---

## Rejected alternatives

- **Keep the full snapshot, just gate the host closures (status quo of
  `register_gated`).** Rejected as insufficient: it saves closure CPU but still
  serializes, emits, decodes, and re-assigns every projection every tick — the
  profiler-dominant costs are untouched. We *build on* its `ChangeGate` idea but
  push it to the transport.
- **Content-hash each projection to detect change.** Rejected: hashing a
  multi-MB projection is O(n) and can cost as much as serializing it, so it does
  not reduce the dominant cost; it only moves it. A mutation-site rev counter is
  O(1). (If a projection's source is genuinely cheap to hash and hard to
  instrument, a hash *may* back its `ChangeGate` — but it is not the default
  mechanism.)
- **Host-owned rev (the current `register_gated` design where the host bumps the
  `Arc<AtomicU64>`).** Rejected as the model for built-ins: it splits the
  source-of-truth (the kernel mutates the state but the host must remember to
  bump the rev), which is fragile and violates "the kernel owns truth." Kernel
  mutation sites own the rev. (The host-owned gate remains valid for purely
  host-derived projections that the kernel cannot see — it is the right tool for
  that narrow case only.)
- **Full op-log / CRDT-style delta sync with state vectors.** Rejected as
  over-engineered for NMP: projections are *derived, recomputable* views and the
  kernel always holds ground truth, so convergence is trivial (resend state).
  State vectors solve multi-writer reconciliation NMP does not have (one
  producer, one consumer, trusted, in-process). We borrow CRDTs' *insight*
  (ship state not ops ⇒ a missed update is superseded, not lost) without their
  machinery.
- **Remove the FFI serialization boundary (à la React Native JSI/Fabric).**
  Out of scope and not available: Rust↔Swift/Kotlin is a real ABI boundary. The
  correct response to an unavoidable serialization boundary is to *shrink what
  crosses it* (this ADR) — which is precisely what JSI's motivation teaches.
- **ONE global `snapshot_epoch` vs. per-projection epochs.** Decided: one global.
  Per-projection epochs are unjustified complexity — account-switch/restart
  invalidate everything anyway, so a per-key epoch adds protocol surface with
  no benefit.
- **`session_id` as new random `u64` vs. reusing existing init-time stamp.**
  Decided: reuse `TimingMilestones::started_unix_ms`. Any value that differs
  across process restarts suffices; adding new state is unjustified.

---

## Open questions — DECIDED

All four forks from the design review have owner decisions recorded:

1. **Frame-level vs key-level epoch.** DECIDED: ONE global `snapshot_epoch` (see
   D4). Per-projection epochs add complexity without benefit; account-switch and
   restart invalidate the whole set anyway.

2. **`state` encoding: 3-valued vs 2-valued + omission.** DECIDED: omission ==
   Unchanged on the wire (saves bytes); `Cleared` is explicit; the wire enum
   carries only `Changed`/`Cleared`. Absence is the third state. See D3.

3. **`session_id` representation.** DECIDED: reuse the existing kernel-start
   wall-clock stamp (`TimingMilestones::started_unix_ms`) rather than adding new
   state. See D4.

4. **Feed row-delta home.** DECIDED: deferred-to-measurement (not
   deferred-indefinitely). The Rung 0 measurement gate produces the evidence. If
   the feed remains dominant post-Rung-3, it graduates to row-deltas in a
   follow-up ADR targetting `nmp-feed` (ADR-0033) viewport mechanics. See D8.

---

## Consequences

- aim.md §10 + Doctrine #12 are amended (incremental-by-default for projections;
  full snapshot becomes the baseline/resync shape). A doc PR updates both.
- The wire schema gains `projection_rev` + `state` on the projection carrier and
  `snapshot_epoch` on the frame (appended at the tail — old readers ignore).
- Hosts must become rev-aware to realize the host-side win, but a host that
  ignores the new fields and treats every `Changed` as authoritative still works
  (graceful degradation): correctness never depends on the host honoring
  "unchanged," only performance does.
- New invariant tests: drop/reorder/gap/epoch-reset all converge to the
  full-snapshot result (a property test asserting "incremental stream applied ==
  full snapshot of final state" is the core correctness gate).

---

## Appendix — research provenance

This ADR is backed by five parallel SOTA investigations (local-first sync
engines; CRDTs/op-logs; UI-runtime state bridges; DB change feeds/CDC;
versioned-snapshot/structural-sharing). The condensed digests are in the
"Research synthesis" section above; the full per-investigation findings with all
source URLs were delivered in the design report accompanying this ADR. The
closest single precedent is **Firestore `docChanges()`** (first callback = full
snapshot, then per-item added/modified/removed, `RESET` ⇒ re-baseline); the
sharpest mechanism warning is that **content-hashing to detect change is O(n)
and self-defeating** (use an O(1) mutation-site rev); and the strongest
existence proof for the host side is **Tantalus** (Rust→Swift delta-state over
UniFFI applied into `@Observable` slots).
