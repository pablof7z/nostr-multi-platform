# OP-Centric Feed Architecture (RETIRED)

> **RETIRED (#3082/#3086).** This design record describes the `RootIndexedFeed`
> engine, `NoteFeedItem` row, `AttributionPayload` reply-rollup, and
> `FeedShape::RootIndexed` — all demolished by the composite-feed redesign.
> None of the concrete types named below exist on current `master`. The
> current design record is
> [`docs/perf/composite-feed-architecture.md`](../perf/composite-feed-architecture.md).
> This file is kept only as a searchable historical breadcrumb; do not treat
> any claim below as current. See also
> [ADR-0076](../decisions/0076-app-facing-feed-helpers.md) and
> [`docs/architecture/crate-boundaries.md`](../architecture/crate-boundaries.md) §8.

---

> **Status (historical):** Shipped. Design record for ADR-0076/ADR-0076/ADR-0072/ADR-0076.
> The shipped invariants are: `nmp-feed` provides bounded feed mechanics only;
> protocol/app layers declare primary kinds and perspectives; protocol adapters
> derive repost wrapper acquisition; app/runtime composition roots wire
> ReducedSource expansion for active-user follows; `nmp-core` only sees the materialized
> generic interests; secondary data is claimed by the component or sibling
> module that needs it, not by the feed.
>
> **Revision:** 2026-05-27d (post-codex-v2 + user decisions). Shipped via
> 7-PR ladder. Pre-kind:3 buffer was not retained; replay comes from interest
> registration and cache-serve.

---

## 1. Executive summary

An OP-centric following timeline is a **stream of thread roots** produced by a generic engine
`RootIndexedFeed<R: ParentResolver, A: AttributionPayload>` in `nmp-feed`.
Each root carries an attribution list of follows' replies, exposed as raw data
so every render surface chooses its own enumeration policy.

**Crate layout:**

- **`nmp-feed`** — `RootIndexedFeed<R, A>` engine, `AttributionPayload` trait.
  Engine takes a generic `FollowPredicate: Arc<dyn Fn(&str) -> bool + Send + Sync>`
  and an `EventLookup: Arc<dyn Fn(&EventId) -> Option<KernelEvent> + Send + Sync>`.
  No NIP-named tokens; no follow-set trait; no planner coupling; no acquisition.
- **`nmp-nip01`** — kind:1/NIP-10 fact owner: note builder/decoder and
  reply/thread grouping semantics.
- **`nmp-note-feed`** — `Nip10ReplyAttribution: AttributionPayload`,
  concrete `NoteFeedItem` row payload, and `register_op_feed(viewer,
  follow_predicate, event_lookup)` wiring helper.
- **`nmp-nip02`** — `ActiveFollowSet`, observable snapshot of active account's
  follows. Exposes `follows() -> Vec<String>` and
  `predicate() -> Arc<dyn Fn(&str) -> bool + Send + Sync>`. No `FollowSetLookup`
  trait.
- **app/runtime composition roots** — open typed `FeedParams` sessions that
  reduce `FeedSourceExpr::ActiveUserFollows` into dependent child interests
  through the standard feed-session compiler.
- **`nmp-core`** — owns the generic interest registry, cache-serve, routing, and
  planner execution. It does not own active-user follow-feed acquisition, a
  follow-set trait, NIP tokens, `SocialTimeline`, or bespoke C-ABI symbols.
- **`nmp-threading`** — `TimelineBlock::Standalone { id, root: Option<ThreadPointer> }` (lossless).
- **`nmp-planner`** — unchanged. No `SocialTimeline` variant.

**Four mechanisms:**

1. `TimelineBlock::Standalone` is lossless (root pointer preserved on 1-event chains).
2. `RootIndexedFeed<R, A>` engine consumes `KernelEvent`s via the observer fan-out,
   buffers followed replies whose roots are absent, keeps repost placeholders keyed
   by target id, and exposes `RootFeedSnapshot<C, A>` (visible-window-only).
3. Components that render secondary data use their own mounted dependencies
   (`claim_event`, profile, count, media, preview). The feed never does that.
4. App/runtime roots compose the feed owner explicitly.

---

## 2. Key architectural decisions

### A. Crate ownership

Generic engine in `nmp-feed`; concrete NIP-10 note-feed instance in
`nmp-note-feed`; lower-level NIP-10 facts in `nmp-nip01`; follow-set producer in
`nmp-nip02`; ReducedSource composition in app/runtime roots; generic
interest/cache/routing execution in `nmp-core`.

### B. How Bob's unfollowed OP enters the kernel

Not because the feed fetched it. A followed reply records pending attribution in
`pending_attributions[bob_op_id]`. Bob's OP enters through normal acquisition:
active interests, relay replay/cache-serve, or a mounted UI component explicitly
claiming it. If Bob's OP never arrives, the pending attribution stays bounded local
state until evicted or cleared by a perspective reset.

### C. Attribution metadata

The projection exposes ALL enumerated repliers (bounded only by D5 cap per root).
No `attribution_total` field — `Vec<A>` length IS the count. Each render surface
picks its own policy (chirp-tui: 1; iOS: N).

D5 bound: each `BTreeMap<EventId, A>` capped at `MAX_ATTRIBUTION_PER_ROOT` (default 64),
evicting oldest by `reply_created_at`. Global `BoundedMessageMap` is also
`MAX_PROJECTION_MESSAGES` bounded.

```rust
pub trait AttributionPayload: Clone + Send + Sync + 'static {
    fn from_reply(reply: &KernelEvent, follow: &dyn Fn(&str) -> bool) -> Option<Self>;
    fn reply_event_id(&self) -> &str;
}
```

### D. Follow set — closure predicate, no trait (ARCHITECTURE OVERRIDE)

No `FollowSetLookup` trait. No `LogicalInterest::SocialTimeline`. No bespoke
core follow-feed door.

**`nmp-nip02::ActiveFollowSet`** exposes `follows()`, `predicate()`, `on_change()`.
The defaults runtime opens an observed projection for `kinds:[3]` scoped to the
active account author and switches that projection on account changes.

**App/runtime composition root:**

```rust
pub fn open_active_follows_op_feed(
    app: &NmpApp,
    primary_kinds: Vec<u32>,
    key: FeedKey,
) -> Result<FeedHandle, FeedSpecOpenError> {
    app.feeds().open_spec(
        key,
        feed::events()
            .primary_kinds(primary_kinds)
            .from(source::active_user().follows())
            .shape(FeedShape::RootIndexed)
            .order(FeedOrder::NewestByFeedPosition)
            .window(FeedWindowPolicy::bounded(80))
            .project(FeedItemProjection::feed_rows()),
    )
}
```

Rationale: the v3 `SocialTimeline` planner-side expansion forced a
`FollowSetLookup` trait that creates an `nmp-feed → nmp-core → nmp-planner` cycle.
The current shape avoids the cycle and delivers the V-45 affordance through the
standard typed feed-session compiler and its generic ReducedSource/dependent-
interest mechanism.

### E. `TimelineBlock::Standalone` lossless reshape

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

### F. Doctrine compliance

| Doctrine | Status | Notes |
|---|---|---|
| D0 | ✅ | `nmp-feed` has no NIP nouns. CI grep gate: `grep -E 'nip[0-9]+\|marmot\|ProfileDisplay' crates/nmp-feed/src/` returns zero. |
| D1 | ✅ | Profile display resolved by UI components, not the feed. |
| D4 | ✅ | Engine `attributions` is single owner per root. |
| D5 | ✅ | Every map bounded; visible-window-only snapshot. |
| D7 | ✅ | Closure-shaped capabilities (`Arc<dyn Fn(...)>`). |
| D8 | ✅ | Observer-driven; no poll. |
| D11 | ✅ | No new bespoke C-ABI symbol. |
| D14 | ✅ | App-owned OP-feed keys emit the shared typed NNFS projection schema. |

**ADRs:** ADR-0076 (generic root-indexed feed engine), ADR-0076 (composition-root
followset expansion). See also ADR-0072 and ADR-0076 for the shipped implementation.

### G. Card-payload shape

```rust
pub struct RootCard<C, A> where C: Clone + Serialize, A: Clone + Serialize {
    pub card: C,
    pub attribution: Vec<A>,   // bounded by MAX_ATTRIBUTION_PER_ROOT
}

pub struct RootFeedSnapshot<C, A> where C: Clone + Serialize, A: Clone + Serialize {
    pub cards: Vec<RootCard<C, A>>,
    pub page: Option<FeedPage>,
    pub metrics: Option<FeedWindowMetrics>,
}
```

### H. Repost edge cases

Engine gains `event_lookup: Arc<dyn Fn(&EventId) -> Option<KernelEvent> + Send + Sync>`.

- **L-1:** Followed user reposts → insert kind:6 wrapper into `roots[target_id]`.
- **L-2:** Followed user replies to kind:6 → consult `event_lookup(kind6_id)`;
  if returns supersedes target, re-key attribution to target.
- **L-3:** Followed user replies to original note → standard attribution case.
- **L-4:** Repost + reply on same card → both display.
- **L-5:** E-tag-only repost (empty content) → insert empty placeholder; when
  target arrives later, engine rebuilds the card via `card_builder`.

### K. Startup and identity-change semantics

Cold-start replay comes from interest registration and cache-serve after kind:3
rebuilds `timeline_authors`. The historical pre-kind:3 buffer described in earlier
drafts was not retained.

Account switch: `ActiveFollowSet` rebuilds from the active-account slot;
`on_change` resets `RootIndexedFeed` perspective (clears roots, attributions,
pending_attributions, render window).

---

## 3. Implementer notes

- **Do not** add feed-owned secondary acquisition. Components that need a missing
  event/profile/count open their own dependencies through existing NMP seams.
- **Do not** parse NIP-10 inside `nmp-core`. Decoder lives in `nmp-nip01`.
- **Do not** import any `nmp-nip*` crate from `nmp-feed`. CI grep enforces.
- **Do not** add a `FollowSetLookup` trait, `LogicalInterest::SocialTimeline`
  variant, or bespoke follow-interest expansion outside the ReducedSource path.
  Re-read §2-D.
- **Do not** accept dual `Standalone` JSON shapes.
- **Do not** poll. Observer callbacks only.
- **Do** read the precedent: `claim_event`, `crates/nmp-core/src/subs/oneshot.rs`,
  `crates/nmp-nip01/src/timeline_projection.rs` for `BoundedMessageMap`.
- **Doctrine path:** `docs/product-spec/doctrine.md`.

---

## 4. Post-v1 follow-ups

- **V-60:** NIP-51 mute-list interaction. Adapter-side `ActiveFollowSet::predicate` subtraction.
- **V-61:** NIP-22 (kind:1111) `RootIndexedFeed` instance (~150 LOC; zero engine changes).
- **V-62:** Retire `timeline_authors` from `nmp-core` (social concept in substrate).
- **V-63:** `claim_event` EOSE-driven release vs. host-release refcount mismatch.
- **V-64:** Typed `Kernel::event_provenance(event_id)` accessor for relay hints.
