# NDK `$metaSubscribe` — Indirect-Subscription Pattern and NMP Fit

Source: `/Users/pablofernandez/Work/NDK-nhlteu/svelte/src/lib/builders/meta-subscription.svelte.ts` (468 LOC) + exposure at `ndk-svelte.svelte.ts:319-323`. Doc + examples at `svelte/registry/src/routes/(app)/docs/subscriptions/+page.svelte` and `svelte/templates/sveltekit-vercel-ndk/src/routes/highlights/+page.svelte`.

## 1. What `$metaSubscribe` is

A reactive **two-stage** subscription. Stage one subscribes to a filter that returns **pointer events** (kind:6 reposts, kind:7 reactions, kind:9802 highlights, kind:1111 comments, kind:9735 zaps — anything whose `e`/`a` tags reference the *real* content). Stage two extracts those tags and **batch-fetches the pointed-to events**. The reactive output is the pointed-to events, with a bidirectional `pointedBy: Map<tagId, NDKEvent[]>` so the UI can render "reposted by N people" or "highlighted by N readers" without further plumbing.

API shape (`meta-subscription.svelte.ts:28-52`):

```ts
interface MetaSubscription<T extends NDKEvent = NDKEvent> {
  get events(): T[];                             // pointed-to events, sorted
  get count(): number;
  get eosed(): boolean;
  get pointedBy(): Map<string, NDKEvent[]>;      // tagId -> pointer events
  eventsTagging(event: NDKEvent): NDKEvent[];    // convenience reverse-lookup
  start(): void; stop(): void; clear(): void;
}
```

Sort options (`meta-subscription.svelte.ts:10-26`): `time` (newest content), `count` (most-pointed-to), `tag-time` (most recently tagged), `unique-authors` (author-diversity). Re-sort happens **without** restarting the subscription (`:197-206`).

## 2. How it works internally

Hot path is `handlePointerEvents` (`:211-311`):

1. For every pointer event, collect `e`-tags and `a`-tags into one `Set<string>` plus a reverse map `pointersByRef: Map<ref, NDKEvent[]>`.
2. Partition references: bare hex → `ids: []` filter; `kind:pubkey:dtag` → addresses, grouped by `pubkey` into per-author filters of shape `{ kinds, authors: [pk], "#d": dTags }` (`:264-290`).
3. Issue **one** `ndk.guardrailOff().fetchEvents(filters)` call — bypassing the framework's normal subscription path (`:295`).
4. Match fetched events back to their pointers via `event.tagId()`; insert into `targetEventMap` and `pointersByTarget`; re-`updateEvents()` (which applies the current sort).

Reactive lifecycle (`:174-206`): the `$derived` over `config()` rebuilds `filters` whenever the caller's reactive deps change (e.g. `$follows` mutates); `restart()` does a full teardown + rebuild of the pointer subscription. `closeOnEose: false` keeps the pointer feed open indefinitely (`:399`).

## 3. Real-world callers

- **Reposted-content feed** — `filters: [{ kinds: [6,16], authors: $follows }], sort: 'tag-time'` returns the *posts being reposted*, with per-post repost counts (`registry/.../docs/subscriptions/+page.svelte:33`).
- **Highlighted articles** — `filters: [{ kinds: [9802], limit: 100 }], sort: 'unique-authors'` returns the *articles being highlighted*, with `articleHighlights(article).length` and `uniqueHighlighters(article)` rendered in the card (`templates/.../highlights/+page.svelte:9`).
- **Discussed comments** — `kind:1111` comments by follows return the *parents* (`templates/.../+page.svelte:21`).
- **Generic "engagement feed"** — any combination of reactions, reposts, zaps, comments, all collapsing into the underlying content sorted by engagement.

## 4. What it costs NDK and what's missing

**Pros:** one call replaces ~80 LOC of manual cascade (subscribe-to-pointers → extract-tags → batch-fetch → maintain-two-maps → re-sort-on-input-change). Re-sort without restart preserves cache. Bidirectional index is built once.

**Architectural smell:** the second-stage fetch uses `ndk.guardrailOff().fetchEvents(filters)` (`:295`) — an **out-of-band call that bypasses the planner**, outbox routing, and dedup. If three views all want the same article (one from highlights, one from comments, one from reposts), NDK issues three separate `fetchEvents` calls. The pointer subscriptions are also untouched by outbox routing on the pointer side. Errors are swallowed silently (`:308-310`).

## 5. The NMP-side equivalent: already covered by `ViewModule` + compiler

The five-family kernel substrate (`docs/design/kernel-substrate.md` §1) intentionally has **five** trait families; `ViewModule` covers all reactive projections including hydration cascades. The existing M2 design already specifies this exact pattern for threads (`docs/design/subscription-compilation/compiler.md` §3.5 row "open_thread"):

> *"Move to a `ThreadViewModule` in `nmp-nip10`. The hydration cascade is `view_module.reduce(...)` returning additional interests as new event ids surface in store."*

A meta-subscription is structurally **a sibling of `ThreadViewModule`**: pointer filter is `interests()[0]`; each pointer arrival surfaces new `EventId`/`NaddrCoord` references; `reduce(...)` returns a second `LogicalInterest { event_ids: ..., lifecycle: OneShot }`; the compiler dedups it across views and against the cache (compiler §3 stage 3 merge lattice). The bidirectional `pointedBy` index, the four sort modes, and the placeholder rendering are **State + Payload + Delta** concerns local to one view module — not new substrate.

| `$metaSubscribe` concern | NMP substrate location |
|---|---|
| Pointer subscription | `LogicalInterest { shape, lifecycle: Tailing }` registered at `open()` |
| Tag extraction + reference set | `ViewModule::on_event_inserted` returning `Option<Delta>` + emitting new interests via `ctx.register_interest()` |
| Batch fetch of pointed-to events | A second `LogicalInterest { event_ids, lifecycle: OneShot }` re-emitted on `reduce`; compiler routes/dedups/merges with overlapping interests from other views |
| `pointedBy` map | `Self::State` (an in-memory `BTreeMap<TagId, Vec<EventId>>`) |
| Sort modes | `Self::Payload` rendering, recomputed on `on_event_inserted` |
| Re-sort without restart | Setter on the view module's `Self::Spec` triggers `on_projection_changed`, not `restart` |
| Reactive on `$follows` | `InterestScope::ActiveAccount` + Trigger A1 (`Nip65Arrived`) and the proposed `Trigger::FollowListChanged` (framework-magic §C5) already recompile dependent interests |
| Outbox routing on pointer fetch | Free, via compiler §3.1 Stage 1 — `$metaSubscribe` does **not** get this; NMP does |
| Cross-view dedup of hydration | Free, via compiler §3.3 merge lattice — `$metaSubscribe` does **not** get this |

The architectural win NMP gets for free: when a `RepostedContentTimeline`, a `HighlightedArticles` view, and a `ProfileClaim` for the same author's note are all open, **NMP issues one merged REQ** per relay covering all three; NDK's `$metaSubscribe` issues an unbounded number of `fetchEvents` calls outside the planner.

## 6. Recommendation: partial — reference `MetaTimelineViewModule`, no new trait, slot in M2

**Build it as one reference view module** in `nmp-nip01` (proposed `meta_timeline.rs`, ~200 LOC). **Do not** introduce a sixth trait family — the framework-magic contract explicitly forbids new types (`framework-magic.md` "Non-goals" line 91). **Slot: M2** — the compiler already needs `ThreadView`-style hydration cascades for thread building, so meta-subscribe is the second consumer that proves the cascade generalises.

Cost: ~200 LOC view module + ~50 LOC payload types + 1 contract test (`c14_meta_timeline_hydrates_pointed_to_via_compiler`). Zero substrate change if the open question in §7 resolves "addresses fit existing InterestShape composition." If it resolves the other way (needs an `addresses: BTreeSet<NaddrCoord>` field on `InterestShape`), add ~30 LOC there and one open-question close-out in `subscription-compilation/intro.md` §2.1.

Benefit: every reposted-feed / highlighted-articles / commented-articles / engagement-aggregator UI in every NMP app becomes one `useMetaTimeline(spec)` call. The five social-feed patterns that NDK ships as a single API (`$metaSubscribe`) become a single API for NMP too — without giving up planner-level dedup, outbox routing, or warmth-grace lifecycle.

Sketch (NOT prescriptive — view module shape, not new substrate):

```rust
// crates/nmp-nip01/src/meta_timeline.rs (proposed)
pub struct MetaTimelineViewModule;

#[derive(Clone, Serialize, Deserialize)]
pub struct MetaTimelineSpec {
    pub pointer_filter: InterestShape,     // e.g. {kinds: [6,16], authors: <follows>}
    pub sort: MetaSort,                    // Time | Count | TagTime | UniqueAuthors
}

#[derive(Clone, Serialize)]
pub struct MetaTimelinePayload {
    pub items: Vec<MetaItem>,              // pointed-to events, sorted
}

#[derive(Clone, Serialize)]
pub struct MetaItem {
    pub event: EventPayload,               // best-effort per D1
    pub pointer_count: u32,
    pub unique_authors: u32,
    pub latest_pointer_at: UnixSeconds,
}

#[derive(Clone, Serialize)]
pub enum MetaDelta {
    NewTarget { item: MetaItem },                       // pointed-to event arrived
    NewPointer { target_id: TagId, by: Pubkey },        // additional pointer for existing target
    TargetReordered { from: usize, to: usize },         // sort changed
    SortChanged { sort: MetaSort },
}

impl ViewModule for MetaTimelineViewModule {
    type Spec = MetaTimelineSpec;
    type Payload = MetaTimelinePayload;
    type Delta = MetaDelta;
    type State = MetaState;       // holds targets, pointersByTarget, sort
    type Key = blake3::Hash;      // hash(pointer_filter, sort)

    // open() registers ONE pointer interest; on_event_inserted on a pointer
    // collects refs, returns the hydration interest set via ctx.register_interest()
}
```

The two `LogicalInterest`s the module emits:

```
pointer:    { shape: spec.pointer_filter, lifecycle: Tailing, scope: ActiveAccount }
hydrate:    { shape: { event_ids: <refs>, addresses: <addrs> }, lifecycle: OneShot,
              scope: Global }
```

The compiler's existing dedup makes "three views asking for the same article" emit one REQ. The existing warmth grace (compiler §3 line 26 / `subsystems.md` §7.6) keeps the hydration REQ alive 30s after the last view drops.

## 7. Open questions for M2

- **Address-pointer shape in `InterestShape`.** `meta-subscription.svelte.ts:264-290` builds per-author filters `{kinds, authors:[pk], "#d":dtags}` from `a`-tag references. NMP's `InterestShape` (`subscription-compilation/intro.md` §2.1) has `authors`, `kinds`, `tags`, `event_ids` — but no first-class `addresses: BTreeSet<NaddrCoord>` field. Two ways to resolve: (a) the view module decomposes addresses into the existing `(authors, kinds, tags[#d])` shape per author at registration time (mirrors NDK's approach; compiler stays unchanged); (b) add `addresses: BTreeSet<NaddrCoord>` to `InterestShape` so the compiler can dedup parameterized-replaceable hydration across views (cleaner; ~30 LOC compiler change). Recommend (b) for compiler-level dedup correctness; (a) is the v1.x fallback if M2 LOC budget tightens.
- **Re-sort delta semantics.** `$metaSubscribe`'s re-sort without restart fits naturally into `on_projection_changed` if `Spec.sort` is treated as a projection input rather than a spec field. Alternative: emit `Delta::TargetReordered` from a `set_sort()` action on the view handle. The view-catalog contract template (`view-catalog/template-and-enumeration.md`) does not currently address mutable specs; this is a v1.x decision for any view module that wants client-side reconfiguration without restart.
- **WoT-rank parity.** NDK's `SubscribeConfig` includes `wot` / `wotRank` (`subscription.svelte.ts:30-34`); `$metaSubscribe` inherits via `extends Omit<SubscribeConfig, "dedupeKey">`. NMP's WoT module is post-v1 (not in `kernel-substrate.md` §11 v1 module list); the reference `MetaTimelineViewModule` would not ship WoT rank in v1 even if the underlying primitive lands later. Re-evaluate in the milestone that brings WoT.
