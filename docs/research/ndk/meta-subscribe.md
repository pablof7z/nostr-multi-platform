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
| Re-sort on caller toggle | `sort` is a `Self::Spec` field; key changes ⇒ view reopens; hydration interest sees full cache hit (no relay round-trip); pointer interest re-merges via compiler dedup (no relay churn either) |
| Reactive on `$follows` | `InterestScope::ActiveAccount` + Trigger A1 (`Nip65Arrived`) and the proposed `Trigger::FollowListChanged` (framework-magic §C5) already recompile dependent interests |
| Outbox routing on pointer fetch | Free, via compiler §3.1 Stage 1 — `$metaSubscribe` does **not** get this; NMP does |
| Cross-view dedup of hydration | Free, via compiler §3.3 merge lattice — `$metaSubscribe` does **not** get this |

The architectural win NMP gets for free: when a `RepostedContentTimeline`, a `HighlightedArticles` view, and a `ProfileClaim` for the same author's note are all open, **NMP issues one merged REQ** per relay covering all three; NDK's `$metaSubscribe` issues an unbounded number of `fetchEvents` calls outside the planner.

## 6. Recommendation: partial — reference `MetaTimelineViewModule`, no new trait, slot in M2

**Build it as one reference view module** in `nmp-nip01` (proposed `meta_timeline.rs`, ~200 LOC). **Do not** introduce a sixth trait family — the framework-magic contract explicitly forbids new types (`framework-magic.md` "Non-goals" line 91). **Slot: M2** — the compiler already needs `ThreadView`-style hydration cascades for thread building, so meta-subscribe is the second consumer that proves the cascade generalises.

Cost: ~200 LOC view module + ~50 LOC payload types + ~30 LOC compiler change for the `addresses: BTreeSet<NaddrCoord>` field on `InterestShape` (decided in §7, no longer optional — without it parameterized-replaceable hydration cannot dedup across views) + 1 contract test file with ~6 sub-tests (enumerated in §7). The compiler change is in M2 scope by construction: M2 owns `InterestShape`, and a thread view module needs the same address-hydration capability anyway (a NIP-22 comment thread on a NIP-23 article addresses by `kind:30023:pubkey:dtag`).

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

## 7. M2 decisions and test surface

### Decisions (not deferrable)

- **Address-pointer shape in `InterestShape`: add the field.** `meta-subscription.svelte.ts:264-290` builds per-author filters `{kinds, authors:[pk], "#d":dtags}` from `a`-tag references. NMP's `InterestShape` (`subscription-compilation/intro.md` §2.1) must grow `pub addresses: BTreeSet<NaddrCoord>`, where `NaddrCoord { kind: u32, pubkey: Pubkey, d_tag: String }` is a named record (not a tuple — it will surface in `ViewSpec` payloads and needs a stable generated FFI shape per ADR-0010). Stage-3 merge lattice (compiler.md §3.3) adds one mergeability rule (`addresses` union per relay, capped after decomposition by the relay's per-filter tag-value limit on `#d` and by the per-author filter-count limit). The wire-emitter decomposes each `NaddrCoord` into a per-author `{kinds, authors:[pk], "#d":[dtags]}` filter at REQ-emit time. Without this, parameterized-replaceable hydration cannot dedup across views (highlights of a NIP-23 article + comments on the same article would issue two separate per-author REQs instead of one merged one). M2 owner: track as a follow-up sub-task in the M2 exit gate; the same field is required for `ThreadViewModule` when threads address replaceable parents.
- **Re-sort semantics: sort is a `Spec` field, not a runtime setter.** A change to `sort` changes `Self::Key` (which hashes the spec); the view registry treats it as a new view open. The pointer `LogicalInterest` is identical across sort modes, so compiler dedup merges it with the prior REQ (zero relay churn). The hydration interest is identical too (same pointed-to event ids), so the cache satisfies it without a relay round-trip. The old payload is dropped on warmth-grace expiry. **This is consistent with the current `ViewModule` trait** (no mutable-spec contract exists, and we do not propose one). The cost is one extra `ViewBatch::FullState` payload on sort change; the benefit is no new trait surface. NDK's "re-sort without restart" optimisation is a UX win only when sort changes are frequent — for the typical meta-feed UI ("sort by reposts | sort by recency"), one full state per user-driven toggle is acceptable.
- **WoT-rank parity: out of v1.** NDK's `SubscribeConfig` includes `wot` / `wotRank` (`subscription.svelte.ts:30-34`); `$metaSubscribe` inherits via `extends Omit<SubscribeConfig, "dedupeKey">`. NMP's WoT module is post-v1 (not in `kernel-substrate.md` §11 v1 module list); the reference `MetaTimelineViewModule` ships without WoT rank in v1. When WoT lands as a module, it surfaces as a `ProjectionCache` consumed via `on_projection_changed` — view-module-local addition, no further substrate change.

### Test surface for `c14_meta_timeline_hydrates_pointed_to_via_compiler`

Six sub-tests in one `#[test] fn`, all using the `PlannerHarness` (`subscription-compilation/tests.md` §9.3):

1. **Hydration via event-ids.** Open a meta view with `pointer_filter: {kinds: [6], authors: [A]}`; ingest one kind:6 with an `e`-tag to event `X`. Assert one pointer REQ on A's write relay AND one hydration REQ `{ids: [X]}`. Ingest `X`; assert payload contains one `MetaItem { event: X, pointer_count: 1 }`.
2. **Hydration via addresses.** Ingest one kind:6 with an `a`-tag `30023:B:my-article`. Assert the hydration REQ shape decomposes to `{kinds:[30023], authors:[B], "#d":["my-article"]}` routed to B's write relays (Stage 1 outbox).
3. **Cross-view hydration dedup.** Open two meta views whose pointer interests would produce the same hydration ref set (e.g. a highlight view and a comment view both touching the same article). Assert one merged hydration REQ per relay, not two — the property NDK's `guardrailOff().fetchEvents()` cannot provide.
4. **All four sort modes.** With a fixed pointer event set (3 pointers from 3 distinct authors to 2 targets), assert payload `items` ordering matches each of `Time`, `Count`, `TagTime`, `UniqueAuthors`. Same `State`, four distinct `Payload`s.
5. **Sort change is a reopen, not a restart.** Open with `sort: Time`; capture the wire-frame audit log. Change spec to `sort: Count` (new view-handle, new Key); assert zero new wire frames issued (pointer REQ merges via dedup; hydration is fully cache-hit). One additional `ViewBatch::FullState` payload arrives with the resorted items.
6. **Placeholder for missing target — D1 rendering.** Two sub-cases (per D1: no loading gates; every field non-`Option`):
   - **`e`-tag pointer (no known author).** Ingest a pointer to event `Y` not in the store; assert payload contains `MetaItem { event: EventPayload::placeholder(id=Y, author_pubkey=Pubkey::ZERO, content=""), pointer_count: 1 }`. The author placeholder field renders as the shortened-`Y` identifier (the only stable handle available); author_pubkey stays zero so `on_projection_changed` on a later kind:0 arrival is a no-op for this item.
   - **`a`-tag pointer (author known via address coord).** Ingest a pointer to `30023:B:my-article` not in the store; assert payload contains `MetaItem { event: EventPayload::placeholder(coord=Naddr, author_pubkey=B, content=""), pointer_count: 1 }`. Then ingest `B`'s kind:0; assert `on_projection_changed` refines the author display fields in place. Then ingest the article; assert `on_event_inserted` refines `content` + replaces `id` placeholder with real id, same item position (no list reordering unless sort dictates).
