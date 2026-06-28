# ADR-0057 — Unified kind-agnostic accepted-event ingest chokepoint: persistence ≠ admission ≠ projection

- **Status:** Implemented. The kind-agnostic chokepoint is live:
  `ingest_accepted_event` / `project_accepted_event` / `verify_and_persist`
  (`crates/nmp-core/src/kernel/ingest/`), with `record_local_publish_intent` /
  `local_publish_intent.rs` / `pre_kind3_buffer` deleted; coverage in
  `crates/nmp-core/src/kernel/chokepoint_tests.rs` and
  `contacts_chokepoint_pr3_tests.rs`. App-visible read ownership is amended by
  ADR-0070.
- **Date:** 2026-06-15
- **Issues:** #1440 (ghost-post — no optimistic local echo for non-replaceable
  kinds), #1442 (persistence entangled with relevance — the authoritative store
  has relevance-shaped holes), #1443/#1480 (production durable retention keeps
  valid fetched events by default; finite durable LRU is explicit policy only).
- **Decision record:** this ADR is the durable, self-contained authority for the
  ingest-chokepoint architecture. Durable "why" lives here and in the issues above.
- **Amends / supersedes:**
  - **Amends ADR-0042** — its §5.1 "Remaining kernel work" and §6 frame
    `should_store_event` as a **store-admission** gate (generalised so an event is
    *stored* when it matches an active interest). This ADR demotes
    `should_store_event` to a **read-time projection predicate** with no power over
    persistence; the ADR-0042 admission framing is withdrawn.
  - **Sets the unified ingest design** — one kind-agnostic chokepoint handles
    accepted events from relay and local-publish sources.
  - **Extends ADR-0045** — the single always-on cache-serve mechanism. ADR-0045's
    "one mechanism, replay feeds the post-store seam not `store.insert`" principle
    is the read-half precedent this ADR generalises to *all* event sources.
- **Related:** ADR-0053 (host-declared projections — observers self-gate by
  registration), crate-boundaries.md §4.2 (the `IngestParser` migration this
  finishes), ADR-0064 and the D26 doctrine gate for the separate signer authority
  and action/projection lifecycle problem domain.

**Current disposition:** the accepted-event chokepoint remains the persistence
and ingest authority. ADR-0070 moves app-visible read lifecycles to typed read
sessions; it does not reopen kind-specific ingest ladders or public filterless
event observers.

---

## Context

### The problem: ingest is split by source AND by kind, and three concerns are fused

Event ingestion is split **by source** (relay vs local publish vs cache-replay)
**and by kind** (per-kind `match` arms), and within each arm three distinct
concerns — **persistence**, **admission/relevance**, and **projection mutation**
— are fused. The fused logic is then duplicated across two hand-maintained
per-kind ladders that drift:

- **Relay ladder:** `Kernel::handle_event` (`crates/nmp-core/src/kernel/ingest/mod.rs:245`)
  does relay bookkeeping (`:257-280`) then a `match event.kind` (`:296-425`) with
  bespoke arms for kind:0 (`ingest_profile`), kind:3 (`ingest_contacts`), kind:1|6
  (`ingest_timeline_event`), and a wildcard arm for everything else.
- **Local ladder:** `Kernel::record_local_publish_intent`
  (`crates/nmp-core/src/kernel/local_publish_intent.rs:9`), reached from
  `publish_engine.rs:169`, mirrors those arms (`record_local_profile_intent`,
  `record_local_contacts_intent`, `record_local_replaceable_intent`) — a parallel
  ladder that exists only to give locally-published replaceables read-your-writes.

### Confirmed code facts (verified in source, 2026-06-15)

- `verify_and_persist` (`ingest/mod.rs:436`) **already** does, kind-agnostically:
  sig-verify → `store.insert` → raw-tap fan-out → `EventIngestDispatcher.dispatch`
  to the NIP `IngestParser`s (gated `Inserted | Replaced | Ephemeral`, `:486-499`)
  → replaceable TTL stamping. **But it does NOT fire `notify_event_observers`** —
  every caller does that per-arm.
- **kind:1 / kind:6 bypass `verify_and_persist` entirely** and run a self-contained
  `ingest_timeline_event` (`ingest/timeline.rs:18`), which independently does sig-
  verify + `store.insert`, the admission gate, pre-kind:3 parking, the timeline
  read-cache append, and the created_at clamp.
- **The clearest layering bug:** `should_store_event` (`ingest/timeline.rs:25`, def
  `:299`) is a *timeline-relevance* predicate — its primary clause is
  `timeline_authors.contains(author)` (the follow set) — and it runs **before**
  `store.insert`, so it gates **persistence**. A self-authored note fails it (you
  are not in your own follow set); a non-followed author's reply is dropped unless
  an escape-hatch clause (`matches_active_open_interest`, `:328`) matches.
- **The asymmetry (the #1442 core):** kind:1/6 are the **only** kinds whose
  *persistence* is relevance-gated. Every other kind (0, 3, 7, 10002, …) persists
  on valid-signature alone via `verify_and_persist`.
- **Ephemeral (20000–29999) is already correctly excluded from PERSISTENCE at the
  store layer** (`nmp-store` `is_ephemeral()`, returning `InsertOutcome::Ephemeral`).
  This stays at the store layer; no ingest-layer persistence check is added.
- **LATENT BUG (this ADR fixes it):** ephemeral events do **not** reach app
  observers today. `verify_and_persist` dispatches to NIP parsers on
  `Inserted | Replaced | Ephemeral` (`ingest/mod.rs:486-490`), but the wildcard
  arm's `notify_event_observers` fires only on `Inserted | Replaced`
  (`ingest/mod.rs:389-404`). So an ephemeral reaches the NIP parser registry but
  **not** the app-facing `ObservedProjectionSink` seam — apps cannot react to
  ephemeral events they never store.
- Two stores exist: the authoritative `EventStore` (`self.store`) vs the in-memory
  read-caches (`self.profiles`, `self.events`, `self.timeline`). The volatile
  cache's *follow-set relevance policy* is punching holes in the durable store —
  inverted authority. Because the NIP parsers' derived caches are **rebuilt from
  the store on restart**, a relevance-shaped hole in the store becomes a permanent
  hole in every projection (the mechanism behind past "missing DMs/replies"
  findings).

### Why this is the same bug as #1440 and #1442

#1440 (ghost post: a locally-published note shows no optimistic echo) and #1442
(authoritative store has relevance-shaped holes) are the **same root cause** —
persistence, admission, and projection fused per-kind per-source — and share **one
fix**: separate the three concerns into three layers behind one chokepoint.

---

## Decision

The two per-kind ingest ladders were replaced with **one kind-agnostic,
source-agnostic accepted-event chokepoint**. The three fused concerns became three
distinct layers:

### 1. Admission to the chokepoint = valid signature. Nothing else.

**Admission** is what is allowed to reach `store.insert`: **a valid signature,
full stop** — not acquisition-matched, not relevance-gated, not kind-gated. The
acquisition-match concept is **deleted, not generalised** (owner decision
2026-06-15, plan Q5):

- Signatures already prevent forgery.
- Projections filter at **read time**, so an unsolicited event is invisible to the
  UI anyway.
- The check is incoherent today: only kind:1/6 carry it; kind:0/3/7/10002 already
  persist on valid-sig alone.
- The only thing acquisition-match could buy is DoS / write-amplification defense
  against a hostile relay signing garbage under throwaway keys. That is a
  **transport-layer concern** (a per-relay rate-limit / quota), **not** an ingest
  gate, and GC + pinning already bound the damage (spam is cold/unpinned → evicted
  first; live events are pinned). If DoS ever becomes real, add a transport quota
  at that layer.

All three sources collapse to the same admission rule: **relay** — sig-verified at
the chokepoint; **local publish** — the publish engine already accepted it;
**replay** — already in the store.

> **Ephemeral-ness is NOT an admission criterion.** Every validly-signed event,
> ephemeral or not, is admitted to the chokepoint. Ephemeral-ness is resolved by
> the *store outcome* (below), not by an admission check.

### 2. Delivery vs persistence — gated by the store OUTCOME, not just the signature

Admission gets an event *to* `store.insert`. What happens next is gated by the
**`InsertOutcome`** the store returns — the signature is necessary but not
sufficient. The canonical outcome enum (the by-kind table at
`docs/builder-guide/08-eventstore.md:51`, "What happens on insert, by kind"; enum
def `crates/nmp-store/src/types/outcomes.rs:11-33`) is
`Inserted | Replaced | Superseded | Duplicate | Tombstoned | Rejected | Ephemeral`.

- **Persistence** = the non-ephemeral canonical subset, `Inserted | Replaced`.
  An ephemeral (20000–29999) is dropped at the **store layer** and returns
  `InsertOutcome::Ephemeral` — it is never written. `Superseded | Duplicate |
  Tombstoned | Rejected` are likewise not new canonical writes. Persistence stays
  exactly where it is, at the store layer.
- **Delivery** = both `EventIngestDispatcher.dispatch` (to the NIP `IngestParser`s)
  **and** the internal observed-projection sink slot, fired on the **canonical
  accepted store outcome `Inserted | Replaced | Ephemeral`** — NOT on
  `Duplicate | Superseded | Tombstoned | Rejected`. The sink slot is not a public
  all-event app primitive: production app/product read models receive future
  delivery only through ADR-0062 declarations filtered by their `InterestShape`.
  The dispatcher already uses exactly this gate
  (`ingest/mod.rs:486`); the fix was to move `notify_event_observers` inside the
  chokepoint under the **same** gate. That gate including `Ephemeral` **closes the
  latent bug**: an ephemeral reaches both the parsers and the app observers, so
  apps can react to ephemeral events they never store.

The store outcome — not just a valid signature — is what gates delivery. A
validly-signed event whose outcome is `Duplicate` (or `Superseded` / `Tombstoned`
/ `Rejected`) is **not** delivered.

> **Invariant:** delivery (*dispatch + notify*) fires exactly once, kind- and
> source-agnostically, on the canonical accepted outcome `Inserted | Replaced |
> Ephemeral`. Persistence is the `Inserted | Replaced` subset (ephemerals return
> `InsertOutcome::Ephemeral` and are not written).

> **Duplicate is projection-silent (D4 single-fire).** A `Duplicate` relay echo —
> including the relay echo of a locally-published event — does **NOT** notify
> observers; this is precisely what preserves D4 single-fire for read-your-writes
> (the local publish already fired delivery; the echo must not re-fire it).
> **BUT** kind:1/6 today still bump the cached `relay_count` on `Duplicate`
> (`ingest/timeline.rs:143-154`) — a diagnostic signal, not a projection mutation.
> **PR 1 requirement:** the chokepoint / timeline observer MUST preserve that
> `relay_count` bump on `Duplicate` even though it does not re-notify.

### 3. Projection / relevance = read-time only

`should_store_event` is demoted from a persistence gate to the **timeline-cache
observer's read-time predicate** — "does this event belong in MY timeline VIEW?".
It no longer gates `store.insert` and has no power over persistence. Relevance is a
read-time projection concern for every projection (timeline, profile, contacts,
mailbox, feeds). **This ADR amends ADR-0042**, which currently frames
`should_store_event` as store admission; that framing is withdrawn.

### The chokepoint

The chokepoint splits into two kind-agnostic halves:

- `verify_and_persist` does **persistence only** — sig-verify → `store.insert` →
  raw-tap → provenance → TTL stamping — and returns `(InsertOutcome, VerifiedEvent)`.
- `project_accepted_event` is the **single post-store fan-out**, gated on the
  canonical accepted outcome (`Inserted | Replaced | Ephemeral`): NIP-parser
  `EventIngestDispatcher` dispatch + the per-cache transition sweep
  (mailbox / dm-relay / profile projection-rev bumps) + the D9 future-`created_at`
  clamp on the observer payload + `notify_event_observers`.

`project_accepted_event` is called by **both** the live chokepoint
(`ingest_accepted_event`, after `verify_and_persist`) **and** the cache-serve
replay path (`feed_served_event`, which per ADR-0045 never calls `store.insert`).
Routing both through the one helper is what guarantees the live and replay paths
cannot diverge (the bug that PR 2 review caught: cache-serve had been missing the
D9 clamp and the projection-rev bumps).

- **Relay** events enter the chokepoint **after** `handle_event`'s relay-only
  bookkeeping. The clean seam is `ingest/mod.rs:281→282` (plan Q1): frame decode,
  event counters/timing, transport provenance, wire-sub diagnostics, and
  `claim_expansion_match_author` stay relay-only in `handle_event`; the shared
  body is the `match event.kind` region (`:296-425`) minus the kind arms, centered
  on `verify_and_persist`. Relay claim-hit scoring stays a relay wrapper after the
  helper returns.
- **Local publishes** enter the chokepoint **directly** with provenance
  `local://publish` (the entry point is `publish_engine.rs:169`).
  `record_local_publish_intent` and `local_publish_intent.rs` are **deleted** —
  they existed only to mirror the per-kind arms for read-your-writes, which the
  chokepoint now provides for **all** kinds uniformly.
- **Cache-replay** keeps feeding the same post-store projection seam
  (`feed_served_event`, `cache_serve/continuation.rs:210`) per ADR-0045 — replay
  **skips** `store.insert` (the event is already on disk; re-insert returns
  `Duplicate`, a no-op).

**Source / provenance — three existing encodings, preserved.** The chokepoint
takes a `source` discriminator (relay vs local-publish vs cache-serve). Each
source already carries a distinct provenance encoding that PR 1 preserves
verbatim:

| Source        | Provenance encoding (preserved)                                              |
| ------------- | ---------------------------------------------------------------------------- |
| Relay insert  | the delivering relay URL as store provenance (`ingest/mod.rs:464` → `store.insert(.., &provenance, ..)`) |
| Local publish | the literal string `local://publish` (`local_publish_intent.rs:20`, fed into the chokepoint at the publish-engine entry) |
| Cache-serve   | `relay_count: 0` — the de-facto `Provenance::LocalStore` marker (`cache_serve/mod.rs:59-64`); no relay confirmed the event this session |

The `relay_count == 0 ⇔ local-store-served` convention is a **de-facto marker
pending a proper `Provenance::LocalStore` enum variant** (named but not yet
introduced — `cache_serve/mod.rs:63` and ADR-0045 R2.4(b)). **This ADR preserves
the three existing encodings; it does NOT introduce the `Provenance` enum.**
Promoting the `relay_count: 0` convention to a typed `Provenance::LocalStore`
variant is left to the ADR-0045 amendment that names it — flagged here so it is
not silently conflated with the chokepoint's `source` discriminator.

The timeline read-cache, like `profiles` and `seed_contacts`, becomes an
observer/parser fed by the chokepoint, not a `match kind` arm.
`pre_kind3_buffer` is **not part of the current design**. Followers added later
surface prior events from the store through the normal read path.

**D9 created_at clamp — clamp the future date at the chokepoint observer
fan-out (universal hostile-relay defense).** Pre-PR-1 the future-date clamp lived
only in the timeline ingest path; the generic `kernel_event_from_nostr`
(`ingest/helpers.rs`) does **not** clamp. The observer fan-out is the input to
**every** app feed — `nmp-feed` and `nmp-nip01::FlatFeed` order their cursors by
`KernelEvent.created_at` — so a future-dated `created_at` delivered raw would pin
a hostile/buggy relay's event to the top of every consumer's feed. Clamping
future → `now` is therefore a **universal** invariant, not a timeline-only
concern. **PR 1 (as implemented):** the chokepoint clamps the future
`created_at` to `now` on the observer-delivered `KernelEvent` (inside
`verify_and_persist`, at the single `notify_event_observers` site), protecting
ALL feed consumers once; the **timeline read-cache projection**
(`project_timeline_event`) also clamps its `self.events` entry independently
(strictly stronger — it protects the kernel's own timeline ordering too). The
authoritative `EventStore` row retains the **original** wire timestamp for
protocol correctness (NIP-01 replaceable/ephemeral handling) — only the
observer-delivered / read-cache shapes are clamped. (Earlier PR-1 drafts emitted
the raw timestamp on the observer and clamped only the timeline read-cache; that
left non-timeline feed consumers exposed and was corrected to the
chokepoint-observer clamp above.)

### Storage model: complete durable store, bounded RAM working set

The on-device `EventStore` keeps every valid fetched event by default. "Persist
everything" means **admission is not relevance-gated** (every validly-signed event
is admitted kind-agnostically, and the canonical non-ephemeral subset
`Inserted | Replaced` is written), and production GC does not age out valid
durable rows:

- `Kernel::run_gc_step` (wired in production on the 60s actor idle tick) uses
  `GcBudget::production()`, which leaves durable LRU deletion disabled
  (`max_total_events = usize::MAX`, #1480). It still reaps correctness deletes
  and tombstones. RAM working-set pressure is handled by kernel RAM-cache
  eviction, not durable LMDB row deletion.
- The pin-aware durable LRU path remains available only for an explicit finite
  disk/user quota policy via `GcBudget::with_durable_event_ceiling(n)`.
  `derive_store_pin_set` (#1090 Stage 2) and the coverage-ledger backstop (#0056
  Stage D3) remain the safety machinery for that explicit path, so a future quota
  cannot punch a hole below an active floored self-healing REQ.
- If a future app enables explicit finite durable retention, it must treat
  locally accepted but not-yet-confirmed publishes as protected until first relay
  confirmation or terminal settlement, or make that quota policy explicitly
  ineligible for publish-in-flight rows. The production default does not need a
  publish-in-flight durable pin because valid durable rows are not LRU-deleted.

### What falls out for free

- **Read-your-writes for ALL kinds** — local events are admitted and pass the same
  notify step → **#1440 closed**, with no per-kind local arm.
- **Complete store (no relevance holes)** — cache-serve / offline becomes sound,
  every projection is rebuildable from the store, the cross-session dedup floor is
  restored → **#1442 closed**.
- **No drift** — one ingest path; a relay echo of a local publish dedups to
  `Duplicate`, so observers fire exactly once (D4).
- **Persistence stops assuming "social" (partial D0)** — removing the follow-set
  *relevance persistence gate* means third-party non-follow interests get stored,
  kind-agnostically. This is the persistence-layer D0 fix only. kind:0/kind:3
  remain in kernel-owned ingest paths (with their own kind literals) until PR 2 /
  PR 3, so **full ingest-path D0 — zero kind literals in dispatch — lands only
  after PR 3**, not in PR 1.

---

## Scope and sequencing (landed record)

This ADR established the architecture; the code landed as the ordered PR sequence
below. All three PRs shipped — the chokepoint is live and contacts/profile parsing
reached the D0 finish-line (`contacts_chokepoint_pr3_tests.rs`).

- **PR 1 — core fix (atomic; closed #1442 + #1440).** Moved `notify_event_observers`
  inside `verify_and_persist` gated `Inserted | Replaced | Ephemeral`; introduce
  the `ingest_accepted_event(source, event)` chokepoint at `ingest/mod.rs:281→282`;
  route kind:1|6 through `verify_and_persist` and demote `should_store_event` to the
  timeline observer's read-time predicate; demote the timeline read-cache to a
  chokepoint-fed observer; route the relay path and publish-engine success through
  the chokepoint and delete `record_local_publish_intent` / `local_publish_intent.rs`;
  keep the ADR-0045 replay rule; delete `pre_kind3_buffer`; add the GC/pin stress
  tests; upgrade the NMP consumer apps and cut a new NMP version. **profile and
  contacts caches stay kernel-owned for now** but are CALLED BY the chokepoint
  post-`verify_and_persist` (no scattered ladder).
- **PR 2 — `profiles` → capability-owned cache.** Added a `ProfileLookup`-style read
  trait, migrated the synchronous profile readers, moved kind:0 parsing to a
  registered `IngestParser`; dropped the kernel arm.
- **PR 3 — `contacts` → parser + source-trigger seam (the D0 finish-line).**
  A kind:3 parser writes the cache and the kernel detects active-account contact
  transitions by bracketing the chokepoint with before/after cache reads. The
  kernel enqueues a source recompile trigger; ReducedSource owners materialize
  child interests through the generic dependent-interest path. After PR 3 the
  ingest path has **zero kind literals** — full D0 purity.

Signer / capability authority and action/projection-lifecycle ownership are a
separate problem domain and are **out of scope** for this ADR.

---

## How we'll know it's correct (verification oracles)

Concrete oracles for PR 1 (these are the acceptance criteria of this decision):

- A non-followed kind:1/6 **persists** to the store but does **NOT** timeline-project
  (persistence ≠ projection — the event IS stored; it just is not projected into
  the timeline view).
- A local kind:1 / kind:6 / kind:7 **read-your-writes** works — persisted and
  delivered to app observers immediately, before any relay ACK.
- A relay echo of a locally-published event **dedups** (`Duplicate`) and does **NOT**
  double-notify observers (D4) — yet the kind:1/6 cached `relay_count` **still
  bumps** on that `Duplicate` (the diagnostic signal is preserved).
- A future-dated (hostile-relay) event is **clamped to `now` on the
  observer-delivered `KernelEvent`** (so it cannot pin to the top of any app feed
  that orders by `created_at`) and on the timeline read-cache projection; the
  authoritative stored event retains the original timestamp (D9).
- kind:0 / kind:3 still update profile / contact caches (no regression), and an
  active-account kind:3 enqueues the source recompile trigger without inlining
  feed-interest expansion in the parser or ingest arm.
- An ephemeral event (20000–29999) does **NOT** persist (store-layer exclusion
  intact) **BUT still reaches NIP parsers AND app `ObservedProjectionSink`s** — the
  latent-bug fix: an app can react to an ephemeral event it never stores.
- `pre_kind3_buffer` deletion does **NOT** lose later timeline visibility — a follow
  added later still surfaces prior events from the store.
- GC/pin stress (Q4): read-your-writes events are pinned and survive; non-followed
  cold notes are reaped to the ceiling; the truncation → LRU-skip path
  (`ram_eviction.rs:309-318`) stays bounded.

---

## Doctrine

- **D0** — no NIP kind literals in the kernel ingest dispatch; gate by behavioral
  predicates (`is_replaceable`, `is_addressable`, active generic interests, parser
  `is_interested`). kind:1059 gift-wrap stays excluded via the parser registry,
  not a literal. Full D0 purity (zero kind literals in the ingest path) is
  reached at the end of PR 3, when profile and contacts also become
  parser/observer-fed.
- **D4** — `store.insert` stays the single writer; observers/parsers fire once per
  accepted event, on the outcome gate. A relay echo of a local publish dedups to
  `Duplicate` and does not double-notify.
- **D5 / #1090** — pin-aware LRU eviction is the only storage bound; admission is
  never relevance-gated.
- **D8** — push observers, no polling.
- **D9** — the created_at clamp (hostile-relay defense) stays, applied at the
  chokepoint observer fan-out (universal — protects all feed consumers) and on
  the timeline read-cache projection; the stored event keeps the original
  timestamp.
- **ADR-0045** — single always-on mechanism for event acquisition + post-store
  projection dispatch; replay feeds the seam, not `store.insert`. This ADR
  generalises that read-half precedent to all event sources.

---

## Consequences

- **Tradeoff — shape-matching cost moves off the table for admission.** Deleting
  acquisition-match removes the `matches_active_open_interest` per-event walk
  (`timeline.rs:334`) from the persist path; the relevance walk moves to read-time
  where it belongs and only runs for projections that actually read it.
- **One ingest path, two source wrappers.** Relay bookkeeping and local provenance
  become thin wrappers around one chokepoint; the dual ladder and its drift are
  gone.
- **The store becomes complete.** Canonical non-ephemeral outcomes
  (`Inserted | Replaced`) are persisted regardless of relevance (ephemeral delivers
  but is not stored; `Duplicate | Superseded | Tombstoned | Rejected` are not new
  canonical writes), so every derived projection (NIP-parser caches, feeds,
  timeline) is rebuildable on restart with no relevance-shaped holes.
- **Deletions:** `record_local_publish_intent`, `local_publish_intent.rs`,
  `pre_kind3_buffer`, and `should_store_event`'s persistence authority.
- **Doctrine gates (landed):** doctrine-lint **D23** bans `store.insert` outside
  the single accepted-event ingest module (`verify_and_persist`), and **D24** bans
  `notify_event_observers` outside the shared `project_accepted_event` seam —
  locking this architecture in
  (`crates/nmp-testing/bin/doctrine-lint/rules/d23.rs`, `d24.rs`).
- **Docs amended:** ADR-0042 (above), and the durable docs that echoed the
  `should_store_event`-as-admission framing
  (`docs/product-spec/subsystems.md`, `docs/builder-guide/08-eventstore.md`,
  `docs/builder-guide/12-publish-and-ledger.md`) no longer carry that framing.

---

## References

- Issues #1440, #1442, #1443, #1480 — the durable "why" / tracking for this
  decision.
- ADR-0042 (`docs/decisions/0042-m2-open-interest.md`) — amended.
- ADR-0045 (`docs/decisions/0045-store-projection-replay.md`) — extended.
- ADR-0053 (`docs/decisions/0053-host-declared-projection-subscriptions.md`).
- `docs/architecture/crate-boundaries.md` §4.2 — the `IngestParser` migration.
- Code: `crates/nmp-core/src/kernel/ingest/mod.rs` (`handle_event:245`,
  store provenance `:464`, `verify_and_persist:436`, wildcard observer gate
  `:389-404`, dispatcher/delivery gate `:486`),
  `crates/nmp-core/src/kernel/ingest/timeline.rs` (`ingest_timeline_event:18`,
  `Duplicate` relay_count bump `:143-154`, D9 created_at clamp `:229`,
  `should_store_event:299`),
  `crates/nmp-core/src/kernel/ingest/helpers.rs` (`kernel_event_from_nostr:49` —
  no clamp), `crates/nmp-core/src/kernel/local_publish_intent.rs`
  (`local://publish` provenance `:20`),
  `crates/nmp-core/src/kernel/publish_engine.rs:169`,
  `crates/nmp-core/src/kernel/cache_serve/continuation.rs:210` (`feed_served_event`),
  `crates/nmp-core/src/kernel/cache_serve/mod.rs:59-64` (`relay_count:0`
  de-facto `Provenance::LocalStore` marker),
  `crates/nmp-core/src/kernel/ram_eviction.rs:221` (`derive_store_pin_set`),
  `crates/nmp-core/src/substrate/ingest.rs` (`IngestParser` / `EventIngestDispatcher`),
  `crates/nmp-store/src/types/gc.rs` (`GcBudget::production`,
  `GcBudget::with_durable_event_ceiling`),
  `crates/nmp-store/src/types/outcomes.rs:11-33` (`InsertOutcome` enum).
- Docs: `docs/builder-guide/08-eventstore.md:51` (outcome-by-kind table).
