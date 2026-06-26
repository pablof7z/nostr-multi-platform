# WIP Plan — NMP event-flow architecture (one kind-agnostic door: acquire · ingest · publish)

**Status:** design / pre-ADR · **Started:** 2026-06-15 · **Owner-driven**

Temporal working file for an in-flight architectural change. Detail moves into an
ADR + durable wiki once landed; delete this file when the work is merged.

Initial tracking issues: **#1440** (ghost-post / no optimistic local echo for non-replaceables),
**#1442** (persistence entangled with relevance — authoritative store has relevance-shaped holes).
Both are the **same root cause** and share **one first-keystone fix**. This plan owns the kernel's
**event-flow architecture**: the bet that the kernel has exactly ONE kind-agnostic door on each path —
**acquire (Workstream B), ingest (Workstream A), publish (Workstream C)** — and nothing bypasses it,
locked in by doctrine gates (Workstream F).

> **Sibling plan:** capability/authority ownership and action/projection-lifecycle ownership (the former
> Workstreams D & E — signer authority, `active_local_keys` ambient authority, `AppHost` god-trait, NIP-55
> concurrency, action-lifecycle collapse) are real framework architecture of equal caliber but a **separate
> problem domain** (the kernel owning its *authority and lifecycles*, not its *event flow*). They live in
> **`docs/plans/arch-authority-lifecycle.md`** so this plan stays one coherent concern. Sequenced independently.

**Delivery: ONE plan, an ordered sequence of PRs that reaches the FULL architectural endpoint — zero deferred debt.**
The bug fix (PR 1) is internally coupled and ships atomically. The remaining endpoint work (profile/contacts
parser-ownership, the crate-boundaries §4.2 finish-line, and the adjacent boundary violations below) is split into
follow-on PRs **within this same plan** — NOT dropped into a someday-issue. The plan is done only when every item
below is merged, explicitly superseded by source verification, or moved to a linked issue/ADR with owner-approved scope.
**An ADR is written BEFORE PR 1** (it supersedes the dual-ladder design and folds in #1440 + #1442).

---

## 1. The bug (one sentence)

Event ingestion is split **by source** (relay vs local publish vs cache-replay) **and by kind**
(per-kind arms), and within those arms three distinct concerns — **persistence**,
**admission/relevance**, and **projection mutation** — are fused. The fused logic is then
duplicated across two hand-maintained per-kind ladders that drift.

- Relay ladder: `Kernel::handle_event` (`crates/nmp-core/src/kernel/ingest/mod.rs:~257` bookkeeping, `:296` kind dispatch)
- Local ladder: `Kernel::record_local_publish_intent` (`crates/nmp-core/src/kernel/local_publish_intent.rs:9`)

### Confirmed code facts (both Opus + codex agreed; verified in source)
- `verify_and_persist` (`ingest/mod.rs:436`) already does, kind-agnostically: sig-verify → `store.insert`
  → raw-tap fan-out → `EventIngestDispatcher.dispatch` (gated on `Inserted|Replaced|Ephemeral`, `:486-499`)
  → replaceable TTL stamping. **But it does NOT fire `notify_event_observers`** (callers do, per-arm).
- kind:0 → `ingest_profile`, kind:3 → `ingest_contacts` (own arms, mutate kernel-owned caches `self.profiles`/`self.seed_contacts`).
- **kind:1 / kind:6 bypass `verify_and_persist` entirely** → self-contained `ingest_timeline_event`
  (`ingest/timeline.rs:18`), which alone also does: admission gate, pre_kind3 parking, timeline read-cache
  append, created_at clamp.
- **The clearest layering bug:** `should_store_event` (`ingest/timeline.rs:25`, def `:299`) is a *timeline-relevance*
  predicate (primary clause `timeline_authors.contains(author)` — the follow set) that runs **before** `store.insert`,
  so it gates **persistence**. → A self-authored note fails (you're not in your own follow set); a non-followed
  author's reply is dropped unless an escape hatch matches.
- **Asymmetry (the #1442 core):** kind:1/6 are the ONLY kinds whose *persistence* is relevance-gated. Every other
  kind (0,3,7,10002,…) persists on valid-signature alone via `verify_and_persist`.
- **Ephemeral (20000–29999) is already excluded from PERSISTENCE correctly at the STORE layer** (`nmp-store`
  lmdb/insert.rs:36-41, mem/insert.rs:40-43, `is_ephemeral()` types/events.rs:68-72, returns `InsertOutcome::Ephemeral`).
  Keep it there; do NOT add an ingest-layer persistence check.
- **LATENT BUG (found 2026-06-15): ephemeral events do NOT reach app observers today.** `verify_and_persist` dispatches
  to NIP parsers on `Inserted|Replaced|Ephemeral` (`ingest/mod.rs:486`), but the wildcard arm's `notify_event_observers`
  fires only on `Inserted|Replaced` (`:389-404`). So ephemerals reach the parser registry but NOT the app-facing
  `KernelEventObserver` seam. Apps must be able to use ephemeral events → PR 1 must make the observer gate
  `Inserted|Replaced|Ephemeral`. Persistence stays non-ephemeral; delivery becomes all-valid.
- Two stores exist: authoritative `EventStore` (`self.store`) vs in-memory read-caches (`self.profiles`, `self.events`,
  `self.timeline`). The volatile cache's *policy* (follow-set relevance) is punching holes in the durable store — inverted authority.

---

## 2. Target architecture (three layers, one chokepoint)

```
 SOURCES            ADMISSION (per-source)        CHOKEPOINT (kind-agnostic)         PROJECTIONS (observers/parsers)
 relay   ──► matches active acquisition source ─┐  ┌───────────────────────────┐   ┌► timeline cache (relevance-filtered at read)
 local   ──► publish-engine already accepted  ──┼─►│ 1 verify sig              │   ├► profile cache
 replay  ──► already in store ─────────────────┘  │ 2 store.insert (uncond.,  │──►├► contacts cache
                                                   │   ephemeral dropped @store)│   ├► mailbox / DM-relay caches (NIP parsers)
                                                   │ 3 dispatch → NIP parsers   │   ├► nmp-feed / app projections
                                                   │ 4 notify_event_observers   │   └► (all gated on Inserted|Replaced|Ephemeral)
                                                   └───────────────────────────┘
                                       (cache-replay skips step 2, feeds 3+4 directly — ADR-0045)
```

**Invariant:** **dispatch + notify every validly-signed event** (ephemeral included) once, kind-agnostically, from
any source, so apps can react to all of them; **persist** every validly-signed **non-ephemeral** event (ephemerals
skip `store.insert` at the store layer). Relevance is a **read-time projection concern only**; durable storage is
bounded by GC/watermark only (D5, #1090).

### Layer responsibilities
- **(a) Admission** — **valid signature. That's the whole gate.** No acquisition-match, no `should_store_event`,
  no per-source admission logic. **Ephemeral-ness is NOT an admission criterion** — it only governs the *persist*
  step (see below). Every validly-signed event, ephemeral or not, flows through the chokepoint to the NIP parsers
  AND the app-facing `KernelEventObserver`s; ephemerals simply skip `store.insert`. (Owner decision 2026-06-15:
  an acquisition-match check is
  pointless complexity — signatures already prevent forgery, and projections filter at read-time so an unsolicited
  event is invisible anyway. The check is also incoherent today: only kind:1/6 have it; kind:0/3/7/10002 already
  persist on valid-sig alone. So we delete the concept, not generalize it.) The only thing acquisition-match could
  buy is DoS/write-amplification defense (a hostile relay signing garbage under throwaway keys), but that is a
  **transport-layer** concern (per-relay rate-limit/quota) — NOT an ingest gate — and GC+pinning already bound the
  damage (spam is cold/unpinned → evicted first; live events are pinned). If DoS ever becomes real, add a transport
  quota at that layer. Local: publish engine already accepted it. Replay: already in store. All three collapse to the
  same rule.
- **(b) Chokepoint** — one helper (working name `ingest_accepted_event(source, event)`), placed **below**
  `handle_event`'s relay-frame bookkeeping (codex: don't route local through `handle_event` literally — it does
  relay counters / transport provenance / wire-sub diagnostics first). Essentially `verify_and_persist` with
  `notify_event_observers` pulled **inside** it.
- **(c) Projections as observers/parsers** — `self.profiles`, `self.seed_contacts`, the timeline read-cache become
  observers/`IngestParser`s fed by the chokepoint, NOT `match kind` arms. `should_store_event` becomes the
  **timeline-cache observer's** read-time predicate ("does this belong in MY timeline view?") — no power over persistence.

### Why the NIP-parser seam matters here
`EventIngestDispatcher` / `IngestParser` (`substrate/ingest.rs:33,58`) is the D0-honest mechanism: NIP crates
register per-kind parsers that write their OWN typed caches (e.g. `nmp-nip17::DmRelayCache`, `nmp-nip65::MailboxCache`);
the kernel reads those back via narrow capability traits (`DmInboxRelayLookup`, routing) and never names a kind.
These caches are **derived projections rebuilt from the store on restart** → a relevance-shaped hole in the store
becomes a permanent hole in every projection (this is the mechanism behind past "missing DMs/replies" findings).
End-state: finish the migration this seam was built for (crate-boundaries.md §4.2) — profile/contacts/timeline also
become parser/observer-fed, leaving zero kind literals in the kernel ingest path.

---

## 3. What falls out for free (verification targets)
- Read-your-writes for ALL kinds (local admitted + same notify step) → **#1440 closed**, no per-kind arm.
- Complete store (no relevance holes) → cache-serve/offline sound, projections rebuildable, cross-session dedup floor restored → **#1442 closed**.
- No drift: one ingest path; relay echo of a local publish dedups to `Duplicate` → observers fire once (D4).
- `pre_kind3_buffer` deleted — it only existed to park events the entanglement would have dropped.
- App-agnostic again (D0): persistence stops assuming "social"; non-follow third-party interests get stored.

### Storage model clarification (updated by #1480)
The on-device `EventStore` keeps every valid fetched event by default. The durable
LMDB row set is **not** capped by the RAM hot-set ceiling; `Kernel::run_gc_step`
(wired in production, actor 60s idle tick) uses `GcBudget::production()`, which
reaps correctness deletes and tombstones but leaves durable LRU deletion disabled
(`max_total_events = usize::MAX`). RAM working-set pressure is handled separately
by kernel RAM-cache eviction.

"Persist everything" therefore means **admission is not relevance-gated** and
production GC does not age out valid durable rows. The guarded pin-aware durable
LRU machinery remains available only through an explicit finite-retention budget
(`GcBudget::with_durable_event_ceiling(n)`) for a future disk/user quota policy.
When that explicit path is used, pin-set correctness and coverage backstops remain
the safety property that prevents holes below active floors.

---

## 4. Resolved design questions (from parallel Opus + codex code research, 2026-06-15)
- **Q1 [RESOLVED]** — clean chokepoint seam at `ingest/mod.rs:281→282`. Relay-only (stays in `handle_event`,
  NOT in shared helper): frame decode `:252-255`, event counters/timing `:257-266`, transport provenance `:263`,
  wire-sub diagnostics `:267-280`, `claim_expansion_match_author` `:281`. EOSE is a sibling frame (`eose.rs`), not ingest.
  Shared body = the `match event.kind` region `:296-425` minus kind arms, centered on `verify_and_persist` `:436`.
  Relay claim-hit scoring stays a relay wrapper after the helper returns. Local publish enters at `publish_engine.rs:151-170`;
  cache replay keeps feeding `feed_served_event` (`cache_serve/continuation.rs:210-272`) → same post-store seam.
- **Q3 [RESOLVED]** — no milestone/coverage breakage. Every site reads read-caches (`self.timeline`, `self.profiles`,
  `self.seed_contacts`, `self.events`) or per-REQ `since_floor`, never `self.store` counts (`status.rs:193-208`,
  `should_open_timeline` `timeline.rs:375-403`, coverage ledger `coverage_ledger.rs:52-84`, EOSE `eose.rs:49-51`).
  Caveat (not a bug): the `stored_events` metric is actually a RAM-projection count (`update.rs:122-127`) — misnamed;
  rename for honesty when convenient.
- **Q2 [OWNER-DECIDED: full migration, sequenced across PRs, no debt]** — `ingest_contacts` is NOT just a cache write:
  it drives source-recompile effects (`CompileTrigger` `contacts.rs`, active
  follows via ReducedSource/dependent-interest recompilation) that an
  `IngestParser` (which gets a bare `VerifiedEvent`, no `&mut self`/`active_account`) structurally cannot reach. `seed_contacts` also has a
  non-ingest writer (sign-in `prepopulate_seed_contacts` `identity.rs:1032`). `profiles` has ~10 synchronous readers
  (hot `profile_for_pubkey` `views.rs:185`, zap LNURL `diagnostic_counters.rs:77-84`, TTL/claim dedup `requests/profile.rs`).
  → These cannot fold into PR 1 safely, but per owner they are NOT deferred to debt — they are PRs 2 & 3 of this plan.
- **Q4 [OWNER-DECIDED: keep ceiling, prove with tests]** — GC is wired (old "never called" finding STALE). Bound =
  10k ceiling / 2000 events per 50ms per 60s tick, unpinned-first LRU. Newly-stored non-followed notes are exactly the
  unpinned class evicted first. One real hazard: `derive_store_gc_inputs` disables LRU for a tick when the floor-coherent
  pin scan truncates (`ram_eviction.rs:309-318`) — must not regress into unbounded growth. No write-time size gate exists
  (`mem/insert.rs`, `lmdb/insert.rs` only reject malformed/ephemeral/expired/superseded).
- **Q5 [OWNER-DECIDED: NO acquisition-match admission]** — admission is **valid signature, nothing else**; ephemerality
  governs only the *persist* step (ephemerals still dispatch + notify). The acquisition-match concept is **deleted, not generalized** (rationale in §2a: signatures prevent forgery, read-time
  projections hide unsolicited events, and the check is incoherent today — only kind:1/6 have it). DoS/write-amplification
  is a transport-layer quota concern, not an ingest gate. This **dissolves the earlier B→PR1 dependency and the
  loose-vs-strict admission ambiguity**: PR 1 no longer depends on `InterestRegistry` being single-door, so Workstream B
  is now a pure DRY/maintainability cleanup, NOT a correctness prerequisite for the ingest fix.

---

## 5. Workstream A (event ingest) — ordered PR sequence (ONE plan, no deferred debt)

### PR 0 — ADR (written first, before any code)
- [ ] Write ADR "Unified kind-agnostic accepted-event ingest chokepoint; persistence ≠ admission ≠ projection".
      Supersedes the dual-ladder design; folds in #1440 + #1442; states the storage model (bounded cache + pin-aware LRU).
- [x] **Amend ADR-0042** so it no longer frames `should_store_event` as store admission — DONE (§5/§5.1/§5.2 corrected in place).
- [x] Verified the three durable docs (`docs/product-spec/subsystems.md`, `docs/builder-guide/08-eventstore.md`,
      `docs/builder-guide/12-publish-and-ledger.md`) do NOT contain the admission framing — they already describe the
      store via the one-insert-path + `InsertOutcome` model (ephemeral = "deliver live, never store"), consistent with
      ADR-0057. No correction needed. NOTE for PR 1: `08-eventstore.md:80` ("the kernel does exactly that for kinds
      0/3/10002") describes the per-kind arms and should be refreshed to the unified chokepoint when PR 1 lands.
- [ ] Record that #1440's narrow "add a 4th local-publish arm" framing is **superseded** by this architecture.

### PR 1 — Core fix (atomic; closes #1442 + #1440)
The changes below are internally coupled — moving observers into the chokepoint, decoupling `should_store_event`,
and unifying the source paths cannot land separately without a broken intermediate (double-fire or dropped events).
- [ ] 1. Move `notify_event_observers` INTO `verify_and_persist`, gated `Inserted|Replaced|**Ephemeral**`; remove per-arm
        calls. **This fixes the latent bug (§1): ephemerals must reach app observers, not just NIP parsers.** Persistence
        stays non-ephemeral (store layer); delivery (dispatch + notify) is all-valid-including-ephemeral.
- [ ] 2. Introduce `ingest_accepted_event(source, event)` chokepoint at the `ingest/mod.rs:281→282` seam (Q1).
- [ ] 3. Route kind:1|6 through `verify_and_persist` (kill the duplicate `store.insert`/sig-verify in `ingest_timeline_event`);
        demote `should_store_event` to the timeline-cache **observer's read-time predicate** — it no longer gates `store.insert`.
- [ ] 4. Demote the **timeline read-cache** to an observer fed by the chokepoint. (profile/contacts caches stay kernel-owned
        for now but are CALLED BY the chokepoint post-`verify_and_persist`, gated on `Inserted|Replaced` — no scattered ladder.)
- [ ] 5. Route relay path (after `handle_event` bookkeeping) AND publish-engine success (provenance `local://publish`)
        through the chokepoint. Delete `record_local_publish_intent` / `local_publish_intent.rs`.
- [ ] 6. Keep ADR-0045 replay rule (replay skips `store.insert`, feeds post-store seam — `feed_served_event` already models this).
- [ ] 7. Delete `pre_kind3_buffer` (admission/persistence now separated → parking is obsolete).
- [ ] 8. Add GC/pin stress tests (Q4): non-followed kind:1 stays unpinned + reaped to ceiling; **read-your-writes events are
        pinned and survive until relay echo**; the truncation→LRU-skip path (`ram_eviction.rs:309-318`) stays bounded.
- [ ] 9. Upgrade NMP consumer apps (podcast-player / hl) to latest; cut new NMP version.

### PR 2 — `profiles` → capability-owned cache
- [ ] Add a `ProfileLookup`-style capability read trait (mirrors `nmp-nip17::DmInboxRelayLookup` / `ZapProfileLookup`).
- [ ] Migrate the ~10 synchronous profile readers (`views.rs`, `projections.rs`, `requests/profile.rs`,
      `discovery.rs`, `diagnostic_counters.rs`, `typed_projections`, `ram_eviction`) onto the trait.
- [ ] Move kind:0 parsing to a registered `IngestParser` writing the capability-owned profile cache; drop the kernel arm.

### PR 3 — `contacts` → parser + kernel-owned effect seam (the hard one)
- [ ] Design a typed "contacts changed" effect signal the kind:3 parser can emit that the kernel reacts to on its tick.
      The parser writes the cache; source reduction and dependent-interest recompilation remain kernel/session-owned
      and are driven by the signal, not inlined into the parser. This keeps planner/lifecycle effects kernel-owned (D-correct)
      while removing the last kind literal from the ingest path.
- [ ] Reroute the non-ingest `seed_contacts` writer (sign-in `prepopulate_seed_contacts`) and reader
      (`register_follow_feed_for_active_account`) through the new ownership.
- [ ] Move kind:3 parsing to the registered parser; delete the kernel arm. **Ingest path now has ZERO kind literals.**

## 6. Doctrine constraints
- **D0** — no NIP kind literals in kernel dispatch; gate by predicates (`is_replaceable`, `is_addressable`,
  ReducedSource predicates, parser `is_interested`). kind:1059 gift-wrap stays excluded via parser registry, not a literal.
  (Full D0 purity — zero kind literals — is reached at end of PR 3.)
- **D4** — `store.insert` stays single writer; observers/parsers fire once, on outcome gate.
- **D5 / #1090** — pin-aware LRU eviction is the ONLY storage bound; admission is never relevance-gated.
- **D8** — push observers, no polling.
- **D9** — keep created_at clamp (hostile-relay defense) in the timeline observer.
- **ADR-0045** — single mechanism for event acquisition + post-store projection dispatch; replay feeds the seam, not `store.insert`.

## 7. Verification oracles (concrete, for PR 1)
- [ ] Non-followed kind:1/6 **persists** to the store but does **NOT** timeline-project (admission ≠ persistence).
- [ ] Local kind:1 / kind:6 / kind:7 **read-your-writes** works (visible immediately, before any relay ACK).
- [ ] Relay echo of a locally-published event **dedups** (`Duplicate`) and does **NOT** double-notify observers (D4).
- [ ] kind:0 / kind:3 still update profile / contact caches (no regression) — and kind:3 still rebuilds `timeline_authors` / interests.
- [ ] Ephemeral (20000–29999) does **NOT** persist (store-layer exclusion intact) **BUT still reaches NIP parsers AND
      app `KernelEventObserver`s** (the §1 latent-bug fix — an app can react to an ephemeral event it never stores).
- [ ] `pre_kind3_buffer` deletion does **NOT** lose later timeline visibility (a follow added later still surfaces prior events from the store).
- [ ] GC/pin stress (Q4): read-your-writes events pinned & survive; non-followed cold notes reaped to ceiling; truncation-skip path bounded.

## 8. Remaining NMP architecture workstreams

This section is for **framework-level architectural repairs** only: ownership boundaries, one-door seams,
ambient authority removal, and doctrine gates that prevent the same boundary violations from returning.
Higher-layer and app-specific cleanups found during the audit are tracked outside this plan, unless they
require a new NMP framework seam or doctrine gate.

Each architecture workstream below lands as an atomic PR or short PR sequence. Each PR removes the old path
it replaces and adds a test or doctrine gate that prevents reintroduction. No compat shims, no "later" TODOs,
no parallel authorities.

### Workstream B — acquisition one-door: `InterestRegistry`
> **Decoupled from PR 1 (Q5):** since admission is now valid-sig-only (no acquisition-match), this workstream is a
> DRY/maintainability cleanup (one way to build REQs, no hand-rolled duplicates) — NOT a correctness prerequisite for
> the ingest fix. Can land independently / in parallel with PRs 1–3.
- [ ] Profile claims stop constructing direct REQs in `kernel/requests/profile.rs`; `claim_profile` / `release_profile`
      become owner-keyed `LogicalInterest`s owned by `InterestRegistry -> SubscriptionLifecycle`.
- [ ] Replaceable reverify stops constructing direct REQs in `kernel/requests/mod.rs`; model it as a registry-owned
      freshness / one-shot interest keyed by `ReplaceableKey`, with EOSE tied to the compiled subscription identity.
- [ ] Implicit mailbox discovery gets an epoch/probe lifecycle so empty EOSE or indexer outage does not permanently
      suppress re-probing uncached authors.
- [ ] Add a one-door test/lint that feature request helpers cannot call `req_for_relay` outside the compiler/lifecycle.

### Workstream C — publish policy one-door
- [ ] Replace raw kind literals in generic publish/outbox routing with typed behavior classification:
      `reserved_builder_only`, `requires_explicit_target`, `discovery_indexable`, `public_routable`,
      `private_fail_closed`.
- [ ] Keep D10 fail-closed behavior for gift-wrap/private events, but make invalid `Auto` routing impossible by type
      or protocol action shape rather than relying on scattered literal guards.
- [ ] Keep reserved kind:0/kind:3 builder invariants, but move that policy behind typed publish capabilities.

> **Workstreams D (signer/capability authority) and E (action/projection lifecycle ownership) moved out** to
> `docs/plans/arch-authority-lifecycle.md` — separate problem domain (authority & lifecycle ownership, not event flow).

### Workstream F — doctrine and regression gates
- [ ] Add a doctrine/lint gate banning `store.insert` outside the single accepted-event ingest module, store impls,
      migrations, and tests.
- [ ] Add a doctrine/lint gate banning `notify_event_observers` outside the chokepoint/cache-replay seam.
- [ ] Add the D22 coverage-floor gate: after presence heuristic deletion, subscription planning cannot call
      event-store newest-match queries as a floor source; the coverage ledger is the only legal floor authority.
- [ ] Add a framework-level shell-boundary gate: raw event history, signatures, tag payloads, and protocol policy
      cannot cross FFI as app state. App-specific violations are fixed in app PRs; the gate belongs here.
- [ ] Add a framework-level raw-kind policy gate for shells: raw Nostr kind switches in platform shell code
      require an explicit presentation-only allowlist or a Rust projection replacement.
- [ ] Add issue/doc update checks for every PR in this plan: if a PR discovers or changes a durable invariant, the
      owning product spec, builder guide, ADR, or GitHub issue is updated in the same PR.
