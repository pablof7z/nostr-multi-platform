# OP-Centric Home Feed — Architecture Proposal

> **Status:** design proposal. Not implementation. ADR-grade rigor.
> **Author:** Architect (Serena Blackwood)
> **Date:** 2026-05-27 (revised same-day after user correction)
> **Scope:** redefine the chirp-tui / Chirp / NMP home-feed model from "threaded
> notes (replies + roots) over the follow-set" to **"thread roots only, with
> follow-replies as attribution metadata on their root."** Includes the
> protocol-level mechanics required to make a non-followed root appear in the
> feed when a followed user replies to it. **Delivers the OP-centric feed as
> a generic primitive in `nmp-feed`, with `nmp-nip01` as a thin protocol
> instance.**
>
> **Revision note (2026-05-27, post-review):** the first draft scoped the
> "generic factoring" of the projection state machine as post-v1 follow-up.
> That was the smallest-now move, not the right one. This revision delivers
> the generic engine `RootIndexedFeed<R, A>` in `nmp-feed` as part of the
> first cut. NIP-01 becomes a ~100-LOC instance. Future protocols (NIP-22
> covering all non-kind:1 comment trees) compose the same engine.

---

## 1. Executive summary

The home feed becomes a **stream of thread roots** produced by a new generic
engine `RootIndexedFeed<R: ParentResolver, A: AttributionPayload>` in
`nmp-feed`. Each root carries an optional list of reply-attribution payloads
naming who replied (and when). The engine knows nothing about NIP-10 or any
other protocol; it is parameterized over the two protocol-shaped concerns:

- **`ParentResolver`** (already exists in `nmp-threading`) — decodes the
  reply / root pointer from a `KernelEvent`.
- **`AttributionPayload`** (new trait in `nmp-feed`) — describes how a reply
  event becomes a sibling attribution record on the root's card, plus the
  in-place profile refresh that `kind:0` ingest triggers.

`nmp-nip01` then provides the NIP-10 instance: `Nip10Resolver` (already
exists) + a new `Nip10ReplyAttribution` (raw-pubkey, raw-timestamp, optional
profile mirrors per the 2026-05-25 display-separation doctrine) +
~100 LOC of wiring (the registration helper that constructs
`RootIndexedFeed<Nip10Resolver, Nip10ReplyAttribution>` and registers the
root-claim action). A future `nmp-nip22` crate adds a second instance over
kind:1111 comment trees — which by Nostr-protocol definition covers ALL
non-kind:1 reply structures (comments on NIP-23 longform articles, NIP-94
file metadata, NIP-99 classified listings, podcasts, etc.) — without
adding new state-machine code.

Four cooperating mechanisms power the system:

1. **`Nip10Resolver::root` becomes lossless.** Bug #1 — `grouper.rs:367`
   dropping the root pointer for 1-event chains — is fixed by reshaping
   `TimelineBlock::Standalone` to carry the `Option<ThreadPointer>` root
   (mirroring `Module.root`). The threading library now structurally tells
   downstream consumers "this is a reply to X; X was/wasn't locally absent".
   No semantic change to the existing thread-detail consumers.
2. **`RootIndexedFeed<R, A>` engine in `nmp-feed`.** Owns the roots map, the
   attributions index, and the orphan buffer for replies-arriving-before-
   roots. Pure generic CPU + map state; no protocol decoder runs inside it
   beyond the trait calls. Emits a snapshot type `RootFeedSnapshot<C, A>`
   that composes with the existing `FeedWindowState` / `page_for_request`
   machinery.
3. **`nmp-nip01` thin instance.** Registers
   `RootIndexedFeed<Nip10Resolver, Nip10ReplyAttribution>` as a
   `KernelEventObserver`, plus the action `nmp.nip01.thread_root.claim`
   that hydrates non-followed roots via a `OneShot + Global +
   event_ids:[root_id]` `LogicalInterest` (the existing PD-033-C
   discovery path).
4. **`LogicalInterest::SocialTimeline` (V-45) co-delivered.** Substrate
   seam for "the follow set's kind:1/6 stream." The view module declares
   the seam; the planner expands it via the new generic capability
   `FollowSetLookup` (substrate-honest, no NIP-02 names in the lookup
   interface).

Net effect: `nmp-core` D0-clean, `nmp-feed` becomes the canonical OP-feed
primitive, `nmp-nip01` is the first instance, and any future protocol
(`nmp-nip22` for kind:1111 comment trees) composes the same engine with
~100 LOC of resolver + payload + wiring. Every cross-crate side-effect
routes through `dispatch_action`, the `EventIngestDispatcher` (V-40), or
the `KernelEventObserver` registry — no new bespoke C-ABI surface.

---

## 2. Architecture diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  PRODUCT BEHAVIOR — chirp-tui left pane (and Chirp iOS home feed)           │
│                                                                             │
│   ┌──────────────────────────────────────────────────────────────────────┐  │
│   │ Bob (not followed)  ·  2h ago                                        │  │
│   │ Building something interesting with Marmot...                        │  │
│   │ ↳ Alice replied · Carol replied                                      │  │
│   │ ❤ 12  ↻ 1  💬 4  ⚡ 3                                                │  │
│   └──────────────────────────────────────────────────────────────────────┘  │
│   ┌──────────────────────────────────────────────────────────────────────┐  │
│   │ Alice (followed)  ·  3h ago                                          │  │
│   │ Just shipped a thing.                                                │  │
│   │ ❤ 4  ↻ 0  💬 2  ⚡ 0                                                 │  │
│   └──────────────────────────────────────────────────────────────────────┘  │
│                                                                             │
└────────────────────────────────────▲────────────────────────────────────────┘
                                     │ raw JSON snapshot
┌────────────────────────────────────┴────────────────────────────────────────┐
│  Layer 4 — nmp-nip01 (THIN — ~100 LOC of glue)                              │
│                                                                             │
│   Nip10Resolver: ParentResolver                  (existing)                 │
│   Nip10ReplyAttribution: AttributionPayload      (NEW — ~80 LOC)            │
│     ├── pubkey, reply_event_id, reply_created_at (raw)                      │
│     ├── author_display, author_display_name, author_picture_url (Option)    │
│     └── refresh_for_profile(&kind0_profile)                                 │
│                                                                             │
│   register_op_feed(app, viewer) → wires:                                    │
│     ├── construct RootIndexedFeed<Nip10Resolver, Nip10ReplyAttribution>     │
│     ├── register as KernelEventObserver (ingest fan-out)                    │
│     ├── register snapshot key "nmp.feed.home" (output)                      │
│     └── register thread_root_claim_actions (dispatch_action surface)        │
│                                                                             │
│   nmp.nip01.thread_root.claim ActionModule       (Claim / Release shape,    │
│     mirrors visible_relations.rs precedent)                                 │
└─────────────────────────────────────────────────────────────────────────────┘
                                     ▲ KernelEvent fan-out + action dispatch
┌────────────────────────────────────┴────────────────────────────────────────┐
│  Layer 4 — nmp-feed (GENERIC ENGINE — ~400 LOC NEW)                         │
│                                                                             │
│   trait AttributionPayload                                                  │
│     fn from_reply(reply, follow_set, profile_lookup) → Option<Self>         │
│     fn reply_event_id(&self) → &str                                         │
│     fn author_pubkey(&self) → &str                                          │
│     fn reply_created_at(&self) → u64                                        │
│     fn refresh_for_profile(&mut self, profile)                              │
│                                                                             │
│   RootIndexedFeed<R: ParentResolver, A: AttributionPayload>                 │
│     struct Inner {                                                          │
│       resolver: R,                                                          │
│       roots: BoundedMessageMap<EventId, RootCard<C, A>>,                    │
│       attributions: BoundedMessageMap<EventId, BTreeMap<EventId, A>>,       │
│       pending_attributions: BoundedMessageMap<EventId,                      │
│                                BTreeMap<EventId, A>>,                       │
│       window: FeedWindowState,                                              │
│       follow_set: Arc<dyn FollowSetLookup>,                                 │
│       profiles: BoundedMessageMap<Pubkey, ProfileDisplay>,                  │
│     }                                                                       │
│                                                                             │
│     impl KernelEventObserver { on_kernel_event(evt):                        │
│       1. delegate to resolver.parent(evt) / resolver.root(evt) /            │
│          resolver.supersedes(evt) — no protocol logic in the engine         │
│       2. branch on the resolver's verdict:                                  │
│          • root-shaped (parent == None) → insert in roots, flush            │
│            pending_attributions for this id                                 │
│          • reply-shaped AND viewer.is_followed(evt.author) →                │
│              extract root_id from resolver.root() or .parent()              │
│              build A via A::from_reply(evt, follow_set, profiles)           │
│              record in attributions[root] OR pending_attributions[root]     │
│              emit ClaimRequest(root_id) if root absent                      │
│          • repost-shaped (resolver.supersedes != None) → target becomes     │
│            surfaced root, repost attribution attaches (RepostAttribution    │
│            on the underlying card C, orthogonal to A)                       │
│          • non-follow reply / repost → drop (never feed row, never attr)    │
│       3. if evt is kind:0 → refresh profile mirrors via A.refresh_for_profile
│     }                                                                       │
│                                                                             │
│     fn snapshot() → RootFeedSnapshot<C, A> {                                │
│       blocks/cards composed via existing nmp-feed::window helpers           │
│     }                                                                       │
│                                                                             │
│   ClaimRequest                                                              │
│     (typed value the engine emits when an attribution arrives for an        │
│      unknown root; the host wiring layer turns this into a                  │
│      dispatch_action("…thread_root.claim", …) call — the engine itself      │
│      stays free of action-system imports)                                   │
└─────────────────────────────────────────────────────────────────────────────┘
                                     ▲ KernelEvent + interest registration
┌────────────────────────────────────┴────────────────────────────────────────┐
│  Layer 3 — nmp-core (substrate)                                             │
│                                                                             │
│   KernelEventObserver registry        (existing)                            │
│   EventIngestDispatcher                (existing, V-40 seam)                │
│   ActionModule registry                (existing)                           │
│   LogicalInterest::SocialTimeline      ← NEW, V-45 co-delivery              │
│   FollowSetLookup capability           ← NEW, V-45 supporting plumbing      │
│                                                                             │
│   Substrate vocabulary stays NIP-clean. The engine in nmp-feed compiles     │
│   without ever naming "nip01" or "nip10". Doctrine-lint banned tokens:      │
│   nothing introduced.                                                       │
└─────────────────────────────────────────────────────────────────────────────┘
                                     ▲ logical interests
┌────────────────────────────────────┴────────────────────────────────────────┐
│  Layer 2 — nmp-planner                                                      │
│                                                                             │
│   SocialTimeline expansion at compile time:                                 │
│       • Look up follows via FollowSetLookup                                 │
│       • Emit one InterestShape per follow (kinds 1, 6), routed by           │
│         NIP-65 outbox (Case A authors) → existing path                      │
│                                                                             │
│   event_ids one-shot path (existing, PD-033-C):                             │
│       • OneShot + Global + event_ids → Case D, indexer-eligible             │
│         when no relay hint is provided                                      │
└─────────────────────────────────────────────────────────────────────────────┘
                                     ▲ KernelEvents
┌────────────────────────────────────┴────────────────────────────────────────┐
│  Layer 4 — nmp-threading (REVISED)                                          │
│                                                                             │
│   ParentResolver trait                  (existing, unchanged)               │
│   Grouper<R: ParentResolver>            (existing, untouched —              │
│                                          thread-detail engine)              │
│   TimelineBlock                                                             │
│     ├── Standalone { id, root: Option<ThreadPointer> }   ← LOSSLESS         │
│     └── Module { events, has_gap, root: Option<ThreadPointer> }             │
└─────────────────────────────────────────────────────────────────────────────┘
```

The home-feed data flow is now: **EventIngest → `RootIndexedFeed`
(KernelEventObserver in `nmp-feed`) → `Nip10ReplyAttribution` decode in
`nmp-nip01` instance → snapshot push to FFI**. The `Grouper` is no longer on
the home-feed hot path; it remains load-bearing for the thread-detail view
through `Nip10ModularTimelineView`. No NIP-10 knowledge in the kernel, none
in `nmp-feed`'s engine. Every cross-crate side-effect is either an observer
fan-out (substrate seam) or a `dispatch_action` call (D11).

---

## 3. Per-question decisions (A–J)

### A. Where does "OP-centric feed with attribution" semantics live?

**Decision: A new generic engine `RootIndexedFeed<R: ParentResolver, A:
AttributionPayload>` in `nmp-feed`, plus a thin protocol instance in
`nmp-nip01`.**

- `nmp-feed` adds two new public items:
  - `trait AttributionPayload` — describes how a reply event becomes a
    sibling attribution record on the root's card.
  - `struct RootIndexedFeed<R, A>` — the state machine. Owns roots,
    attributions, pending-attribution buffer, follow-set capability,
    profile cache. Implements `KernelEventObserver` and `FeedController`.
- `nmp-nip01` adds:
  - `struct Nip10ReplyAttribution` — the NIP-10-shaped payload (carries raw
    pubkey, raw timestamp, optional profile mirrors).
  - `fn register_op_feed(app, viewer)` — constructs
    `RootIndexedFeed<Nip10Resolver, Nip10ReplyAttribution>`, registers it as
    a `KernelEventObserver` and snapshot projection, and registers the
    `nmp.nip01.thread_root.claim` `ActionModule`.
  - `nmp.nip01.thread_root.claim` `ActionModule` — the dispatch surface
    that turns an engine-emitted `ClaimRequest` into an
    `EnsureInterest` command.

**Rejected alternatives:**

- **Mode flag on `Nip10ModularTimelineView`** — would couple two views with
  different output shapes, different dependency sets (the OP feed needs the
  follow set; the modular timeline does not), and different consumption
  patterns. One type, two behaviors → the well-known antipattern.
- **Engine inside `nmp-nip01`** — couples the engine's state machine to a
  single protocol. A second NIP that wants OP-centric semantics (NIP-22
  kind:1111 comments — covering all non-kind:1 root kinds: NIP-23
  longform, NIP-94 files, NIP-99 listings, podcast episodes, …) would
  duplicate the state machine. The user's standing rule is the right
  shape, not the smallest move; the right shape is one engine with N
  instances.
- **Engine inside `nmp-threading`** — `nmp-threading` already owns
  `ParentResolver` + `Grouper` + `ThreadPointer`. Adding the feed engine
  would force `nmp-threading` to depend on `nmp-feed` (for cursor /
  window machinery) or to re-implement windowing. The dep edge runs the
  *other* way today (`nmp-feed` → `nmp-threading`); inverting it would
  hurt. The right home is `nmp-feed`, which already depends on both
  `nmp-threading` (for `TimelineBlock`) and `nmp-core` (for
  `KernelEvent`, `BoundedMessageMap`).
- **A wholly new crate (`nmp-feed-roots`)** — `nmp-feed` is already the
  generic feed substrate (cursors, windowing, `FeedCard`/`FeedController`
  traits) with zero protocol semantics. A new crate would either be empty
  (one trait, one struct) or recreate the dependency edges `nmp-feed`
  already has. The crate-boundary spec endorses `nmp-feed` as a
  Layer-4-grade generic substrate; expanding its charter to include the
  engine is consistent with that.

### B. How does Bob's unfollowed OP enter the kernel?

**Decision: Engine emits typed `ClaimRequest` values; the protocol-instance
wiring layer (e.g. `nmp-nip01`) dispatches the
`nmp.<protocol>.thread_root.claim` action.**

- Action shape: `Claim { root_id, consumer_id }` / `Release { root_id,
  consumer_id }`, mirroring `VisibleNoteRelationsAction`. The dispatch
  enqueues a `OneShot + Global + event_ids:[root_id]` `LogicalInterest`
  (the existing PD-033-C path; verified at
  `nmp-planner/src/compiler/partition/case_d_no_author.rs`).
- The engine does NOT call `dispatch_action` directly. It emits a
  typed `ClaimRequest` value through a `&dyn Fn(ClaimRequest)` callback
  threaded into `on_kernel_event` via the observer's outer adapter (the
  protocol-instance side). This keeps the engine free of action-system
  imports — `nmp-feed` does not depend on the action machinery. The
  instance crate (`nmp-nip01`) owns the engine-to-action translation:
  `Nip10ReplyAttribution::wire_claims_to_dispatch(app, root_id, consumer_id)`.
- Refcounted by `(root_id, consumer_id)` so multiple replies to the same
  root coalesce; releasing one consumer doesn't tear down a still-needed
  REQ. Identical shape to `visible_note_relations_identity`.

**Rejected alternatives** (same reasoning as the first draft):

- **B.1 Kernel-internal hydration** — adds NIP-10 parsing inside
  `nmp-core`. D0 violation; D3 violation (writes through actions); contradicts
  step 4.2 of `docs/architecture/crate-boundaries.md` (input-side ingest
  seam).
- **B.2 Planner-driven indirect-root interest** — embeds a NIP-10 decoder
  in the planner. Same D0 violation; conflates "what to fetch from whom"
  with "what root ids to hydrate."
- **B.4 ViewDependencies.ids static declaration** — the OP feed view
  module cannot know which unfollowed root ids will be needed at
  registration time. Static deps cannot express it; dynamic deps are a
  much larger seam.

**Flow control, provenance, lifecycle:** identical to first draft.
`Claim`/`Release` refcount caps the worst case at
`O(open-roots-pending-hydration)`. OneShot REQ flows through the PD-033-C
indexer path, no manual relay selection. Self-closes on EOSE; `unroutable`
toast through existing planner machinery (D6).

### C. Where does "Alice replied in thread" attribution metadata live?

**Decision: The engine's bounded `attributions` index, populated by typed
`AttributionPayload` values supplied by the protocol instance. The
`RootCard` carries `Vec<A>` (capped) + `total: u32`. The index decouples
arrival order from card presence.**

Trait shape (in `nmp-feed`):

```rust
// In nmp-feed::attribution (NEW)
pub trait AttributionPayload: Clone + Send + Sync + 'static {
    /// Build an attribution from a reply event. Returns None when the
    /// event does not qualify (e.g. not a follow's reply, or the
    /// protocol's reply decoder rejects it).
    ///
    /// `follow_set` carries the read-side lookup so the protocol's
    /// is-follow check is centralised at construction time — the engine
    /// stores the payload only when this returns Some.
    fn from_reply(
        reply: &KernelEvent,
        follow_set: &dyn FollowSetLookup,
        profile_for: &dyn Fn(&str) -> Option<ProfileDisplay>,
    ) -> Option<Self>;

    /// The id of the reply event itself. The engine uses this as the
    /// key inside the per-root attribution sub-map (so duplicate
    /// arrivals coalesce).
    fn reply_event_id(&self) -> &str;

    /// The replier's pubkey (raw hex, per display-separation doctrine).
    /// The engine uses this for kind:0 fan-out: when an incoming kind:0
    /// matches this pubkey, the engine calls refresh_for_profile.
    fn author_pubkey(&self) -> &str;

    /// Reply timestamp (raw Unix seconds). The engine uses this to sort
    /// attributions within a root (most-recent first by default; the
    /// instance can re-sort in display).
    fn reply_created_at(&self) -> u64;

    /// Refresh profile mirrors in place when a kind:0 arrives for
    /// `author_pubkey`. The instance owns the mirror semantics (which
    /// fields, what fallback). The engine just delivers the update.
    fn refresh_for_profile(&mut self, profile: &ProfileDisplay);
}
```

The `Nip10ReplyAttribution` instance (in `nmp-nip01`):

```rust
// In nmp-nip01::op_feed::attribution (NEW)
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Nip10ReplyAttribution {
    pub author_pubkey: String,                  // raw 64-hex
    pub author_display: AuthorDisplay,
    pub author_display_name: Option<String>,
    pub author_picture_url: Option<String>,
    pub reply_event_id: String,
    pub reply_created_at: u64,                  // raw Unix seconds
}

impl AttributionPayload for Nip10ReplyAttribution {
    fn from_reply(
        reply: &KernelEvent,
        follow_set: &dyn FollowSetLookup,
        profile_for: &dyn Fn(&str) -> Option<ProfileDisplay>,
    ) -> Option<Self> {
        if reply.kind != KIND_SHORT_NOTE { return None; }
        if !follow_set.is_followed(&reply.author) { return None; }
        let refs = parse_nip10(&reply.tags);
        if !refs.is_reply() { return None; }      // root notes don't attribute
        let profile = profile_for(&reply.author);
        let display = AuthorDisplay::from_profile(&reply.author, profile.as_ref());
        Some(Self {
            author_pubkey: reply.author.clone(),
            author_display_name: display.name.clone(),
            author_picture_url: display.picture_url.clone(),
            author_display: display,
            reply_event_id: reply.id.clone(),
            reply_created_at: reply.created_at,
        })
    }
    // …other methods straightforward
}
```

The engine's bounded index in `Inner`:

```rust
// In nmp-feed::root_indexed (NEW)
struct Inner<R, A, C> {
    resolver: R,
    follow_set: Arc<dyn FollowSetLookup>,
    profiles: BoundedMessageMap<String, ProfileDisplay>,
    roots: BoundedMessageMap<EventId, RootCard<C, A>>,
    attributions:
        BoundedMessageMap<EventId /* root */, BTreeMap<EventId /* reply */, A>>,
    // Reply arrived before its root: buffered keyed by missing root id.
    // BOUNDED — a follow's reply to an unhydratable root would otherwise
    // park entries forever and violate D5.
    pending_attributions:
        BoundedMessageMap<EventId, BTreeMap<EventId, A>>,
    window: FeedWindowState,
    card_builder: Box<dyn Fn(&KernelEvent, /* …profile, relations… */) -> C + Send + Sync>,
}
```

**D5 cap discipline.** Every map is `BoundedMessageMap` with
`MAX_PROJECTION_MESSAGES` capacity (same constant the existing
`NoteRelationIndex::relation_by_event` already uses). When
`pending_attributions` evicts a root entry, the engine emits a matching
`ClaimRequest::Release` for that root (single-writer per fact: bounded
map eviction is the release trigger). The home feed's working set is
bounded by the constant regardless of inbound event volume.

**Case-by-case arrival ordering** (engine semantics, identical to first
draft):

- **(a) Reply arrives before root.** Recorded in `pending_attributions[root]`.
  Engine emits `ClaimRequest::Claim(root_id)` immediately. When root
  lands, engine moves the buffered map into `attributions` and emits a
  fresh snapshot tick.
- **(b) Root arrives before reply.** Recorded in `roots`. Subsequent
  qualifying reply ingest appends to `attributions[root]`.
- **(c) Reply is deleted/replaced.** Same behavior as `NoteRelationIndex`
  today: append-only inside a session. Deletion handling is a separate,
  generic concern (Q1).
- **(d) Profile (kind:0) updates later.** Engine fans out: for every
  attribution whose `author_pubkey()` matches, calls
  `A::refresh_for_profile(&profile)`. Mirrors
  `ModularTimelineProjection::refresh_author_cards`.

**Rejected alternatives** (same as first draft):

- On `TimelineEventCard` directly — couples the card to the feed-composition
  mode. The card is generic; attribution is a sibling field on `RootCard`.
- A new card type entirely distinct — duplicates rendering substrate.
- Computed on-snapshot from the event store — violates D8 (per-event
  alloc after warmup).

**Where does the kind:0 / profile refresh happen — engine or projection?**
Chosen: **the engine** owns the fan-out. The engine already needs a
profile cache (so cards can refine in place), so it sees every kind:0
ingest by virtue of being a `KernelEventObserver`. The engine iterates
`attributions` (and `pending_attributions`) and calls
`A::refresh_for_profile` for every entry whose `author_pubkey()`
matches the incoming kind:0's pubkey. The protocol instance owns *what*
to refresh inside its payload (display name vs. picture URL vs. nip05
verification); the engine owns *when* (kind:0 arrival) and *for which
entries* (matching author). Same factoring as
`ModularTimelineProjection::refresh_author_cards` today.

### D. How does the projection know the follow set?

**Decision: `FollowSetLookup` substrate capability in `nmp-core`, paired
with `LogicalInterest::SocialTimeline` (V-45).** Unchanged from first draft.

Same trait shape, same wiring path. The engine in `nmp-feed` takes an
`Arc<dyn FollowSetLookup>` at construction; the planner consults the same
capability at compile time when expanding `SocialTimeline`. The
`FollowSetLookup` trait lives in `nmp-core` precisely so both `nmp-feed`
(the engine) and `nmp-planner` (the expansion) can consume it without
cycling through `nmp-nip02`.

### E. Where does the grouper's chain-stitching still serve a purpose?

**Decision: Keep `nmp-threading::Grouper` as-is for thread-detail. Refactor
`TimelineBlock::Standalone` to be lossless. Home feed no longer consumes
the grouper.** Unchanged from first draft (re-verified after generic
refactor).

The lossless `TimelineBlock` reshape:

```rust
// nmp-threading::block (revised)
pub enum TimelineBlock {
    Standalone {
        id: EventId,
        /// Some when the event has a NIP-10 reply marker but its chain
        /// collapsed to length 1 (parent isn't locally present, or the
        /// max_module_size cap is 1). None when the event is itself a root.
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

**JSON snapshot compatibility.** The serde encoding moves from
`"Standalone": "<id>"` to `"Standalone": { "id": "<id>", "root": null }`.
chirp-tui's `ids_from_block` is the only consumer that pattern-matches
the string form; the rung 2 PR patches it in-flight (see §5 Stage 1).
F-05 codegen does not emit `TimelineBlock` today; rung 5's switch to
`RootFeedSnapshot` is the codegen pass that delivers the new Swift
Decodables.

Future factoring of `Grouper` itself into "root index" + "chain stitcher"
is no longer load-bearing — the home feed never touches the grouper, and
the thread-detail view's grouper consumption is unchanged. That note is
removed from the "post-v1 follow-up" list because there's nothing to
follow up on.

### F. Doctrine compliance (D0–D14 line items)

Re-run against the new shape:

| Doctrine | Compliance | Notes |
|---|---|---|
| **D0 — substrate is NIP-clean** | ✅ | `nmp-core` gains `FollowSetLookup` (capability, generic) + `LogicalInterest::SocialTimeline` (substrate seam named/tracked by V-45, generic application read pattern). `nmp-feed` gains `AttributionPayload` + `RootIndexedFeed<R, A>` — verified against `nmp-core`'s doctrine-lint banned tokens: no `nip01`, `nip10`, `nip17`, `nip22`, `nip29`, `nip47`, `nip57`, `nip77`, `marmot` appear in the engine's API surface. The engine's public API names exactly two new types (the trait + the struct) and uses only substrate vocabulary (`KernelEvent`, `EventId`, `Pubkey`, `ProfileDisplay`, `FollowSetLookup`, `ParentResolver`, `ThreadPointer`, `BoundedMessageMap`). |
| **D1 — render now, refine in place** | ✅ | `Nip10ReplyAttribution.author_display_name: Option<String>` mirrors `RepostAttribution`. Engine fans out kind:0 through `A::refresh_for_profile`. No spinners, no waiting on profile arrival. |
| **D2 — negentropy before REQ** | ✅ | Root-id hydration runs through the existing OneShot+event_ids path which participates in negentropy. No new bypass. |
| **D3 — outbox routing automatic** | ✅ | `SocialTimeline` expansion produces per-follow interests routed by NIP-65 (Case A). Root-claim OneShot routed by PD-033-C indexer path. No manual relay selection. |
| **D4 — single writer per fact** | ✅ | The engine's `attributions` index is the single owner of attribution facts (one engine instance per `(viewer, kinds)` registration). Cards in `roots` derive from kernel events. Action is the writer of new interests. |
| **D5 — snapshots bounded by open views** | ✅ | Every internal map (`roots`, `attributions`, `pending_attributions`, `profiles`) is `BoundedMessageMap<…>` with `MAX_PROJECTION_MESSAGES` capacity. Eviction emits a matching `ClaimRequest::Release` so no stranded REQs accumulate. Per-row attribution capped at `MAX_THREAD_ATTRIBUTION = 8` enumerated + `total: u32` (rationale in §7-Q1). The engine reuses `FeedWindowState` from this same crate for paging. |
| **D6 — errors as state, never exceptions** | ✅ | OneShot REQ failures land in the planner's `unroutable` toast machinery. `ActionRejection::Invalid` returns `{"error":...}` per D6. No new exception path. |
| **D7 — capabilities report, kernel decides** | ✅ | `FollowSetLookup` is a read capability, returns raw data, takes no policy decisions. The engine asks "is this followed?" and the protocol instance decides "does this reply qualify as attribution?" — D7 holds at both layers. |
| **D8 — bounded reactivity, ≤60 Hz** | ✅ | Observer dispatch buffers through kernel snapshot rhythm. Engine work per ingest is O(per-root-attribution-set) which is small in practice; same hot-path cost as `NoteRelationIndex::ingest`. NIP-10 decode runs once at attribution construction inside `Nip10ReplyAttribution::from_reply`, not on every snapshot tick. |
| **D9 — kernel owns time** | ✅ | `reply_created_at` is `event.created_at` (signed). No wall-clock read in engine or instance. |
| **D10 — provenance + private events** | n/a | Home feed is public kind:1/kind:6. |
| **D11 — publish via dispatch_action** | ✅ | No new publish path. Root hydration is a *read* action (interest registration), routed through `dispatch_action`. No new bespoke `nmp_app_*` C-ABI symbol. |
| **D14 — relay slots typed projections** | ✅ | `nmp.feed.home` remains a typed projection; the engine emits `RootFeedSnapshot<C, A>`. |

**Doctrine-lint verification plan.** Before merging rung 3 (the
`nmp-feed` engine PR), confirm:

- `grep -E 'nip[0-9]+|marmot' crates/nmp-feed/src/` returns zero matches.
- `cargo test -p nmp-testing --test doctrine_lint_smoke` stays green.
- `nmp-feed`'s Cargo.toml gains no new dep on any `nmp-nip*` crate.
  Existing deps (`nmp-core`, `nmp-threading`, `serde`, `serde_json`) are
  unchanged. (Confirmed by reading `crates/nmp-feed/Cargo.toml`.)

**New ADRs:**

- **ADR-0033 — `FollowSetLookup` substrate capability.** Records the
  read seam. Light, ~120 LOC of prose.
- **ADR-0034 — Generic root-indexed feed engine in `nmp-feed`;
  protocol-specific instances in NIP crates.** Records the design
  decision to host the engine in `nmp-feed` and parameterize over
  `ParentResolver` + `AttributionPayload`. Documents the future shape:
  one engine, two foreseeable instances (`Nip10ReplyAttribution` in
  `nmp-nip01`, `Nip22ReplyAttribution` in `nmp-nip22` covering ALL
  non-kind:1 root kinds). Light, ~200 LOC of prose.

### G. What is the right card-payload shape?

**Decision: `RootCard<C, A> { card: C, attribution: Vec<A>,
attribution_total: u32 }` in `nmp-feed`. For the NIP-10 instance,
`C = TimelineEventCard` and `A = Nip10ReplyAttribution`.**

The shape:

```rust
// In nmp-feed::root_indexed (NEW)
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RootCard<C, A> {
    pub card: C,
    pub attribution: Vec<A>,        // capped at MAX_THREAD_ATTRIBUTION
    pub attribution_total: u32,     // raw count even when Vec is truncated
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RootFeedSnapshot<C, A> {
    pub cards: Vec<RootCard<C, A>>,
    pub page: Option<FeedPage>,
    pub metrics: Option<FeedWindowMetrics>,
}
```

Display-separation constraints (raw pubkeys, raw timestamps,
`Option<String>` profile mirrors) are enforced by the
`AttributionPayload` impls in instance crates — `nmp-feed`'s engine
itself stores `A` opaquely and never formats anything. The
`MAX_THREAD_ATTRIBUTION` cap is enforced inside the engine on snapshot
construction; display layer never sees an unbounded `Vec`.

### H. Framework reusability (v1-B framework thesis)

**Decision: Ship as a reusable view module in `nmp-feed` (engine) +
`nmp-nip01` (instance). Compose into Chirp through
`nmp-app-template::register_defaults`. A second app composing the
OP-centric feed for a different protocol is one resolver + one payload
type + the wiring helper.**

Future composition:

```rust
// In apps/reader/nmp-app-reader/src/register.rs (illustrative, post-v1):
nmp_app_template::register_defaults(&app, viewer);
nmp_nip01::register_op_feed(&app, viewer);          // kind:1 thread roots
nmp_nip22::register_op_feed_for_comments(&app, viewer);  // kind:1111 comment
                                                          // trees covering
                                                          // ALL non-kind:1
                                                          // root kinds
```

Both calls compose `RootIndexedFeed` over distinct resolver+payload
parameters. The state-machine code is written exactly once.

### I. Migration / sequencing

**Decision: 7-PR ladder, each PR independently mergeable.** One rung
added vs. the first draft (Stage 2a — `nmp-feed` generic engine —
before Stage 2b — `nmp-nip01` instance). See §5.

### J. Test surface

Tests split cleanly between layers:

- **`nmp-feed` engine tests** (in `crates/nmp-feed/src/root_indexed/tests.rs`)
  — exercise the state machine with a synthetic `ParentResolver` +
  synthetic `AttributionPayload`. Cases:
  - Reply arrives, root arrives → attribution attaches.
  - Reply arrives, root never arrives → `ClaimRequest::Claim` emitted,
    `pending_attributions` populated, eviction emits `Release`.
  - Root arrives, reply arrives later → attribution attaches.
  - Two replies to same root → `attributions` set has both entries; one
    refcount of `Claim`.
  - kind:0 fan-out → engine calls `A::refresh_for_profile` for every
    matching attribution.
  - Bounded-map eviction triggers `Release`.
  - Repost-shaped event (resolver.supersedes returns Some) → target is
    surfaced root, repost attribution attaches.
  - Non-follow reply → dropped, no `Claim` emitted.
  - These tests use ZERO NIP-10 / NIP-22 knowledge: the synthetic
    resolver is a plain `struct TestResolver { parents: BTreeMap<EventId,
    ThreadPointer> }`. This proves the engine is genuinely generic.

- **`nmp-nip01` instance tests** (in
  `crates/nmp-nip01/src/op_feed/tests.rs`) — exercise the wiring:
  - `Nip10ReplyAttribution::from_reply` filtering (kind, follow set,
    NIP-10 reply marker).
  - `register_op_feed` composes the engine correctly.
  - `nmp.nip01.thread_root.claim` action shape (Claim / Release).
  - End-to-end: feed the registered observer a KernelEvent stream
    including follow-replies + a non-followed root, assert the snapshot.

- **PR #710 tests to DELETE in chirp-tui** (per first-draft analysis,
  unchanged):
  - `snapshot_rows_follow_block_order` — asserts a reply renders at
    `depth=1`; OP-centric model never renders replies as feed rows.
  - `partial_chain_module_head_gets_flag` — partial chains no longer
    appear in the feed.
  - `event_root_matching_module_head_is_not_partial_chain` — modules
    don't appear in the home feed.
  - `address_and_external_roots_are_not_partial_event_chains` — same.
  - `module_with_absent_root_field_is_not_partial_chain` — same.
  - `standalone_block_is_never_partial_chain_head` — replaced by a new
    test asserting "RootFeedSnapshot card with empty attribution
    renders as a root row; root with attribution renders with the
    attribution sub-row."

- **Tests to KEEP**: every test for `RowRepost`, `RowRelationCounts`,
  content tree rendering, profile display.

- **Tests to ADD in chirp-tui**:
  - `RootCard` JSON → `TimelineRow` mapping for a card with no
    attribution.
  - `RootCard` JSON → `TimelineRow` mapping for a card with N=3
    attribution entries (assert pubkeys passed through as raw hex,
    display layer composes the line).
  - `RootCard` JSON → `TimelineRow` mapping for a card with `total: 12`
    enumerated as 8 + "and 4 others" (display-layer formatter test).

---

## 4. Doctrine compliance checklist

| Check | Status | Where |
|---|---|---|
| `nmp-core` introduces no NIP-named token | ✅ | New types: `FollowSetLookup` (capability), `LogicalInterest::SocialTimeline` (V-45 seam). Doctrine-lint banned-tokens unchanged. |
| `nmp-feed` introduces no NIP-named token | ✅ | New types: `AttributionPayload`, `RootIndexedFeed<R, A>`, `RootCard<C, A>`, `RootFeedSnapshot<C, A>`, `ClaimRequest`. Public API names only substrate vocabulary. Verified by grep before merge. |
| `nmp-router` introduces no NIP-named token | ✅ | Router untouched. |
| No new bespoke `nmp_app_*` C-ABI symbol | ✅ | Root-claim action goes through existing `nmp_app_dispatch_action`. Projection registration goes through existing `nmp_app_register_event_observer` + `register_snapshot_projection`. |
| No write-path outside `dispatch_action` | ✅ | Engine emits typed `ClaimRequest`; the instance translates to `dispatch_action`. The action enqueues `EnsureInterest` (substrate-canonical, same as `VisibleNoteRelationsAction`). |
| No new poll loop | ✅ | All work is observer-driven (push-based). |
| Display-separation (2026-05-25) | ✅ | `AttributionPayload` trait says nothing about display formatting. The NIP-10 instance carries raw pubkeys, raw Unix timestamps, `Option<String>` profile mirrors. No `display::` calls in any of `nmp-feed::root_indexed`, `nmp-nip01::op_feed`, or the FFI snapshot path. |
| File-size ceiling (500 LOC hard) | ✅ | Engine module in `nmp-feed` targets ~400 LOC + ~280 LOC tests in a sibling test file. Instance modules in `nmp-nip01` target ~100 LOC + ~200 LOC tests. `timeline_projection.rs` (579 LOC today) is unchanged. |
| Single-source-of-truth per fact | ✅ | One engine owns attribution. One capability projects the follow set. One action writes new interests. |
| V-45 prerequisite | ✅ | Co-delivered as Stage 0. |
| ADR-0027 (open `ActorCommand`) | ✅ | Reuses existing `EnsureInterest` / `DropInterestOwner`; no new variant. |
| `nmp-feed` dep graph unchanged | ✅ | Existing deps: `nmp-core`, `nmp-threading`, `serde`, `serde_json`. No new `nmp-nip*` edge introduced. (Confirmed by reading `crates/nmp-feed/Cargo.toml`.) |
| `nmp-nip01` dep graph unchanged | ✅ | Existing deps: `nmp-core`, `nmp-content`, `nmp-feed`, `nmp-nip18`, `nmp-nip57`, `nmp-threading`. No new edges. (Confirmed by reading `crates/nmp-nip01/Cargo.toml`.) |
| F-05 codegen impact | ⚠ | `TimelineBlock` schema change (rung 2) + new `RootFeedSnapshot<C, A>` shape (rung 5) require Swift Decodable regeneration. F-05 is the canonical owner; this proposal ships codegen passes alongside the touching PR. |
| Doctrine-lint scoped | ✅ | `cargo test -p nmp-testing --test doctrine_lint_smoke` runs unchanged; no banned tokens introduced. |
| Crate-boundary spec update | ⚠ | `docs/architecture/crate-boundaries.md` §2 (per-crate table) needs a one-row update to record `nmp-feed`'s expanded charter ("owns the generic OP-centric feed engine"). Lands in rung 6 (Stage 4). |

---

## 5. Concrete change list (file-by-file)

Each PR stays ≤500 LOC per file. The overall delta is concentrated in
`nmp-feed` (new engine module) and `nmp-nip01` (new thin instance).

### Stage 0 — V-45 + `FollowSetLookup` (PR ladder rung 1)

> Prerequisite. Without it the engine has no follow-set lookup and the
> view module has no declarative seam.

| File | Change | LOC ±  |
|---|---|---|
| `crates/nmp-core/src/substrate/lookups.rs` | **NEW** — `FollowSetLookup` trait. | +40 |
| `crates/nmp-core/src/substrate/mod.rs` | Export `FollowSetLookup`. | +2 |
| `crates/nmp-core/src/kernel/types.rs` + `kernel/follow_set_lookup_impl.rs` | **NEW** — `impl FollowSetLookup` over the existing `timeline_authors` field. | +60 |
| `crates/nmp-planner/src/interest.rs` | Add `LogicalInterest::SocialTimeline` (per Q2's chosen shape). | +30 |
| `crates/nmp-planner/src/compiler/mod.rs` | Expand `SocialTimeline` at compile time via `Arc<dyn FollowSetLookup>`. | +50 |
| `crates/nmp-app-template/src/lib.rs` | Wire `FollowSetLookup` impl into the canonical builder. | +20 |
| `docs/BACKLOG.md` | Close V-45. | n/a |
| `docs/decisions/0033-followsetlookup-substrate-capability.md` | **NEW ADR** documenting the read seam. | +120 |

### Stage 1 — `nmp-threading::TimelineBlock` lossless variant (PR ladder rung 2)

| File | Change | LOC ±  |
|---|---|---|
| `crates/nmp-threading/src/block.rs` | Reshape `Standalone(EventId)` → `Standalone { id, root }`. Update `len()` / `is_empty()`. | +25 / -10 |
| `crates/nmp-threading/src/grouper.rs` | Fix line 367: chain-length-1 keeps the root pointer. Update `remove_id_from_blocks` Standalone arm. Update `find_block_with_leaf` Standalone arm. | +12 / -8 |
| `crates/nmp-threading/tests/grouper.rs` | Test: single-event chain with reply marker emits `Standalone { id, root: Some(_) }`. | +35 |
| `crates/nmp-nip01/src/meta_timeline/tests.rs` | Update Standalone-shape expectations. | +10 / -10 |
| `crates/nmp-nip01/src/timeline_projection.rs` | Pattern-match the new shape (read-only consumer). | +5 / -3 |
| `apps/chirp/chirp-tui/src/timeline.rs` | **Load-bearing in-PR fix:** update `ids_from_block` JSON match arms to read `Standalone` as an object (was a string). Keep the home feed rendering through `ModularTimelineProjection` correctly between rungs 2 and 5. | +20 / -10 |
| `apps/chirp/chirp-tui/src/timeline/tests.rs` | Update Standalone JSON fixtures. | +15 / -10 |

### Stage 2a — `nmp-feed` generic engine (PR ladder rung 3) — NEW STAGE

| File | Change | LOC ±  |
|---|---|---|
| `crates/nmp-feed/src/root_indexed.rs` | **NEW** — `trait AttributionPayload`, `struct RootIndexedFeed<R, A>` (inner state machine), `RootCard<C, A>`, `RootFeedSnapshot<C, A>`, `ClaimRequest`. Implements `KernelEventObserver` + `FeedController`. | +400 |
| `crates/nmp-feed/src/root_indexed/tests.rs` | **NEW** — generic engine tests with synthetic resolver + payload. Covers every arrival ordering case + eviction + repost supersession + non-follow drop. | +280 |
| `crates/nmp-feed/src/lib.rs` | Export `AttributionPayload`, `RootIndexedFeed`, `RootCard`, `RootFeedSnapshot`, `ClaimRequest`. | +8 |
| `crates/nmp-feed/Cargo.toml` | No change (deps already cover what's needed). | 0 |
| `docs/decisions/0034-generic-root-indexed-feed-engine.md` | **NEW ADR** documenting the engine + the one-engine-N-instances pattern. | +220 |

### Stage 2b — `nmp-nip01` instance (PR ladder rung 4)

| File | Change | LOC ±  |
|---|---|---|
| `crates/nmp-nip01/src/op_feed/mod.rs` | **NEW** — re-export module surface. | +20 |
| `crates/nmp-nip01/src/op_feed/attribution.rs` | **NEW** — `Nip10ReplyAttribution: AttributionPayload`. | +90 |
| `crates/nmp-nip01/src/op_feed/wiring.rs` | **NEW** — `register_op_feed(app, viewer)` that constructs `RootIndexedFeed<Nip10Resolver, Nip10ReplyAttribution>`, registers it, and wires the `ClaimRequest` callback to `dispatch_action`. | +80 |
| `crates/nmp-nip01/src/op_feed/root_claim.rs` | **NEW** — `ThreadRootClaimAction { Claim, Release }`, `ThreadRootClaimModule: ActionModule`, `thread_root_claim_interest_id`, `thread_root_claim_interest`, `thread_root_claim_identity`, `register_thread_root_claim_actions`. Mirrors `visible_relations.rs`. | +140 |
| `crates/nmp-nip01/src/op_feed/tests.rs` | **NEW** — instance integration tests + action shape tests. | +200 |
| `crates/nmp-nip01/src/lib.rs` | Export `op_feed` module. | +12 |

### Stage 3 — Chirp wiring (PR ladder rung 5)

| File | Change | LOC ±  |
|---|---|---|
| `apps/chirp/nmp-app-chirp/src/ffi/register.rs` | Construct via `nmp_nip01::register_op_feed(app, viewer)` instead of `ModularTimelineProjection`. Drop the ~30 LOC of hand-rolled follow-set wiring (V-45 affordance). | +10 / -50 |
| `apps/chirp/chirp-tui/src/timeline.rs` | Rewrite `TimelineRow::from_snapshot` to consume `RootFeedSnapshot<TimelineEventCard, Nip10ReplyAttribution>` JSON. Delete `ids_from_block`, `event_root_mismatches_top`, `is_partial_chain_head`. Add `thread_attribution: Vec<RowReplyAttribution>` field. | +60 / -100 |
| `apps/chirp/chirp-tui/src/ui/post_list.rs` | Delete the `↳ reply in thread` indicator (lines 138-156). Add a new attribution row that renders the follow repliers below row 1. Update layout. | +50 / -25 |
| `apps/chirp/chirp-tui/src/timeline/tests.rs` | **DELETE** the partial-chain tests (per §3-J). **ADD** new tests for RootCard mapping. | +220 / -160 |
| `apps/chirp/chirp-tui/src/render_intents.rs`, `media_cache.rs` | Drop `is_partial_chain_head: false` literals. | -2 |
| `ios/Chirp/Chirp/Bridge/Generated/*.swift` | Regenerated via `nmp-codegen` for `RootFeedSnapshot` + `Nip10ReplyAttribution`. | varies |

### Stage 4 — `nmp-app-template` affordance + crate-boundary spec (PR ladder rung 6)

| File | Change | LOC ±  |
|---|---|---|
| `crates/nmp-app-template/src/lib.rs` | Add `register_op_feed(builder, viewer)` to `register_defaults` so any app composing `nmp-app-template` gets the OP feed for free. | +25 |
| `docs/architecture/crate-boundaries.md` | One-row update in §2 per-crate table for `nmp-feed`: charter expands to "generic OP-centric feed engine over `ParentResolver` + `AttributionPayload`, plus existing cursor/window/registry primitives." Add the engine to the "Owns" column. | +15 |
| `docs/BACKLOG.md` | Close V-45 (Stage 0), close V-37c (already extracted), add V-59 (this work). | varies |
| `docs/plan.md` | Bump framework-thesis status: a second-app composer can now declare a home feed in one line; second protocol composes by adding `(R, A)` only. | +5 |

**Total worktree footprint:** 7 PRs, ~1,700 LOC net add (concentrated in
the new `nmp-feed::root_indexed` + `nmp-nip01::op_feed` modules), ~250
LOC delete (chirp-tui partial-chain logic + hand-rolled follow-set
wiring).

---

## 6. Sequencing plan — PR-by-PR

Each rung is independently mergeable, leaves master green, and produces
no mid-state where the home feed is broken.

### Rung 1 — Substrate seam: `FollowSetLookup` + `LogicalInterest::SocialTimeline` (Stage 0)

- Lands the trait, the kernel-side impl over `timeline_authors`, the
  planner expansion, and `nmp-app-template` wiring.
- ADR-0033.
- **Tested in isolation**: planner unit tests cover expansion; kernel
  tests cover the impl.
- **Master state after merge**: unchanged user-facing behavior. V-45 closed.

### Rung 2 — Lossless `TimelineBlock::Standalone { id, root }` (Stage 1)

- The grouper bug fix proper.
- JSON schema change; chirp-tui's `ids_from_block` patched in-flight in
  the same PR so the home feed stays green.
- **Tested in isolation**: grouper tests, meta_timeline tests, chirp-tui
  timeline tests (including a regression test for the new
  Standalone-with-root JSON fixture).
- **Master state after merge**: home feed unchanged in user behavior;
  the ↳ indicator from PR #710 keeps working and now also fires for
  Standalone-with-root cases that previously couldn't be detected.

### Rung 3 — `nmp-feed` generic engine (Stage 2a) — NEW

- Ships `AttributionPayload` + `RootIndexedFeed<R, A>` + the snapshot
  types. No NIP code yet.
- ADR-0034.
- **Tested in isolation**: 280 LOC of engine state-machine tests using a
  synthetic `ParentResolver` and synthetic `AttributionPayload`. Proves
  the engine is genuinely generic (zero NIP imports anywhere in the
  test file).
- **Doctrine-lint verification**: `grep -E 'nip[0-9]+|marmot' crates/nmp-feed/src/`
  returns zero matches before merge.
- **Master state after merge**: Chirp behavior unchanged. The engine is
  available to instances; no instance exists yet.

### Rung 4 — `nmp-nip01` instance + claim action (Stage 2b)

- Ships `Nip10ReplyAttribution`, `register_op_feed`,
  `nmp.nip01.thread_root.claim` action.
- **Tested in isolation**: attribution decode tests (kind filter, follow
  filter, NIP-10 reply marker filter), action shape tests, end-to-end
  registration test.
- **Master state after merge**: Chirp behavior unchanged. Any app calling
  `nmp_nip01::register_op_feed` now gets the OP-centric home feed; Chirp
  does not yet.

### Rung 5 — Chirp cut-over (Stage 3)

- Chirp swaps `nmp.feed.home` from `ModularTimelineProjection` to
  `RootIndexedFeed<Nip10Resolver, Nip10ReplyAttribution>` via
  `register_op_feed`. chirp-tui rewrites `TimelineRow::from_snapshot`
  for the new shape. Partial-chain tests deleted; new tests added.
- This is the **product-visible PR**. The user-facing behavior change
  lands here.
- **Tested**: chirp-tui tests + live validation against
  `wss://relay.damus.io`. PR description includes screenshots of the new
  attribution rendering.
- **Master state after merge**: Chirp + chirp-tui show the OP-centric
  home feed. PR #710's ↳ indicator is gone (its information is now
  carried by the attribution row).

### Rung 6 — `nmp-app-template` + crate-boundary spec (Stage 4)

- One-line affordance in the canonical template.
- Crate-boundary spec updated to record `nmp-feed`'s expanded charter.
- BACKLOG / plan updates.
- **Tested**: `nmp-app-template` integration tests verify a
  default-registered app produces a `RootIndexedFeed` over the NIP-10
  instance.
- **Master state after merge**: framework thesis strengthened. V-45
  closed with a working consumer. Any new social app composes the OP
  feed with one line.

**Parallelization:** rung 2 (`nmp-threading` block reshape) and rung 3
(`nmp-feed` engine) operate on different crates and don't conflict on
files. They can land in either order. Rung 4 depends on both (it
consumes the new `Standalone { id, root }` for resolver decisions in
some test fixtures, and it consumes `RootIndexedFeed`). Rung 5 depends
on rung 4. Rung 6 depends on rung 5.

**Total wall-clock estimate:** 6-8 days for a single agent at the
file-by-file detail above.

---

## 7. Open questions (require user decision)

### Q1. Attribution cap and "deletion" semantics

**Decision needed:** when 4+ follows reply to one OP, does the feed show
"Alice, Bob, Carol, Dave replied" (full enumeration up to cap N) or
"Alice and 3 others replied" (first M enumerated + total count)?

Proposal recommends N=8 enumerated + `attribution_total: u32`. Display
layer composes "Alice, Bob, Carol, Dave, Eve, Fox, Greg, Hari, and 12
others replied" if needed.

**Also:** is per-session append-only attribution acceptable for v1, or
do we need NIP-09 deletion handling? Existing `NoteRelationIndex` has
no deletion path either. Cleanest fix is global, not OP-feed-specific.

**Default if no answer:** N=8, append-only, deletion handled post-v1
under a separate `nmp-nip09` work item.

### Q2. `LogicalInterest::SocialTimeline` — enum variant or composition?

**Decision needed:** today `LogicalInterest` is a struct. V-45 named the
seam as if it were an enum variant. Two shapes:

- **(a)** Convert to enum with `Concrete { ... }` + `SocialTimeline {
  viewer, kinds }`. Cleaner expansion path; breaks every existing call
  site.
- **(b)** Keep the struct; add `kind: InterestKind` discriminator field
  where `InterestKind` is `Concrete | SocialTimeline { viewer, kinds }`.
  Minimum disruption.

Proposal recommends **(b)**. The user's "right not smallest" framing
might favor (a); flagged.

### Q3. Repost behavior under the OP-centric model

**Decision needed:** today the modular projection puts a kind:6 repost
in the feed as a card attributed to the reposter, with the embedded
inner note as the body. Should the OP-centric feed preserve this?

The proposal preserves it. The engine's `on_kernel_event` handles
repost-shaped events via `ParentResolver::supersedes` (the existing
trait method — `Nip10Resolver` already implements it). When a
followed user reposts, the target is the surfaced root; the
`RepostAttribution` on the existing `TimelineEventCard` carries the
reposter identity. Reply attribution and repost attribution are
orthogonal: a card can have neither, either, or both, though in
practice they rarely co-occur.

**Default if no answer:** reposts stay (kind:6 supersedes target into a
card; reply attribution layers on top).

### Q4. Self-replies — do my own replies surface my own OPs?

**Decision needed:** when the active user replies to a non-followed
user's OP, should that promote the OP into the feed (treating the
active user as a follow for attribution purposes)?

Proposal recommends **yes** — `FollowSetLookup::is_followed` returns
`true` for the viewer's own pubkey (already true today via
`sync_follow_feed_interests`). Consistent with existing behavior.

### Q5. NIP-22 (kind:1111) comments — same model?

**Decision needed:** NIP-22 is the ONE Nostr protocol covering ALL
non-kind:1 reply structures (comments on NIP-23 longform articles,
NIP-94 file metadata, NIP-99 classified listings, podcast episodes
under NIP-54, etc.). Does the OP-centric feed also surface follow's
NIP-22 comments as attribution on their root events?

The proposal scopes this **out for v1**. Post-v1 work is one
`Nip22ReplyAttribution` payload type + one `Nip22Resolver` (both in a
future `nmp-nip22` crate) + a `register_op_feed_for_comments(app,
viewer)` helper. ZERO additional engine code — `RootIndexedFeed<R, A>`
absorbs the second instance unchanged. This is the framework-thesis
demonstration baked into the design.

There is no per-kind resolver explosion: kind:1 replies use NIP-10,
every other kind's reply tree uses NIP-22. The two resolvers cover the
entire Nostr reply universe.

### Q6. Root-hydration latency trade-off

**Decision needed:** when Alice (a follow) replies to Bob's unfollowed
OP, the engine emits `ClaimRequest::Claim` and waits for the OneShot
REQ against the indexer to return (200ms–5s typical, longer on cold
start). During that window:

- **(a) D1-strict (proposal default)** — hold attribution invisibly in
  `pending_attributions`; show nothing until root lands. Correct-but-laggy.
- **(b) Tombstone-card** — render a placeholder. Violates D1: the OP's
  body, author, and timestamp are all unknown.

Proposal recommends **(a)** as the doctrine-honest default. Post-v1
improvements: pre-warm an indexer subscription for "any event id
appearing as a root pointer in recent follow's kind:1s"; render a
RoutingTraceObserver hint in debug builds only.

**Default if no answer:** (a). Latency documented as the cost of
doctrine-correct rendering.

---

## 8. Backlog entry (draft for `docs/BACKLOG.md`)

```markdown
### V-59 · Home feed is thread-roots-only with reply attribution [HIGH · v1 PRODUCT-MODEL FIX]

**Status:** spec proposed 2026-05-27 in
[`docs/perf/op-centric-feed-architecture.md`](perf/op-centric-feed-architecture.md).

**Evidence:** today's home feed (chirp-tui left pane, Chirp iOS home) shows
replies as standalone feed rows. PR #710 added a ↳ "reply in thread"
indicator as a partial mitigation, but the product model the user wants is
different: **feed = thread roots only; follows' replies attribute back to
their root**. A follow's reply to a non-followed OP should surface the OP
with a "↳ Alice replied" badge. Reply rows never stand alone.

Today's code drops the root pointer on chain-length-1 standalone blocks
(`crates/nmp-threading/src/grouper.rs:367`), defeats attribution at the
threading layer, and lacks any mechanism to fetch a non-followed root id
into the local store (the existing thread-hydration logic
`enqueue_thread_hydration_from_event` only fires when a thread detail view
is open — `crates/nmp-core/src/kernel/ingest/timeline.rs:213-241`).

**Architectural shape:** the engine `RootIndexedFeed<R: ParentResolver, A:
AttributionPayload>` lives in `nmp-feed` (generic substrate, zero protocol
knowledge). `nmp-nip01` provides the NIP-10 instance
(`Nip10ReplyAttribution` + `register_op_feed`); a future `nmp-nip22`
provides the kind:1111 instance covering ALL non-kind:1 root kinds
(NIP-23, NIP-94, NIP-99, podcasts, …). One engine, two foreseeable
instances; no per-kind state-machine explosion.

**Prerequisite:** V-45 (`LogicalInterest::SocialTimeline` substrate seam) —
co-delivered as Stage 0 of this work, alongside `FollowSetLookup`
capability.

**Recommended action:** seven-rung PR ladder per
`docs/perf/op-centric-feed-architecture.md` §5. Net add ~1,700 LOC across
`nmp-threading`, `nmp-core` (substrate seam only), `nmp-planner`,
`nmp-feed` (engine), `nmp-nip01` (instance), `nmp-app-template`, and
`apps/chirp/`. Net delete ~250 LOC (partial-chain machinery in chirp-tui
+ hand-rolled follow-set wiring in nmp-app-chirp). Two new ADRs:
ADR-0033 (`FollowSetLookup` capability) and ADR-0034 (generic
root-indexed feed engine in `nmp-feed`; protocol-specific instances in
NIP crates).

**Open user decisions** carried to `docs/perf/pending-user-decisions.md`:
Q1 (attribution cap + deletion semantics), Q2 (LogicalInterest enum vs
discriminator), Q3 (repost behavior under OP-centric model), Q4
(self-replies), Q5 (NIP-22 scope deferred to post-v1), Q6
(root-hydration latency trade-off). All have flagged defaults if the
user is unavailable.

**Out of scope (post-v1):** the `nmp-nip22` instance over kind:1111
comment trees. Implementation is ~150 LOC (one `ParentResolver` impl
plus one `AttributionPayload` impl plus one wiring helper); engine code
is zero new lines. Tracked separately when `nmp-nip22` crate is created.
```

---

## 9. Notes the implementer must read before writing code

- **Do not** add a new bespoke `nmp_app_*` C-ABI symbol. PD-039
  deprecation calendar is in force. Use existing observer/projection/
  action seams.
- **Do not** parse NIP-10 inside `crates/nmp-core/**`. The `parse_nip10`
  import is already a small leak via `event_references` — this proposal
  *shrinks* the leak (the NIP-10 decoder lives only in
  `Nip10ReplyAttribution::from_reply` and `Nip10Resolver`, both in
  `nmp-nip01`) and the implementer should not enlarge it.
- **Do not** import any `nmp-nip*` crate from `nmp-feed`. The engine's
  Cargo.toml stays exactly as today. Doctrine-lint banned-token grep is
  the pre-merge check.
- **Do not** assume the home-feed snapshot key (`nmp.feed.home`) is
  unique — keep the same key so iOS Swift consumers don't break. The PR
  rewires the *contents* (now `RootFeedSnapshot<TimelineEventCard,
  Nip10ReplyAttribution>` instead of `ModularTimelineSnapshot`) but the
  key must persist.
- **Read the precedent** before writing: `visible_relations.rs` (action
  shape, identity, refcount), `timeline_projection.rs`
  (`RepostAttribution`, `refresh_author_cards`, `BoundedMessageMap`
  usage), `note_relations.rs` (the aggregation-index pattern),
  `nmp-feed::window.rs` (existing windowing the engine reuses verbatim).
- **Test scope:** scoped `cargo test` per AGENTS.md. The orchestrator
  runs the full suite at merge time.
- **Doctrine-lint:** `cargo test -p nmp-testing --test
  doctrine_lint_smoke` must remain green throughout. Verify after every
  rung.
- **Crate-boundary spec:** `docs/architecture/crate-boundaries.md`
  needs the one-row update in §2 per-crate table for `nmp-feed`'s
  expanded charter. Lands in rung 6 alongside the BACKLOG / plan
  updates.
