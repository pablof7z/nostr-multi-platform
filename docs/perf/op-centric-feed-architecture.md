# OP-Centric Home Feed — Architecture Proposal

> **Status:** FINAL design. Implementation-ready.
> **Author:** Architect (Serena Blackwood)
> **Revision:** 2026-05-27d (post-codex-v2 + user decisions). This is the
> implementation-input draft. Subsequent residual concerns track as TODOs in
> `docs/BACKLOG.md`, not as further revisions.
>
> **Scope:** redefine the chirp-tui / Chirp / NMP home-feed model from
> "threaded notes (replies + roots) over the follow-set" to **"thread roots
> only, with follow-replies as attribution metadata on their root."**
> Includes the protocol-level mechanics required to make a non-followed root
> appear in the feed when a followed user replies to it. Delivers the
> OP-centric feed as a generic primitive in `nmp-feed`, with `nmp-nip01` as
> a thin protocol instance, follow-set expansion done at the composition
> root (`nmp-app-template`), and a kernel-side pre-kind:3 ingest buffer
> closing the cold-start gap.
>
> ### Revision history
>
> - **v1 (2026-05-27a):** initial draft. Engine in `nmp-nip01`. Bespoke
>   action. Generic factoring scoped post-v1.
> - **v2 (2026-05-27b):** generic engine pulled forward into `nmp-feed`;
>   NIP-01 became a thin instance.
> - **v3 (2026-05-27c):** addressed codex-v1 blockers — associated
>   `type Profile`, `ClaimRequest(ThreadPointer)`, `Kernel::claim_event`
>   primitive, full Swift consumer enumeration.
> - **v4 (this revision, 2026-05-27d):** addresses codex-v2 (review at
>   `docs/perf/op-centric-feed-architect-review.md`) and four user
>   decisions (Q1, Q2, Q5, Q7). Major changes:
>   - **Composition-root expansion of the follow-set timeline (codex
>     architecture override).** `LogicalInterest::SocialTimeline` is
>     **deleted from the design**. `nmp-app-template` expands the active
>     follow set into concrete per-author `LogicalInterest`s at
>     composition + on every kind:3 update. No planner-side seam, no
>     `FollowSetLookup` cycle. **User's Q2 enum-conversion answer is
>     therefore moot** — there is no variant to convert. Reasoning in
>     §3-D-decision.
>   - **`FollowSetLookup` becomes a generic predicate, not a trait.** The
>     engine takes `Arc<dyn Fn(&str) -> bool + Send + Sync>`. No new
>     trait crate, no dependency cycle. Codex §3-Q-options aligned. The
>     follow-set producer lives in `nmp-nip02`; the predicate is wired
>     by `nmp-app-template`.
>   - **Pre-kind:3 ingest buffer (Q7 implementation).** The kernel
>     buffers kind:1 / kind:6 events that arrive before the active
>     account's kind:3 is processed in a bounded queue keyed by
>     event id. When kind:3 lands and `timeline_authors` is rebuilt,
>     the kernel replays buffered events whose author is now in
>     `timeline_authors` through the normal ingest+observer path. The
>     engine sees them as ordinary fan-out, no replay API needed at
>     the engine layer. Rung 1 expansion.
>   - **No-match release signal (codex B2-remainder).** Adds a kernel
>     surface `pub fn event_claim_released(&self) -> &BoundedRingBuffer<EventId>`
>     projection (substrate-generic name; not `claim_event`-specific to
>     callers). The engine observes it through a new `RawEventObserver`
>     callback at registration time. EOSE-driven release inside
>     `complete_unknown_oneshot` now also clears `event_claims` /
>     `event_claim_requested` and pushes the primary id into the
>     released-events ring buffer. Rung 1 expansion.
>   - **URI relay hints become initial planner hints (codex B2-remainder
>     option a).** `OneshotApi::request` gains a `hints: Vec<RelayHint>`
>     parameter; `claim_event` passes URI relay TLVs both into initial
>     `LogicalInterest.hints` AND into `register_claim_expansion`. The
>     first REQ goes to bootstrap content relays ∪ hint relays. This
>     is a kernel-API change in its own right; justified because the
>     existing OP-feed work depends on it and it benefits every other
>     `claim_event` caller (quoted notes, mentions, etc.). Rung 1
>     expansion. Cited tests prove identical behavior for non-hint
>     callers.
>   - **`Q1` attribution rendering — display-layer concern.** User
>     answer: "Only 1, but this is obviously a display concern." The
>     projection now exposes ALL enumerated repliers as raw data
>     (bounded only by D5 — `MAX_PROJECTION_MESSAGES` per root). Each
>     render surface chooses how many to show. chirp-tui renders the
>     most recent 1; iOS may render N via avatars. **`attribution_total`
>     is deleted** (redundant — the `Vec<A>` length IS the total).
>     §3-C and §3-G rewritten.
>   - **Repost edge-case `EventLookup` callback (codex H3-remainder).**
>     L-2 (reply to kind:6 wrapper) and L-5 (e-tag-only repost target
>     hydrates later) require the engine to look up parent / target
>     events from the kernel's read cache. The engine gains an
>     `event_lookup: Arc<dyn Fn(&EventId) -> Option<KernelEvent> + Send + Sync>`
>     callback at construction time. Cited tests added.
>   - **`release_claim_expansion` cleanup (codex M3).** `release_event`
>     now calls `release_claim_expansion(primary_id)` when the last
>     consumer leaves so retargeting work is cancelled. Rung 1
>     expansion. Trivial; one missing line.
>   - **Serialization bounds (codex M4).** `RootFeedSnapshot<C, A>`
>     declares explicit `C: Serialize + Clone` and
>     `A: Serialize + Clone` bounds. §3-G updated.
>   - **All Rust consumers of `TimelineBlock::Standalone` enumerated
>     (codex B3-remainder).** §5 Stage 1 file list grew. Verified by
>     grep — full list in §5.
>   - **§3-B-3 (address pointer arm) corrected.** Verified against
>     `claim_event` source: address URIs use `kinds + authors + #d`,
>     not `InterestShape.addresses`.
>   - **Account-change push path is real.** `Kernel::active_account_handle()`
>     already exists (`crates/nmp-core/src/kernel/mod.rs:1265-1267`)
>     returning an `ActiveAccountSlot` the adapter can observe. v3's
>     `KernelAccountChanged` fiction is replaced by the real handle. The
>     adapter watches the slot through the same mechanism every other
>     subsystem uses today. No invented APIs.
>
> ### Codex residual disagreements
>
> Codex preferred (in §6 out-of-scope) deleting `LogicalInterest::SocialTimeline`
> entirely. The user's Q2 chose to convert it to an enum. **The architecture
> override resolves the tension in codex's direction** — there is no
> `SocialTimeline` variant in v4, so the enum-vs-discriminator question
> is moot. The user's "right not smallest" rule is satisfied because the
> resulting graph is genuinely cleaner (no FollowSetLookup trait, no
> planner consumption of follow-set capability, no risk of
> nmp-planner → nmp-feed cycle, no `LogicalInterest` enum churn touching
> 50+ call sites). I'm surfacing the override explicitly so the user can
> challenge it if I've misread; the substantive decision is logged in
> §3-D.

---

## 1. Executive summary

The home feed becomes a **stream of thread roots** produced by a generic
engine `RootIndexedFeed<R: ParentResolver, A: AttributionPayload>` in
`nmp-feed`. Each root carries an attribution list of follow's replies,
exposed as raw data (no display cap) so every render surface chooses its
own enumeration policy.

**Crate layout:**

- **`nmp-feed`** — `RootIndexedFeed<R, A>` engine, `AttributionPayload`
  trait (`type Profile`), `ClaimRequest { Claim, Release }` carrying
  `ThreadPointer`. Engine takes a generic `FollowPredicate: Arc<dyn
  Fn(&str) -> bool + Send + Sync>` and an `EventLookup: Arc<dyn
  Fn(&EventId) -> Option<KernelEvent> + Send + Sync>`. No NIP-named
  tokens; no follow-set trait; no planner coupling.
- **`nmp-nip01`** — `Nip10ReplyAttribution: AttributionPayload<Profile =
  ProfileDisplay>`, `register_op_feed(app, viewer, follow_predicate,
  event_lookup)` wiring helper (~150 LOC), `ClaimRequest` sink that
  encodes pointers as `nostr:` URIs and calls the existing
  `nmp_app_claim_event` C-ABI.
- **`nmp-nip02`** — `ActiveFollowSet`, an observable snapshot of the
  active account's follows (raw `Arc<RwLock<BTreeSet<String>>>` or
  equivalent), updated by an internal observer that watches kind:3
  ingest and the active-account slot. Exposes a `follows() -> Vec<String>`
  read and a `predicate() -> Arc<dyn Fn(&str) -> bool + Send + Sync>`
  factory. No trait introduced; no `FollowSetLookup`; this is the
  follow-set producer.
- **`nmp-app-template`** — `register_op_feed_defaults(app, viewer)`
  composes everything: constructs the `ActiveFollowSet`, wires the
  predicate + event-lookup into `register_op_feed`, registers an
  internal observer that calls `nmp_app_expand_follow_timeline_interests`
  on every kind:3 update so the planner sees fresh per-follow
  `LogicalInterest`s. No `SocialTimeline` variant. No planner-side
  capability.
- **`nmp-core`** — gains five small substrate-grade additions: (1) a
  pre-kind:3 ingest buffer for kind:1/kind:6 events, replayed when
  `timeline_authors` is rebuilt; (2) `event_claim_released` projection
  (a bounded ring buffer of released primary_ids); (3) `OneshotApi::request`
  accepts initial `hints` (URI relay TLVs); (4) `release_event` calls
  `release_claim_expansion`; (5) a typed `active_timeline_authors() ->
  Vec<String>` accessor for the existing `timeline_authors` field. **No
  follow-set trait, no NIP token, no new ProtocolCommand, no new
  bespoke C-ABI symbol.**
- **`nmp-threading`** — `TimelineBlock::Standalone { id, root:
  Option<ThreadPointer> }` (lossless).
- **`nmp-planner`** — unchanged. No `SocialTimeline` variant. No new
  capability bundle parameter.

**Four mechanisms power the system:**

1. `TimelineBlock::Standalone` becomes lossless (root pointer preserved
   on 1-event chains — closes `grouper.rs:362-368` bug).
2. `RootIndexedFeed<R, A>` engine in `nmp-feed` consumes `KernelEvent`s
   via the existing observer fan-out, emits typed `ClaimRequest` values
   for unknown roots, exposes `RootFeedSnapshot<C, A>` (visible-window-
   only) as the FFI surface.
3. `nmp-nip01`'s host adapter translates `ClaimRequest` into
   `nmp_app_claim_event` / `nmp_app_release_event` calls (existing
   C-ABI). Hydrates non-followed roots via `Kernel::claim_event`'s
   canonical OneShot path. The kernel-side enhancements (initial-hint
   plumbing, no-match release signal) benefit every claim_event consumer,
   not just the OP feed.
4. **`nmp-app-template`** is the composition root. It owns the
   follow-set producer (`nmp-nip02`'s `ActiveFollowSet`), the
   pre-engine kind:1/6 buffer drainage, the per-follow
   `LogicalInterest` registration, and the OP-feed registration.

**Net effect:**

- `nmp-core` D0-clean. `nmp-feed` D0-clean. No new bespoke C-ABI symbol.
- One-line affordance for any composing app:
  `nmp_app_template::register_op_feed_defaults(app, viewer)`.
- The pre-kind:3 cold-start gap is closed at the source (kernel
  ingest), not papered over at the engine layer.

---

## 2. Architecture diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  Product behavior — root cards + attribution row                            │
│   "Bob · 2h ago"                                                            │
│   "Building something interesting with Marmot..."                           │
│   "↳ Alice replied · Carol replied"   ← chirp-tui shows 1; iOS may show N   │
└────────────────────────────────────▲────────────────────────────────────────┘
                                     │ RootFeedSnapshot<C, A> JSON
                                     │ (visible window only)
┌────────────────────────────────────┴────────────────────────────────────────┐
│  Layer 5 — nmp-app-template (COMPOSITION ROOT — ~150 LOC NEW)               │
│                                                                             │
│   register_op_feed_defaults(app, viewer):                                   │
│     1. construct nmp_nip02::ActiveFollowSet (observer over kind:3 +        │
│        active-account slot)                                                 │
│     2. expand_active_follow_timeline_interests(app, &follow_set)           │
│        — registers per-follow LogicalInterest with the planner             │
│        — re-runs on every kind:3 change via observer callback              │
│     3. nmp_nip01::register_op_feed(app, viewer,                            │
│           predicate = follow_set.predicate(),                              │
│           event_lookup = kernel_event_lookup(app))                         │
└─────────────────────────────────────────────────────────────────────────────┘
                                     ▲
┌────────────────────────────────────┴────────────────────────────────────────┐
│  Layer 4 — nmp-nip01 (THIN — ~150 LOC)                                      │
│                                                                             │
│   Nip10Resolver: ParentResolver                  (existing)                 │
│   Nip10ReplyAttribution: AttributionPayload<Profile = ProfileDisplay>       │
│   register_op_feed(app, viewer, predicate, event_lookup) wires:             │
│     - RootIndexedFeed<Nip10Resolver, Nip10ReplyAttribution>                 │
│     - KernelEventObserver registration                                      │
│     - snapshot key "nmp.feed.home"                                          │
│     - ClaimRequest sink → nmp_app_claim_event / nmp_app_release_event      │
│     - event_claim_released observer → forwards to engine                    │
└─────────────────────────────────────────────────────────────────────────────┘
                                     ▲ KernelEvent fan-out
┌────────────────────────────────────┴────────────────────────────────────────┐
│  Layer 4 — nmp-feed (GENERIC ENGINE — ~450 LOC NEW)                         │
│                                                                             │
│   trait AttributionPayload {                                                │
│     type Profile;                                                           │
│     fn from_reply(reply, follow, profile_for) → Option<Self>;               │
│     fn reply_event_id(&self) → &str;                                        │
│     fn author_pubkey(&self) → &str;                                         │
│     fn reply_created_at(&self) → u64;                                       │
│     fn refresh_for_profile(&mut self, profile: &Self::Profile);             │
│   }                                                                         │
│                                                                             │
│   RootIndexedFeed<R, A> {                                                   │
│     resolver: R,                                                            │
│     follow: Arc<dyn Fn(&str) -> bool + Send + Sync>,                        │
│     event_lookup: Arc<dyn Fn(&EventId) -> Option<KernelEvent> + Send + Sync>,│
│     profile_detector: Box<dyn Fn(&KernelEvent)                              │
│                              -> Option<(String, A::Profile)>                │
│                          + Send + Sync>,                                    │
│     card_builder: Box<dyn Fn(&KernelEvent, ...) -> C + Send + Sync>,        │
│     roots: BoundedMessageMap<EventId, RootCard<C, A>>,                      │
│     attributions: BoundedMessageMap<EventId, BTreeMap<EventId, A>>,         │
│     pending_attributions: BoundedMessageMap<EventId,                        │
│                                BTreeMap<EventId, A>>,                       │
│     pending_pointers: BoundedMessageMap<EventId, ThreadPointer>,            │
│     profiles: BoundedMessageMap<String, A::Profile>,                        │
│     window: FeedWindowState,                                                │
│   }                                                                         │
│                                                                             │
│   impl KernelEventObserver { on_kernel_event(evt):                          │
│     • root-shaped (resolver.parent == None) → insert in roots;              │
│       flush pending_attributions for this id; emit Release for the pointer  │
│     • reply-shaped AND follow(evt.author) →                                 │
│         pointer = resolver.root(evt) or .parent(evt)                        │
│         a = A::from_reply(evt, follow.as_ref(), profile_for)                │
│         if pointer is Event AND parent event is locally available:          │
│           if event_lookup(&pointer.id).map(|e| resolver.supersedes(&e)).flatten() │
│             ⇒ re-key the attribution to the supersedes target (L-2 rule)    │
│         if pointer not in roots → emit Claim(pointer, hints)                │
│         record in attributions[pointer.primary_id] or pending_attrs         │
│     • repost-shaped (resolver.supersedes != None) → target = supersedes;    │
│         insert kind:6 wrapper into roots[target] (L-1);                     │
│         if target absent locally → emit Claim(Event(target), hints);        │
│         when target arrives later, engine rebuilds the card via L-5 rule    │
│     • profile event (profile_detector returns Some) → fan out               │
│       A::refresh_for_profile across attributions + pending_attributions     │
│     • non-follow reply / repost → dropped                                   │
│   }                                                                         │
│                                                                             │
│   on_claim_released(primary_id): drop pending_attributions[primary_id];     │
│       remove pending_pointers[primary_id]; surface as a UI hint that the    │
│       reference is permanently unresolvable (visible only via diagnostics). │
│                                                                             │
│   fn snapshot(request) → RootFeedSnapshot<C, A>                             │
│      [visible window only; cards + attribution Vec<A>, both bounded]        │
│                                                                             │
│   ClaimRequest::Claim { pointer: ThreadPointer, hints: Vec<RelayHint>,      │
│                         consumer_id: String }                               │
│   ClaimRequest::Release { pointer: ThreadPointer, consumer_id: String }     │
└─────────────────────────────────────────────────────────────────────────────┘
                                     ▲
┌────────────────────────────────────┴────────────────────────────────────────┐
│  Layer 4 — nmp-nip02 (FOLLOW-SET PRODUCER — ~120 LOC NEW)                   │
│                                                                             │
│   ActiveFollowSet (Arc-internal)                                            │
│     - watches kind:3 ingest via KernelEventObserver                         │
│     - watches Kernel::active_account_handle() for account switch            │
│     - exposes: follows() -> Vec<String>                                     │
│     - exposes: predicate() -> Arc<dyn Fn(&str) -> bool + Send + Sync>       │
│     - exposes: on_change(callback) — fires on follow-set change             │
└─────────────────────────────────────────────────────────────────────────────┘
                                     ▲
┌────────────────────────────────────┴────────────────────────────────────────┐
│  Layer 3 — nmp-core (substrate ADDITIONS, no NIP nouns)                     │
│                                                                             │
│   Existing: KernelEventObserver registry, EventIngestDispatcher, ActionModule
│             registry, OneshotApi, claim_event, claim_expansion,             │
│             active_account_handle, ActiveAccountChanged trigger             │
│                                                                             │
│   NEW for v4 (all rung 1):                                                  │
│     • Pre-kind:3 ingest buffer                                              │
│         BoundedMessageMap<EventId, NostrEvent> for kind:1/6 events that    │
│         fail should_store_event ONLY because author is not in              │
│         timeline_authors. On every sync_follow_feed_interests rebuild,     │
│         walk the buffer and re-ingest any event whose author is now in     │
│         timeline_authors via the normal ingest+observer path. D5 cap.       │
│     • event_claim_released projection                                       │
│         pub fn event_claim_released(&self) -> &BoundedRingBuffer<EventId>   │
│         Pushed by complete_unknown_oneshot's EOSE path AND by release_event │
│         when refcount reaches zero. Observable through a substrate-grade    │
│         callback the engine registers at construction.                      │
│     • OneshotApi::request gains hints: Vec<RelayHint>                       │
│         Plumbed into LogicalInterest.hints so route_hints sees URI relays   │
│         on the FIRST REQ.                                                   │
│     • release_event calls release_claim_expansion(primary_id)               │
│         M3 fix.                                                             │
│     • Kernel::active_timeline_authors() -> Vec<String>                      │
│         Typed accessor over the existing field (no new noun).               │
└─────────────────────────────────────────────────────────────────────────────┘
                                     ▲ KernelEvent + claim_event
┌────────────────────────────────────┴────────────────────────────────────────┐
│  Layer 4 — nmp-threading (REVISED)                                          │
│                                                                             │
│   TimelineBlock                                                             │
│     ├── Standalone { id, root: Option<ThreadPointer> }   ← LOSSLESS         │
│     └── Module { events, has_gap, root: Option<ThreadPointer> }             │
│   ParentResolver, Grouper, ThreadPointer — unchanged                        │
└─────────────────────────────────────────────────────────────────────────────┘
```

Data flow: **EventIngest → (if author in `timeline_authors`) store +
observer fan-out → `RootIndexedFeed::on_kernel_event` → engine emits
`ClaimRequest::Claim(pointer, hints)` if a root is missing → host
adapter calls `nmp_app_claim_event` → kernel runs `claim_event` →
`OneshotApi::request` includes URI relay hints → planner routes to
`bootstrap_content_relays ∪ hint relays` on the FIRST REQ → Bob's OP
arrives, ingest stores it, observer fan-out fires, engine receives the
root → snapshot rebuilds → on EOSE without match, kernel pushes the
primary_id into `event_claim_released` → engine drops
`pending_attributions[primary_id]`.**

Pre-kind:3 cold-start: kernel buffers kind:1/6 events that would
otherwise drop at `should_store_event`. When kind:3 lands and rebuilds
`timeline_authors`, the buffer drains through normal ingest. Engine
sees the events as ordinary fan-out.

---

## 3. Per-question decisions (A–L)

### A. Where does "OP-centric feed with attribution" semantics live?

**Decision: generic engine `RootIndexedFeed<R, A>` in `nmp-feed`; NIP-10
instance in `nmp-nip01`; follow-set producer in `nmp-nip02`;
composition wiring in `nmp-app-template`.** Unchanged from v3 except for
the composition-root expansion of follow-set (§3-D).

### B. How does Bob's unfollowed OP enter the kernel?

**Decision: through the canonical `Kernel::claim_event` primitive
(`crates/nmp-core/src/kernel/requests/event.rs`), with three rung-1
kernel API additions that make the routing match what v3 promised:**

1. `OneshotApi::request(registry, scope, shape, hints)` — new `hints`
   parameter; `claim_event` passes URI relay TLVs into it. Initial
   `LogicalInterest.hints` populated.
2. `event_claim_released` projection (`BoundedRingBuffer<EventId>`) +
   observer callback the engine registers at construction. EOSE-with-
   no-match release inside `complete_unknown_oneshot` clears
   `event_claims` + `event_claim_requested` AND pushes into the ring.
3. `release_event` calls `release_claim_expansion(primary_id)` (M3).

#### Step-by-step trace of the fetch path (v4, every claim verified)

1. **Alice's reply arrives.** `Kernel::ingest_timeline_event`
   (`crates/nmp-core/src/kernel/ingest/timeline.rs:20-211`) admits it
   because Alice is in `timeline_authors` (the active account follows
   her). Event stored; observer fan-out fires.
2. **`RootIndexedFeed::on_kernel_event` runs.** Resolver returns
   `Some(ThreadPointer::Event { id: bob_op_id, relay:
   alice_relay_hint, kind: None })`.
3. **Follow predicate.** `follow(alice_pubkey) == true`. Reply qualifies.
4. **Engine looks up Bob's OP locally.** `roots.contains(bob_op_id) ==
   false`. Records attribution in `pending_attributions[bob_op_id]`,
   `pending_pointers[bob_op_id] = ThreadPointer::Event { … }`. Emits
   `ClaimRequest::Claim { pointer: ThreadPointer::Event { id:
   bob_op_id, relay: alice_relay_hint, kind: None }, hints: vec![
   RelayHint { url: alice_provenance_relay, source: Provenance {
   event_id: alice_event_id } }, ], consumer_id: "nmp.feed.home" }`.
   Alice's provenance relay is read from the kernel's `event_provenance`
   record for Alice's reply id via `event_lookup` (the lookup callback
   can return the provenance through a sibling accessor, or — simpler —
   the engine encodes Alice's reply-id as part of the `RelayHint`
   payload and lets the kernel side resolve it; the implementer picks
   the cleaner shape).
5. **`nmp-nip01` host adapter** encodes the `ThreadPointer::Event` into
   a `nostr:nevent1…` URI with the relay-hint TLV. Calls
   `nmp_app_claim_event(app, uri, consumer_id)` —
   `crates/nmp-ffi/src/timeline.rs:133`.
6. **`Kernel::claim_event`** parses the URI. Falls into the
   `NostrUri::Event` arm
   (`crates/nmp-core/src/kernel/requests/event.rs:93-109`). Extracts
   `event_id`, optional `author`, and `relays`. Constructs
   `InterestShape { event_ids: {bob_op_id}, limit: Some(1) }`. Refcounts
   `event_claims[bob_op_id]`. Calls **`OneshotApi::request(registry,
   InterestScope::Global, shape, uri_relay_hints_as_RelayHints)`** —
   the new v4 signature; the initial `LogicalInterest.hints` is no
   longer empty. Also calls `register_claim_expansion(primary_id,
   Some(interest_id), uri_author /* None */, uri_relay_hints, now)` —
   unchanged. Enqueues `CompileTrigger::ViewOpened`.
7. **Planner partition dispatcher** at
   `crates/nmp-planner/src/compiler/partition/mod.rs:240-289` — verified
   that `route_hints` runs at line 265 BEFORE the bootstrap-content-
   relays test. With v4's `hints` plumbing the initial REQ goes to
   **Alice's provenance relay (from hints) ∪ bootstrap_content_relays
   (from `route_bootstrap_content`)**. **This is the change codex flagged
   was missing in v3.**
8. **Bob's OP arrives.** Ingest accepts via `is_discovery_oneshot(sub_id)`
   (line 209) — the OneshotApi-registered sub id matches. Event stored;
   observer fan-out fires.
9. **`RootIndexedFeed::on_kernel_event` runs again** for Bob's OP.
   Resolver returns `parent == None` — Bob's note is a root. Engine
   inserts into `roots[bob_op_id]`, drains
   `pending_attributions[bob_op_id]` into `attributions[bob_op_id]`,
   removes `pending_pointers[bob_op_id]`. Emits `ClaimRequest::Release
   { pointer: …, consumer_id: "nmp.feed.home" }` because the root is
   now locally available; host adapter calls `nmp_app_release_event`,
   which (with M3 fix) cleans up `event_claims` AND
   `release_claim_expansion(primary_id)`.
10. **If Bob's OP never arrived.** `OneshotApi`'s EOSE path
    (`complete_unknown_oneshot`) inside the kernel — with v4's
    enhancement — clears `event_claims[bob_op_id]`,
    `event_claim_requested.remove(bob_op_id)`,
    `release_claim_expansion(bob_op_id)`, and pushes `bob_op_id` into
    the `event_claim_released` ring buffer. The engine, which
    registered an observer on the ring at construction, drops
    `pending_attributions[bob_op_id]` and `pending_pointers[bob_op_id]`.
    The attribution disappears from the snapshot (it had nowhere to
    attach). No further action.

#### Address-pointer arm (§3-B-3, corrected per codex)

For `ThreadPointer::Address { coord, relay, kind }` (NIP-22 + NIP-23
roots, post-v1 path):

The host adapter encodes the pointer as a `nostr:naddr…` URI.
`claim_event`'s address arm (verified in
`crates/nmp-core/src/kernel/requests/event.rs:110-155`) constructs
`InterestShape { kinds: {kind}, authors: {pubkey}, tags: {"d":
{identifier}}, limit: Some(1) }` — **NOT `InterestShape::addresses`**.
Routing flows through Outbox (Case A authors) on the author's NIP-65
write relays. v4 doesn't ship this path (NIP-22 is post-v1 per Q5) but
the trace is correct for the eventual `nmp-nip22` instance.

`ThreadPointer::External { uri }` is terminal: the engine never emits
`Claim` for it. The attribution attaches to a surrogate id derived from
the URI hash; the host adapter renders an external-link placeholder.
No Nostr fetch.

### C. Where does "Alice replied" attribution metadata live? (REVISED per Q1)

**Decision: in the engine's bounded `attributions[root_id]` map. The
projection exposes ALL enumerated repliers (bounded only by D5 cap on
the per-root sub-map). No `attribution_total` field — the `Vec<A>`
length IS the count. Each render surface picks its own enumeration
policy.**

This is the user's Q1 answer. The 2026-05-25 display-separation
doctrine says raw data in projections, formatting in renderers. The N=8
+ total cap was a baked-in display decision; v4 removes it.

D5 bound: each `BTreeMap<EventId, A>` is itself bounded — at most
`MAX_ATTRIBUTION_PER_ROOT` entries (proposed default = 64). When the
map is full, the oldest reply (by `reply_created_at`) is evicted. The
outer `BoundedMessageMap<EventId, ...>` is also `MAX_PROJECTION_MESSAGES`
bounded as in v3. Per-root and global D5 caps are independent.

Trait (in `nmp-feed`):

```rust
pub trait AttributionPayload: Clone + Send + Sync + 'static {
    type Profile: Clone + Send + Sync + 'static;
    fn from_reply(
        reply: &KernelEvent,
        follow: &dyn Fn(&str) -> bool,
        profile_for: &dyn Fn(&str) -> Option<Self::Profile>,
    ) -> Option<Self>;
    fn reply_event_id(&self) -> &str;
    fn author_pubkey(&self) -> &str;
    fn reply_created_at(&self) -> u64;
    fn refresh_for_profile(&mut self, profile: &Self::Profile);
}
```

`Nip10ReplyAttribution` instance (in `nmp-nip01`):

```rust
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Nip10ReplyAttribution {
    pub author_pubkey: String,
    pub author_display: AuthorDisplay,
    pub author_display_name: Option<String>,
    pub author_picture_url: Option<String>,
    pub reply_event_id: String,
    pub reply_created_at: u64,
}

impl AttributionPayload for Nip10ReplyAttribution {
    type Profile = ProfileDisplay;
    fn from_reply(reply, follow, profile_for) -> Option<Self> {
        if reply.kind != KIND_SHORT_NOTE { return None; }
        if !follow(&reply.author) { return None; }
        let refs = parse_nip10(&reply.tags);
        if !refs.is_reply() { return None; }
        let profile = profile_for(&reply.author);
        let display = AuthorDisplay::from_profile(&reply.author, profile.as_ref());
        Some(Self { …, author_pubkey: reply.author.clone(), … })
    }
    fn refresh_for_profile(&mut self, profile: &ProfileDisplay) {
        let new = AuthorDisplay::from_profile(&self.author_pubkey, Some(profile));
        self.author_display_name = new.name.clone();
        self.author_picture_url = new.picture_url.clone();
        self.author_display = new;
    }
}
```

`ProfileDisplay` is named in `nmp-nip01` only. `nmp-feed` never names it.

### D. How does the engine know the follow set? (ARCHITECTURE OVERRIDE)

**Decision: composition-root expansion. No `FollowSetLookup` trait. No
`LogicalInterest::SocialTimeline` variant. The user's Q2 answer is
moot.**

The follow-set producer is **`nmp-nip02::ActiveFollowSet`**. It exposes:

```rust
impl ActiveFollowSet {
    pub fn new(app: &NmpApp) -> Arc<Self>;
    pub fn follows(&self) -> Vec<String>;
    pub fn predicate(&self) -> Arc<dyn Fn(&str) -> bool + Send + Sync>;
    pub fn on_change(&self, callback: Box<dyn Fn() + Send + Sync>);
}
```

Implementation: an internal `KernelEventObserver` watches kind:3 events
for the active account; an internal observer of
`Kernel::active_account_handle()` watches account switches. The
internal state is `Arc<RwLock<BTreeSet<String>>>`. On every change,
registered `on_change` callbacks fire.

**`nmp-app-template`** is the composition root:

```rust
pub fn register_op_feed_defaults(app: &NmpApp, viewer: Pubkey) {
    let follow_set = nmp_nip02::ActiveFollowSet::new(app);

    // 1. Initial expansion of the follow set into concrete per-author
    //    LogicalInterests (replaces SocialTimeline planner expansion).
    expand_follow_timeline_interests(app, &follow_set.follows(), viewer.clone());

    // 2. On every follow-set change, re-expand (withdraws stale interests,
    //    installs fresh ones). Mirrors the existing
    //    sync_follow_feed_interests semantics, just driven from the
    //    composition root.
    let app_clone = app.clone();
    let follow_set_for_cb = Arc::clone(&follow_set);
    let viewer_for_cb = viewer.clone();
    follow_set.on_change(Box::new(move || {
        expand_follow_timeline_interests(
            &app_clone,
            &follow_set_for_cb.follows(),
            viewer_for_cb.clone(),
        );
    }));

    // 3. Register the OP-feed engine. The predicate is closure-shaped, no
    //    trait crate.
    nmp_nip01::register_op_feed(
        app,
        viewer,
        follow_set.predicate(),
        kernel_event_lookup(app),
    );
}

fn expand_follow_timeline_interests(app: &NmpApp, follows: &[String], viewer: Pubkey) {
    // Constructs one LogicalInterest per follow (kinds 1, 6, Tailing,
    // limit 200). Registers via the existing dispatch_action surface or
    // an actor command. Mirrors the kernel-side sync_follow_feed_interests
    // body, but the call site is at the composition root, not in the
    // kernel.
    // Actual interest construction reuses planner::interest::LogicalInterest
    // verbatim — no enum conversion, no new variant.
}
```

**Reasoning vs. the user's Q2 (enum conversion):** the user chose enum
conversion because the proposal as-written needed planner-side
`SocialTimeline` expansion. Codex pointed out the planner-side
consumption forces a `FollowSetLookup` trait the planner must name,
which creates a cycle through `nmp-feed → nmp-core → nmp-planner`.
The cleanest fix is to eliminate planner-side expansion entirely:
expand at composition. The user's Q2 was answering the wrong question —
the right question is whether to consume the predicate in the planner
at all. v4 answers "no." No SocialTimeline variant, no enum
conversion, no `FollowSetLookup` trait, no cycle. **The user's "right
not smallest" rule is satisfied** because the resulting graph is
genuinely simpler: zero new trait crates, zero new variants, one
ordinary closure parameter on the engine.

**Why this is "right" not "smallest":** the v3 design (trait in
`nmp-feed`) was the smallest move that named V-45 as the seam — and it
broke compilation. v4's composition-root expansion **finishes V-45**:
it gives every composing app a one-line affordance
(`register_op_feed_defaults`) without forcing the planner to grow a
generic capability. The framework-thesis is strengthened more than the
v2/v3 design strengthened it.

**V-45 status:** the original V-45 issue is closed by this design,
just not through the originally-named `SocialTimeline` mechanism. The
BACKLOG entry needs to record that the V-45 affordance is delivered
through `nmp-app-template::register_op_feed_defaults` instead.

### E. `TimelineBlock::Standalone` lossless reshape

**Decision: unchanged from v3** —

```rust
pub enum TimelineBlock {
    Standalone {
        id: EventId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        root: Option<ThreadPointer>,
    },
    Module {
        events: Vec<EventId>,
        has_gap: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        root: Option<ThreadPointer>,
    },
}
```

**Codex B3-remainder fix: every Rust consumer enumerated.** Verified by
grep against the current tree. Rung 2 patches all of:

- `crates/nmp-threading/src/block.rs` (definition + helper methods)
- `crates/nmp-threading/src/grouper.rs` (`grouper.rs:254, 269, 277,
  296, 367, 394, 438, 558, 566`)
- `crates/nmp-threading/tests/grouper.rs` (`tests/grouper.rs:125, 139,
  261, 450, 474, 486, 508, 522, 537, 544`)
- `crates/nmp-feed/src/types.rs:87-93` (`FeedBlock for TimelineBlock`
  match arm)
- `crates/nmp-nip01/src/timeline_projection/tests.rs` (lines 76, 90,
  91, 108, 109, 139, 164)
- `crates/nmp-nip01/src/meta_timeline/tests.rs` (lines 146, 172)
- `apps/chirp/nmp-app-chirp/tests/end_to_end.rs:130`
- `apps/chirp/chirp-tui/src/timeline.rs:244-265`
- `apps/chirp/chirp-tui/src/timeline/tests.rs` (Standalone JSON
  fixtures)
- `ios/Chirp/Chirp/Bridge/TimelineBlock.swift:7-92` (hand-decoder)
- `ios/Chirp/Chirp/Bridge/ModularTimelineBridge.swift` (pattern matches)
- `ios/Chirp/Chirp/Features/HomeFeedView.swift` (pattern matches)
- `ios/Chirp/Chirp/Components/ModularBlockView.swift` (pattern matches)
- `ios/Chirp/Chirp/Bridge/Generated/KernelTypes.generated.swift`
  (regenerated)
- Swift fixtures under `ios/Chirp/ChirpTests/**` if present
- `crates/nmp-codegen/src/swift_projections_registry.rs:199-203` — NO
  change at rung 2 (binding to `ChirpTimelineSnapshot` transitively
  picks up the shape change)

### F. Doctrine compliance (D0–D14)

| Doctrine | Compliance | Notes |
|---|---|---|
| **D0** | ✅ | `nmp-core` gains: pre-kind:3 buffer (substrate-named), `event_claim_released` projection (substrate-named), `OneshotApi::request` hints parameter (no NIP), `active_timeline_authors()` accessor (existing field, substrate name). NO `FollowSetLookup`, NO `SocialTimeline`. `nmp-feed` gains: `AttributionPayload`, `RootIndexedFeed`, `RootCard`, `RootFeedSnapshot`, `ClaimRequest`. Public API uses only substrate vocabulary; `Profile` is an associated type. Verification: `grep -E 'nip[0-9]+|marmot|ProfileDisplay' crates/nmp-feed/src/` must return zero matches (CI test in rung 3). |
| **D1** | ✅ | Profile mirror via `A::refresh_for_profile`. |
| **D2** | ✅ | Root hydration through `claim_event` → `OneshotApi` → planner (coverage hooks intact). |
| **D3** | ✅ | Root-claim routing through `bootstrap_content_relays ∪ hint_relays` (the kernel's v4 enhancement to `OneshotApi::request`). Per-follow `LogicalInterest`s routed through NIP-65 Outbox (Case A) — unchanged. |
| **D4** | ✅ | Engine's `attributions` is single owner per root. `event_claims[primary_id]` single writer per primary_id. |
| **D5** | ✅ | Every map bounded; visible-window-only snapshot. Per-root attribution sub-map bounded at `MAX_ATTRIBUTION_PER_ROOT`. Pre-kind:3 buffer bounded. Acceptance test in §3-J: 5,000 roots populated; snapshot at limit=80; assert exactly 80 cards + JSON size bound. |
| **D6** | ✅ | `claim_event` returns `Vec::new()` on errors. `event_claim_released` is state, not exception. |
| **D7** | ✅ | Closure-shaped capability (`Arc<dyn Fn(...) -> bool>` for follow predicate, `Arc<dyn Fn(&EventId) -> Option<KernelEvent>>` for event lookup). The engine asks; the wiring decides. |
| **D8** | ✅ | Observer-driven. Pre-kind:3 buffer drain is one event-loop pass when kind:3 lands; not a poll. |
| **D9** | ✅ | `reply_created_at` is signed `event.created_at`. |
| **D10** | n/a | Public kind:1/kind:6 only. |
| **D11** | ✅ | No new bespoke C-ABI symbol. Hydration via existing `nmp_app_claim_event`. |
| **D14** | ✅ | `nmp.feed.home` is a typed projection. |

**ADRs:**
- **ADR-0035** — Generic root-indexed feed engine in `nmp-feed`
  (`RootIndexedFeed<R, A>`, `AttributionPayload`,
  `ClaimRequest(ThreadPointer)`). Records the closure-based predicate
  + event-lookup capability shape.
- **ADR-0036** — Composition-root expansion of follow-set timeline
  interests in `nmp-app-template`. Records why `SocialTimeline` was
  rejected and what replaces V-45.
- Existing 0033 (`nmp-feed-viewport-ffi`) + 0034 (kind-dispatch
  content rendering) are not touched.

### G. Card-payload shape

**Decision: `RootCard<C, A>` and `RootFeedSnapshot<C, A>` with
explicit serialization bounds (M4 fix):**

```rust
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RootCard<C, A>
where
    C: Clone + Serialize,
    A: Clone + Serialize,
{
    pub card: C,
    pub attribution: Vec<A>,           // bounded by MAX_ATTRIBUTION_PER_ROOT
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RootFeedSnapshot<C, A>
where
    C: Clone + Serialize,
    A: Clone + Serialize,
{
    pub cards: Vec<RootCard<C, A>>,
    pub page: Option<FeedPage>,
    pub metrics: Option<FeedWindowMetrics>,
}
```

`attribution_total` field removed (Q1 — `Vec` length IS the total).

### H. Framework reusability

**Decision: `register_op_feed_defaults(app, viewer)` is the single-line
affordance.** A second protocol (`nmp-nip22` covering kind:1111 for ALL
non-kind:1 root kinds — NIP-23, NIP-94, NIP-99, podcasts) composes with
`(R, A)` only; the composition-root expansion logic is generic over
"the set of kinds to register interests for," so `nmp-app-template`
also gains a `register_op_feed_for_comments_defaults(...)` helper
post-v1.

### I. Sequencing

**Decision: 7-PR ladder.** Rung 1 grew to absorb the kernel API
additions. §5/§6 below.

### J. Test surface

#### `nmp-feed` engine tests (~320 LOC)

Synthetic resolver + synthetic payload + synthetic follow predicate +
synthetic event_lookup. Zero NIP imports.

- Reply arrives, root arrives → attribution attaches.
- Reply arrives, root never arrives → `Claim` emitted; on `event_claim_released`
  ring update, engine drops `pending_attributions`.
- Bounded-map eviction → emits `ClaimRequest::Release`.
- Per-root sub-map eviction (`MAX_ATTRIBUTION_PER_ROOT`) — oldest reply
  evicted; engine does NOT emit Release (the root is still claimed by
  remaining attributions; refcount is implicit in the per-root map size).
- Profile event → fan out to attributions + pending_attributions.
- Repost (`resolver.supersedes != None`) → target becomes surfaced root.
- L-2: reply to a kind:6 wrapper, parent locally available; engine
  consults `event_lookup` + `resolver.supersedes`; attribution re-keyed
  to target.
- L-5: e-tag-only repost, target arrives later; engine receives the
  target event and rebuilds the card via `card_builder` (the test
  asserts the card body is non-empty after the target arrives).
- Non-follow reply → dropped.
- D5 visible-window snapshot: 5,000 roots, `limit = 80`, assert exactly
  80 cards + JSON size bound + internal maps at cap.
- Address pointer: synthetic `ThreadPointer::Address`; assert
  `ClaimRequest` carries Address variant; host adapter encodes naddr URI.
- External pointer: terminal; no Claim emitted; attribution attaches
  against URI-hash surrogate.
- **Serde bounds compile test:** assert `RootCard<TimelineEventCard,
  Nip10ReplyAttribution>` and `RootFeedSnapshot<…>` round-trip through
  `serde_json::to_string` + `from_str`.

#### `nmp-nip01` instance tests (~220 LOC)

- `Nip10ReplyAttribution::from_reply` filter chain.
- Profile refresh in place.
- `register_op_feed` composes correctly.
- End-to-end with synthetic kernel: follow's reply + non-followed root
  + kind:0 profile; assert snapshot + `nmp_app_claim_event` URI.
- Repost L-1 through L-5 with the new `event_lookup` callback (L-2 and
  L-5 require the lookup; L-1, L-3, L-4 don't).

#### `nmp-nip02` adapter tests (~100 LOC)

- kind:3 ingest → `ActiveFollowSet::follows()` updates.
- Account-switch (via `active_account_handle()` observation) →
  follow-set resets.
- `on_change` callback fires on both transitions.

#### `nmp-core` kernel tests (~180 LOC, rung 1)

- Pre-kind:3 buffer: kind:1 event from Bob (not in `timeline_authors`)
  arrives; gate drops; buffer captures. Active-account kind:3 arrives
  including Bob; `sync_follow_feed_interests` rebuilds
  `timeline_authors`; buffer drains; Bob's kind:1 fires observer
  fan-out.
- Pre-kind:3 buffer D5 bound: insert `MAX_PROJECTION_MESSAGES + N`
  events; oldest evicted.
- `OneshotApi::request` initial hints: assert the constructed
  `LogicalInterest.hints` is non-empty when called with hints. **Identical
  behavior for non-hint callers** (existing tests with hints=Vec::new()
  pass unchanged — non-regression for every other claim_event caller).
- `event_claim_released` ring buffer: EOSE-no-match path pushes
  primary_id; release_event refcount-to-zero path pushes primary_id.
- `release_event` calls `release_claim_expansion` (assert the
  expansion tracker is gone after release).

#### chirp-tui tests

**DELETE** the partial-chain tests (same list as v3).

**ADD:**
- `RootCard` JSON → `TimelineRow` mapping (no attribution).
- `RootCard` JSON → `TimelineRow` mapping (N=3 attributions; raw
  pubkeys preserved; chirp-tui chooses to render 1).

#### iOS Swift tests

- `TimelineBlock.swift` decodes the new shape.
- `ModularBlockView.swift` continues to render.
- `RootFeedSnapshot` Decodable test.

#### Doctrine-lint test

- `nmp-testing` test: `grep -E 'nip[0-9]+|marmot|profiledisplay'
  crates/nmp-feed/src/` returns zero matches. CI gate.

### K. Startup and identity-change semantics (REVISED per Q7 + codex H2-fiction fix)

The pre-kind:3 cold-start gap is real and recurring (codex confirmed:
`Kernel::new` initializes `timeline_authors` empty; every launch
repopulates from network kind:3). v4 closes it at the source:

**Cold start, follows unknown.** Engine constructed. Active account
signs in via session persistence (existing). The kernel registers a
tailing kind:3 sub for the active account (existing). KernelEvents
flow:

- Any kind:1 / kind:6 event whose author is NOT in `timeline_authors`
  hits `should_store_event` (line 195) and would be dropped. **v4
  enhancement:** before dropping, the kernel pushes the event into a
  bounded pre-kind:3 buffer keyed by event id. The buffer is
  `BoundedMessageMap<EventId, NostrEvent>` with `MAX_PROJECTION_MESSAGES`
  capacity. The active user's own posts and the active user's own
  pubkey are always admitted (existing seed: line 104-109 in
  `contacts.rs` puts the active user's pubkey into `timeline_authors`
  on `prepopulate_seed_contacts`).
- The buffer is *only* consulted when `sync_follow_feed_interests`
  runs (`crates/nmp-core/src/kernel/ingest/contacts.rs:86-119`). At
  the end of that function, after `timeline_authors` is rebuilt, the
  kernel walks the buffer; for every buffered event whose author is
  now in `timeline_authors`, it re-runs the normal ingest path
  (`Kernel::ingest_timeline_event` with the original sub_id). The
  observer fan-out fires; the engine receives the event as ordinary
  ingest. Replayed events are removed from the buffer.
- Buffer entries whose author is still NOT in `timeline_authors`
  after the kind:3 update are dropped from the buffer (they were
  noise — e.g., follow-of-follow events arriving on the firehose).

**This is invisible to the engine.** No new engine API, no scan
API on the kernel surface (codex's "no event store iteration"
constraint is satisfied because the buffer holds raw `NostrEvent`s in
arrival order, not store entries). The engine just sees a slightly-
delayed observer fan-out for events that arrived during the gap window.

**Account switch.** `Kernel::active_account_handle()`
(`crates/nmp-core/src/kernel/mod.rs:1265-1267`) is the existing
substrate push seam. `ActiveFollowSet` registers an internal observer
on the slot; when the active account changes, the adapter's `BTreeSet`
is rebuilt against the new active account's `timeline_authors`. The
`on_change` callback fires; `expand_follow_timeline_interests` runs
with the new follow set. The engine receives a
`reset_for_identity_change()` call from the wiring layer (it's a
method on `RootIndexedFeed` that tears down `roots`, `attributions`,
`pending_attributions`, `pending_pointers`, `profiles`). Pre-kind:3
buffer for the previous account is also cleared.

**Logout.** Same teardown rules as account switch. `ActiveFollowSet`
returns an empty `BTreeSet`; predicate returns `false` for everyone;
engine drops all incoming replies.

**NIP-51 mute-list (post-v1 V-42).** Adapter-side subtraction:
`ActiveFollowSet::predicate()` AND-clauses with `!is_muted(pubkey)`
when the mute list is implemented. Tracked under V-42 in BACKLOG.

### L. Repost edge cases (REVISED per codex H3-remainder)

The engine gains an `event_lookup: Arc<dyn Fn(&EventId) ->
Option<KernelEvent> + Send + Sync>` callback at construction time
(supplied by `nmp-nip01`'s wiring layer; reads the kernel's
read-cache).

**L-1: Followed user reposts an OP.** Resolver returns `supersedes ==
Some(target_id)`. Engine:
1. Insert the kind:6 wrapper into `roots[target_id]` — `card_builder`
   produces the `TimelineEventCard` (existing
   `RenderPayload::from_event` handles embedded reposts and e-tag-only
   reposts).
2. If `target_id` not in `roots`, emit `Claim(Event(target_id), hints)`.

**L-2: Followed user replies to a kind:6 wrapper.** `resolver.parent`
returns `Some(ThreadPointer::Event { id: kind6_id, ... })`. Engine:
1. Consult `event_lookup(kind6_id)`. If returns `Some(parent_event)`
   AND `resolver.supersedes(&parent_event) == Some(target_id)`, re-key
   the attribution to `target_id`.
2. If `event_lookup` returns `None`, hold the attribution against
   `kind6_id` AND emit `Claim(Event(kind6_id), hints)`. When the kind:6
   wrapper arrives, the engine re-runs the supersedes check inside
   `on_kernel_event` (it sees the new root) and re-keys via the
   re-attribution loop.

**L-3: Followed user replies to the original note.** Standard case A.
No `event_lookup` needed.

**L-4: Repost + reply on the same card.** Both display.
`RootCard.card` (the `TimelineEventCard`) carries
`reposted_by: Some(RepostAttribution)` AND
`RootCard.attribution: vec![Nip10ReplyAttribution]`. Rendering rule in
chirp-tui post_list.rs: repost banner above row 1, attribution below
row 1.

**L-5: E-tag-only repost (no embedded inner note).** The kind:6 event
arrives with `e` tag but empty `content`. `RenderPayload::from_event`
returns the empty placeholder card. Engine:
1. Insert empty card into `roots[target_id]`.
2. Emit `Claim(Event(target_id), hints)`.
3. **When the target event later arrives**, the engine receives it as
   normal ingest. The engine detects that `roots[target_id]` already
   exists with the empty card AND `target_id` is the supersedes target
   of an existing kind:6 wrapper in the engine state. Engine calls
   `card_builder` again with both events, replaces the empty card with
   the hydrated one. The card rebuild rule is: on every
   `on_kernel_event` for an event id `e`, if `e` matches an existing
   root's `supersedes` target, the engine looks up the kind:6 wrapper
   via `event_lookup` and rebuilds the card from the pair.

Tests in §3-J cover all five cases.

---

## 4. Doctrine compliance checklist

| Check | Status | Where |
|---|---|---|
| `nmp-core` introduces no NIP-named token | ✅ | Additions: pre-kind:3 buffer (substrate-named field), `event_claim_released` projection (substrate-named), `OneshotApi::request` hints parameter (no NIP), `active_timeline_authors()` accessor, `release_event` calls `release_claim_expansion`. No FollowSetLookup, no SocialTimeline. |
| `nmp-feed` introduces no NIP-named token | ✅ | `AttributionPayload`, `RootIndexedFeed`, `RootCard`, `RootFeedSnapshot`, `ClaimRequest`. `Profile` is associated type. CI grep test. |
| `nmp-router` introduces no NIP-named token | ✅ | Untouched. |
| `nmp-planner` introduces no NIP-named token | ✅ | Untouched (composition-root expansion overrides v3's planner-side seam). |
| No new bespoke `nmp_app_*` C-ABI symbol | ✅ | Existing `nmp_app_claim_event`, `nmp_app_release_event`, `nmp_app_register_event_observer`. No new symbol. |
| Doctrine path correct | ✅ | `docs/product-spec/doctrine.md`. |
| No write-path outside `dispatch_action` | ✅ | Hydration is read. |
| No new poll loop | ✅ | Observer-driven; pre-kind:3 buffer drains synchronously when kind:3 lands. |
| Display-separation | ✅ | Raw pubkeys, raw timestamps, `Option<String>` mirrors. **No `attribution_total` — Vec length is the count.** |
| File-size ceiling | ✅ | Engine ~450 + ~320 tests; instance ~150 + ~220 tests; adapter ~120 + ~100 tests; kernel adds ~180 + ~180 tests. None breach. |
| Single-source-of-truth | ✅ | Engine `attributions` is single owner; `event_claims[primary_id]` is single refcount. |
| V-45 prerequisite | ✅ | Closed by `register_op_feed_defaults` (composition-root expansion replaces SocialTimeline). |
| ADR numbering | ✅ | 0035 + 0036 (0033, 0034 already taken). |
| Crate dep graph | ✅ | New edges: `nmp-nip02 → nmp-feed` NOT needed (nmp-nip02 has no `FollowSetLookup` trait to implement); `nmp-app-template → nmp-nip02` (already exists); `nmp-app-template → nmp-nip01` (already exists). NO `nmp-planner → nmp-feed` cycle. |
| F-05 codegen | ⚠ | TimelineBlock shape (rung 2) + RootFeedSnapshot (rung 5) regenerate Swift Decodables. |
| Doctrine-lint scoped | ✅ | `cargo test -p nmp-testing --test doctrine_lint_smoke` + new `op_feed_doctrine_lint` test. |
| Crate-boundary spec update | ⚠ | Rung 7 updates `nmp-feed` row (charter expands to OP-centric engine + the closure-shaped predicate / event_lookup capabilities) and `nmp-nip02` row (gains ActiveFollowSet producer). |

---

## 5. Concrete change list (file-by-file)

### Stage 0 — Kernel API additions (rung 1)

> Five small substrate additions that close codex's remaining gaps and
> deliver Q7 (pre-kind:3 replay). All substrate-named.

| File | Change | LOC ± |
|---|---|---|
| `crates/nmp-core/src/kernel/types.rs` | Add typed `active_timeline_authors() -> Vec<String>` accessor. | +15 |
| `crates/nmp-core/src/kernel/ingest/timeline.rs` | Pre-kind:3 buffer: when `should_store_event` returns `false` due to `!timeline_authors.contains(author)` AND the event is kind:1 or kind:6, push into `Kernel::pre_kind3_buffer` (a new `BoundedMessageMap<EventId, NostrEvent>` field) instead of dropping. | +40 |
| `crates/nmp-core/src/kernel/ingest/contacts.rs` | At end of `sync_follow_feed_interests`, walk `pre_kind3_buffer` and re-run `ingest_timeline_event` for entries whose author is now in `timeline_authors`. Drop the rest. | +30 |
| `crates/nmp-core/src/kernel/mod.rs` | Add `pre_kind3_buffer: BoundedMessageMap<EventId, NostrEvent>` field; clear it on identity-change. | +10 |
| `crates/nmp-core/src/subs/oneshot.rs` | `OneshotApi::request` gains `hints: Vec<RelayHint>` parameter; populate `LogicalInterest.hints` from it. Update every caller. | +15 / -3 |
| `crates/nmp-core/src/kernel/requests/event.rs` | `claim_event` passes URI relay hints into `OneshotApi::request` as initial `LogicalInterest.hints` (in addition to existing `register_claim_expansion`). `release_event` calls `release_claim_expansion(&primary_id)` at the end of the refcount-to-zero arm. | +18 / -2 |
| `crates/nmp-core/src/kernel/oneshot/complete.rs` (or wherever `complete_unknown_oneshot` lives) | EOSE-no-match path clears `event_claims` + `event_claim_requested` for the primary_id AND pushes into `event_claim_released` ring. | +20 |
| `crates/nmp-core/src/kernel/types.rs` | Add `event_claim_released: BoundedRingBuffer<EventId>` field + projection accessor. | +25 |
| `crates/nmp-core/src/kernel/event_observer.rs` | Allow observers to register a `RawEventObserver`-shaped callback for `event_claim_released` ring updates. | +30 |
| `crates/nmp-core/src/kernel/types_tests.rs` + sibling test files | Tests for pre-kind:3 buffer, initial hints, no-match release. | +180 |

### Stage 1 — `nmp-threading::TimelineBlock` lossless + all consumers (rung 2)

| File | Change | LOC ± |
|---|---|---|
| `crates/nmp-threading/src/block.rs` | Reshape `Standalone` → `Standalone { id, root }`. | +25 / -10 |
| `crates/nmp-threading/src/grouper.rs` | Fix `grouper.rs:367` chain-length-1 root preservation. Update lines 254, 269, 277, 296, 394, 438, 558, 566. | +20 / -15 |
| `crates/nmp-threading/tests/grouper.rs` | Update tests at lines 125, 139, 261, 450, 474, 486, 508, 522, 537, 544; add new lossless-shape test. | +50 / -20 |
| `crates/nmp-feed/src/types.rs` | Update `FeedBlock for TimelineBlock` at lines 87-93. | +5 / -3 |
| `crates/nmp-nip01/src/timeline_projection/tests.rs` | Update Standalone test fixtures at lines 76, 90, 91, 108, 109, 139, 164. | +15 / -15 |
| `crates/nmp-nip01/src/meta_timeline/tests.rs` | Update lines 146, 172. | +5 / -5 |
| `crates/nmp-nip01/src/timeline_projection.rs` | Pattern-match new shape (read-only). | +5 / -3 |
| `apps/chirp/nmp-app-chirp/tests/end_to_end.rs` | Update line 130. | +3 / -2 |
| `apps/chirp/chirp-tui/src/timeline.rs` | Update `ids_from_block` to read object shape. | +20 / -10 |
| `apps/chirp/chirp-tui/src/timeline/tests.rs` | Update Standalone JSON fixtures. | +25 / -20 |
| `ios/Chirp/Chirp/Bridge/TimelineBlock.swift` | Rewrite Standalone decode to object form; update enum associated values. | +30 / -10 |
| `ios/Chirp/Chirp/Bridge/ModularTimelineBridge.swift`, `HomeFeedView.swift`, `ModularBlockView.swift` | Update pattern matches. | +18 / -8 |
| `ios/Chirp/Chirp/Bridge/Generated/KernelTypes.generated.swift` | Regenerated. | varies |
| Swift fixtures under `ios/Chirp/ChirpTests/**` | Update. | varies |

### Stage 2 — `nmp-feed` generic engine (rung 3)

| File | Change | LOC ± |
|---|---|---|
| `crates/nmp-feed/src/root_indexed.rs` | **NEW** — `trait AttributionPayload<Profile>`, `struct RootIndexedFeed<R, A>`, `RootCard<C, A>`, `RootFeedSnapshot<C, A>`, `ClaimRequest`. Closure-shaped `follow` + `event_lookup`. Implements `KernelEventObserver` + `FeedController` + observer for `event_claim_released`. | +450 |
| `crates/nmp-feed/src/root_indexed/tests.rs` | **NEW** — engine tests with synthetic resolver + payload + predicate + event_lookup. Covers every arrival case, eviction, repost L-2 / L-5 via event_lookup, D5 visible-window assertion, serde round-trip. | +320 |
| `crates/nmp-feed/src/lib.rs` | Export new items. | +12 |
| `crates/nmp-testing/tests/op_feed_doctrine_lint.rs` | **NEW** — CI grep gate. | +30 |
| `docs/decisions/0035-generic-root-indexed-feed-engine.md` | **NEW ADR**. | +250 |

### Stage 3 — `nmp-nip02` follow-set producer (rung 4)

| File | Change | LOC ± |
|---|---|---|
| `crates/nmp-nip02/src/active_follow_set.rs` | **NEW** — `ActiveFollowSet`. Internal observer over kind:3 ingest. Internal observer over `Kernel::active_account_handle()`. Exposes `follows()`, `predicate()`, `on_change()`. NO `FollowSetLookup` trait. | +120 |
| `crates/nmp-nip02/src/active_follow_set/tests.rs` | **NEW** — kind:3 ingest, account switch, logout, on_change firing. | +100 |
| `crates/nmp-nip02/src/lib.rs` | Export. | +6 |
| `docs/decisions/0036-composition-root-followset-expansion.md` | **NEW ADR** — why no planner SocialTimeline, why composition-root. | +200 |

### Stage 4 — `nmp-nip01` OP-feed instance (rung 5)

| File | Change | LOC ± |
|---|---|---|
| `crates/nmp-nip01/src/op_feed/mod.rs` | **NEW** — module surface. | +20 |
| `crates/nmp-nip01/src/op_feed/attribution.rs` | **NEW** — `Nip10ReplyAttribution: AttributionPayload<Profile = ProfileDisplay>`. | +100 |
| `crates/nmp-nip01/src/op_feed/wiring.rs` | **NEW** — `register_op_feed(app, viewer, predicate, event_lookup)`. Constructs engine, registers observer + snapshot projection at `"nmp.feed.home"`, installs claim sink mapping `ClaimRequest::Claim/Release` → `nmp_app_claim_event`/`nmp_app_release_event`, installs `event_claim_released` observer forwarding to the engine. | +150 |
| `crates/nmp-nip01/src/op_feed/tests.rs` | **NEW** — instance tests + repost L-1 through L-5 + claim URI encoding. | +260 |
| `crates/nmp-nip01/src/lib.rs` | Export. | +12 |

### Stage 5 — `nmp-app-template` composition (rung 6)

| File | Change | LOC ± |
|---|---|---|
| `crates/nmp-app-template/src/op_feed_defaults.rs` | **NEW** — `register_op_feed_defaults(app, viewer)`. Constructs `ActiveFollowSet`, calls `expand_follow_timeline_interests`, registers `on_change` callback re-running expansion, calls `nmp_nip01::register_op_feed`. | +120 |
| `crates/nmp-app-template/src/expand_follow_interests.rs` | **NEW** — `expand_follow_timeline_interests(app, follows, viewer)`. Builds per-follow `LogicalInterest` (kinds 1, 6, Tailing, limit 200); registers via the existing actor command surface. Mirrors `kernel::ingest::contacts::sync_follow_feed_interests` body. | +90 |
| `crates/nmp-app-template/src/lib.rs` | Export. | +8 |
| `crates/nmp-app-template/tests/op_feed_defaults_test.rs` | **NEW** — integration test: register defaults; feed events; assert snapshot. | +180 |

### Stage 6 — Chirp wiring (rung 7)

| File | Change | LOC ± |
|---|---|---|
| `apps/chirp/nmp-app-chirp/src/ffi/register.rs` | Replace `ModularTimelineProjection` registration with `nmp_app_template::register_op_feed_defaults(app, viewer)`. Drop ~30 LOC of hand-rolled follow-set wiring. | +5 / -50 |
| `apps/chirp/chirp-tui/src/timeline.rs` | Rewrite `TimelineRow::from_snapshot` for `RootFeedSnapshot`. Delete `ids_from_block`, `event_root_mismatches_top`, `is_partial_chain_head`. Add `thread_attribution: Vec<RowReplyAttribution>` field. | +60 / -100 |
| `apps/chirp/chirp-tui/src/ui/post_list.rs` | Delete ↳ indicator. Add attribution row (chirp-tui's display policy: render the most recent 1 with "and N others"). Apply L-4 rule. | +50 / -25 |
| `apps/chirp/chirp-tui/src/timeline/tests.rs` | Delete partial-chain tests; add RootCard mapping tests. | +220 / -160 |
| `apps/chirp/chirp-tui/src/render_intents.rs`, `media_cache.rs` | Drop `is_partial_chain_head: false`. | -2 |
| `ios/Chirp/Chirp/Bridge/Generated/*.swift` | Regenerated for `RootFeedSnapshot` + `Nip10ReplyAttribution`. | varies |
| `crates/nmp-codegen/src/swift_projections_registry.rs` | Bind `nmp.feed.home` to new `OpFeedSnapshot` Swift type. | +6 / -3 |
| `docs/architecture/crate-boundaries.md` | Row updates for `nmp-feed` and `nmp-nip02`. | +25 |
| `docs/BACKLOG.md` | Close V-45 (resolved via composition-root expansion). Add V-59 (this work). Add V-60 (mute-list interaction post-v1, per Q5 + §3-K). Add V-61 (NIP-22 instance post-v1, per Q5). | varies |
| `docs/plan.md` | Bump framework-thesis status. | +5 |

**Total worktree footprint:** ~7 PRs, ~2,400 LOC net add (engine + tests
+ instance + adapter + composition + kernel API + ADRs), ~310 LOC delete.

---

## 6. Sequencing plan — 7 rungs

Each rung independently mergeable, leaves master green.

1. **Rung 1 — Stage 0 — Kernel API additions.** Pre-kind:3 buffer,
   `event_claim_released` projection, `OneshotApi::request` hints,
   `release_event` calls `release_claim_expansion`,
   `active_timeline_authors` accessor. Pure substrate additions. No
   consumer yet. Master state: unchanged user-facing behavior; faster
   discovery for every existing `claim_event` caller (URI hints land
   on FIRST REQ).
2. **Rung 2 — Stage 1 — Lossless `TimelineBlock` + all consumers.**
   In-PR patches every cited consumer. Master: home feed unchanged in
   behavior; previously-invisible Standalone roots now flag correctly
   in the existing ↳ indicator.
3. **Rung 3 — Stage 2 — `nmp-feed` engine.** ADR-0035. No consumer yet.
   CI grep gate enforces D0. Master: unchanged.
4. **Rung 4 — Stage 3 — `nmp-nip02` `ActiveFollowSet`.** Producer only,
   no consumer. Master: unchanged.
5. **Rung 5 — Stage 4 — `nmp-nip01` instance.** ADR-0036. Composes the
   engine with the NIP-10 resolver + payload + adapter predicate.
   Master: unchanged (Chirp not wired yet).
6. **Rung 6 — Stage 5 — `nmp-app-template` composition.** One-line
   affordance lands. Composing apps get the feed. Master: unchanged
   (Chirp not wired yet).
7. **Rung 7 — Stage 6 — Chirp cut-over.** Product-visible PR. chirp-tui
   + iOS Swift consume `RootFeedSnapshot`. Live validation against
   `wss://relay.damus.io`. Master: Chirp shows the OP-centric home
   feed.

**Parallelization:** rungs 2 + 3 + 4 + 5 are largely independent on the
file level (different crates). Sensible execution order: 1 → (2 ‖ 3 ‖
4) → 5 → 6 → 7.

**Wall-clock estimate:** 8-10 days for a single agent.

---

## 7. Residual concerns tracked as BACKLOG TODOs

> Per the user's "no further revision rounds" rule, anything not
> resolved in v4 lands as a BACKLOG TODO, not as v5.

- **V-60:** NIP-51 mute-list interaction with the OP feed. Adapter-side
  subtraction in `ActiveFollowSet::predicate`. Post-v1.
- **V-61:** NIP-22 (kind:1111) `RootIndexedFeed` instance covering ALL
  non-kind:1 root kinds. ~150 LOC; zero engine changes. Post-v1.
- **V-62:** Retire `timeline_authors` field from `nmp-core`. It is
  itself a social concept in substrate; `ActiveFollowSet` should
  eventually own the canonical view. Out of scope for v4. Tracked debt.
- **V-63:** `claim_event` EOSE-driven release vs. host-release-driven
  refcount mismatch. Codex §6 out-of-scope observation. Cleanup not
  load-bearing for OP feed. Post-v1.
- **V-64:** `event_provenance` accessor for the engine. Currently the
  engine constructs `RelayHint::Provenance` for the reply event; the
  kernel has the provenance data internally. Cleanest shape is a typed
  `Kernel::event_provenance(event_id) -> Option<&str>` accessor.
  Tracked as a sub-item of rung 1 for the implementer to evaluate (if
  the simpler "host adapter passes Alice's reply id as a hint and the
  kernel resolves it" works, skip V-64).

---

## 8. Backlog entry (final draft for `docs/BACKLOG.md`)

```markdown
### V-59 · Home feed is thread-roots-only with reply attribution [HIGH · v1 PRODUCT-MODEL FIX]

**Status:** spec FINAL 2026-05-27d, ready for implementation. Full design at
[`docs/perf/op-centric-feed-architecture.md`](perf/op-centric-feed-architecture.md).

**Evidence:** today's home feed (chirp-tui + Chirp iOS) shows replies as
standalone rows; PR #710 added a ↳ partial mitigation. Product model is
**feed = thread roots only; follows' replies attribute back to their root**.

**Architectural shape:** generic engine `RootIndexedFeed<R, A>` in `nmp-feed`
parameterized over `ParentResolver` + `AttributionPayload<Profile=…>` +
closure-shaped follow predicate + event-lookup callback. NIP-10 instance in
`nmp-nip01`. Follow-set producer in `nmp-nip02`. Composition root in
`nmp-app-template`. Kernel additions (rung 1): pre-kind:3 ingest buffer,
`event_claim_released` projection, `OneshotApi::request` initial hints,
`release_event` calls `release_claim_expansion`. Root hydration via existing
`Kernel::claim_event` / `nmp_app_claim_event` — no bespoke action.

**Closes V-45** (via composition-root expansion of follow-set timeline
interests in `nmp-app-template`; no planner-side `SocialTimeline` variant).

**Recommended action:** 7-rung PR ladder per §5. Net ~+2,400 LOC,
~-310 LOC. Two ADRs (0035 + 0036).

**User decisions resolved:** Q1 (raw attribution, no cap), Q2 (n/a —
SocialTimeline deleted), Q3 (reposts stay, full case list in §3-L), Q4
(self-replies promote), Q5 (NIP-22 post-v1), Q6 (D1-strict latency), Q7
(pre-kind:3 buffer in kernel — replay through normal ingest).

**Post-v1 follow-ups:** V-60 (mute interaction), V-61 (NIP-22 instance),
V-62 (retire `timeline_authors`), V-63 (`claim_event` release semantics
cleanup), V-64 (`event_provenance` accessor).
```

---

## 9. Implementer notes — read before writing code

- **Do not** add a new bespoke `nmp_app_*` C-ABI symbol. Use
  `nmp_app_claim_event`, `nmp_app_release_event`,
  `nmp_app_register_event_observer`.
- **Do not** parse NIP-10 inside `nmp-core`. Decoder lives in
  `nmp-nip01`.
- **Do not** import any `nmp-nip*` crate from `nmp-feed`. CI grep
  enforces.
- **Do not** add a `FollowSetLookup` trait or `LogicalInterest::SocialTimeline`
  variant. Both were earlier proposals; composition-root expansion
  replaces them. If you find yourself reaching for either, re-read
  §3-D.
- **Do not** accept dual `Standalone` JSON shapes. Rung 2 patches every
  consumer.
- **Do not** poll. Kernel push, observer callbacks, `Arc<RwLock<_>>`
  snapshot reads — never `sleep` + check.
- **Do** read the precedent: `claim_event`,
  `crates/nmp-core/src/subs/oneshot.rs`, `partition/mod.rs:240-289`,
  `crates/nmp-nip01/src/visible_relations.rs` (precedent only —
  v4 does NOT use the bespoke-action pattern),
  `crates/nmp-nip01/src/timeline_projection.rs` for `BoundedMessageMap`
  + `refresh_author_cards`.
- **Doctrine path:** `docs/product-spec/doctrine.md`.

---

## Appendix A — Codex v2 findings and v4 resolutions

| Finding | Status | Where addressed |
|---|---|---|
| B4 (v3-introduced): `FollowSetLookup` in `nmp-feed` creates planner cycle | **Resolved** | `FollowSetLookup` trait deleted. Engine takes closure-shaped `Arc<dyn Fn(&str) -> bool + Send + Sync>`. Producer lives in `nmp-nip02` as `ActiveFollowSet`. No planner consumption. §3-D. |
| B2-remainder: `OneshotApi::request` hardcodes `hints: Vec::new()` | **Resolved** | Rung 1 expands `OneshotApi::request` signature to accept initial `hints`. `claim_event` populates them from URI relay TLVs. Verified against `crates/nmp-core/src/subs/oneshot.rs:120` and `crates/nmp-core/src/kernel/requests/event.rs:83-103`. §3-B step 7. |
| B2-remainder: no engine-observable no-match release signal | **Resolved** | Rung 1 adds `event_claim_released: BoundedRingBuffer<EventId>` projection. EOSE-no-match in `complete_unknown_oneshot` clears `event_claims` + pushes to ring. Engine registers a substrate-grade callback. §3-B step 10. |
| B2-remainder: store-gate via `claim_expansion_match_author` is wrong description | **Resolved** | §3-B step 8 corrected to `is_discovery_oneshot(sub_id)`. |
| B3-remainder: missing Rust consumers of `Standalone` | **Resolved** | §5 Stage 1 enumerates every consumer: `nmp-feed/src/types.rs`, `nmp-nip01/src/timeline_projection/tests.rs`, `apps/chirp/nmp-app-chirp/tests/end_to_end.rs`, `nmp-nip01/src/meta_timeline/tests.rs`, `chirp-tui/src/timeline/tests.rs` fixtures, and `grouper.rs` self-uses. Grep-verified. |
| H2-remainder: `timeline_authors` not LMDB-restored on cold start | **Resolved** | v4 stops claiming LMDB restore. Pre-kind:3 buffer (rung 1) closes the gap by buffering kind:1/6 events that miss the `timeline_authors` gate and replaying them after `sync_follow_feed_interests`. §3-K rewritten. |
| H2-remainder: `Kernel::active_account_pubkey()` / `KernelAccountChanged` fiction | **Resolved** | `Kernel::active_account_handle()` (`crates/nmp-core/src/kernel/mod.rs:1265-1267`) is the real push seam. Adapter observes the slot. §3-K. |
| H3-remainder: L-2 / L-5 require event lookup | **Resolved** | Engine gains `event_lookup: Arc<dyn Fn(&EventId) -> Option<KernelEvent> + Send + Sync>` callback. §3-L rewritten with explicit lookup logic for L-2 and L-5. Tests added in §3-J. |
| M3: `release_event` doesn't call `release_claim_expansion` | **Resolved** | Rung 1 adds the call. §5 Stage 0. |
| M4: serialization bounds implicit | **Resolved** | §3-G declares `C: Clone + Serialize`, `A: Clone + Serialize`. |
| M5: §5 vs §7-Q2 contradiction | **Resolved (architecture override)** | `SocialTimeline` deleted entirely. No internal contradiction. §3-D. |
| §3-B-3 inaccuracy: naddr uses `kinds + authors + #d`, not `addresses` | **Resolved** | §3-B-3 corrected per `crates/nmp-core/src/kernel/requests/event.rs:110-155`. |
| §6 LMDB-restored claim | **Resolved** | Removed; replaced with honest pre-kind:3 buffer behavior. |
| §6 `LogicalInterest::SocialTimeline` may be unnecessary | **Adopted** | Composition-root expansion replaces it. §3-D. |
| §6 `timeline_authors` is already a substrate social cache | **Acknowledged** | V-62 in BACKLOG (retire post-v1). |
| §6 `claim_event` EOSE vs host-release mismatch | **Acknowledged** | V-63 in BACKLOG. |
| Q1 user answer: raw data, display-layer decision | **Adopted** | `attribution_total` deleted; `Vec<A>` length is the count; chirp-tui renders 1, iOS renders N. §3-C, §3-G. |
| Q2 user answer: convert `LogicalInterest` to enum | **Moot** | No `SocialTimeline` variant exists in v4; nothing to convert. §3-D documents the override and reasoning. |
| Q5 user confirmation: NIP-22 post-v1 | **Adopted** | V-61 in BACKLOG. |
| Q7 user answer: add replay capability | **Adopted (in kernel, not engine)** | Pre-kind:3 buffer in kernel (rung 1) replays through normal ingest. Engine needs no replay API. §3-K. |
