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

## 5. The NMP-side equivalent: dependent interests, not out-of-band fetches

The original research mapped this to a proposed runtime reducer trait. That
surface did not ship. Current NMP uses typed feed sessions,
event observers, projections, ref claims, and the interest registry.

A meta-subscription is still the right shape, but the NMP implementation target
is **dependent interests**:

- the pointer stream is one normal `LogicalInterest`;
- each pointer event surfaces `EventId` / `NaddrCoord` / tag references;
- the referenced targets become dependent interests or refs owned by the
  read model/component that needs them;
- those interests go through the existing registry, planner, router, cache, and
  close lifecycle;
- the bidirectional `pointedBy` index and sort modes are local projection/read
  model state.

This is the same architectural family as ReducedSource feed acquisition (#2092),
but event/address hydration should build on the ref/dependent-interest path. It
must not overload pubkey `FeedScope` or reintroduce out-of-band fetches.

| `$metaSubscribe` concern | NMP substrate location |
|---|---|
| Pointer subscription | `LogicalInterest { shape, lifecycle: Tailing }` registered by the read model/session |
| Tag extraction + reference set | Event observer/projection logic derives target ids/addresses from stored pointer events |
| Batch fetch of pointed-to events | Dependent interests or `resolve_ref` claims for ids/addresses; compiler routes/dedups/merges with overlapping consumers |
| `pointedBy` map | Read-model/projection state keyed by target id/address |
| Sort modes | Projection rendering, recomputed from pointer + target state |
| Re-sort on caller toggle | App/read-model state change; target interests remain deduped and cache-first |
| Reactive on `$follows` | `FeedScope` / ReducedSource source changes replace materialized pointer interests; target dependencies stay planner-owned |
| Outbox routing on pointer fetch | Free, via compiler §3.1 Stage 1 — `$metaSubscribe` does **not** get this; NMP does |
| Cross-view dedup of hydration | Free, via compiler §3.3 merge lattice — `$metaSubscribe` does **not** get this |

The architectural win NMP gets for free: when a reposted-content feed, a
highlighted-articles read model, and a profile/event ref all need the same
target, **NMP issues one merged REQ** per relay covering all consumers; NDK's
`$metaSubscribe` issues unbounded `fetchEvents` calls outside the planner.

## 6. Recommendation: use the existing registry/ref/dependent-interest seams

Do not introduce a new trait family and do not revive `ViewModule`. Build
meta-timeline style features as protocol/read-model code that registers a
pointer interest and dependent target interests/refs through existing seams.

If address/event target hydration needs more substrate, add it as a
dependent-interest/ref capability with the same lifecycle rules as feed source
reduction: one owner, fail closed, planner-routed, cache-first, deduped, and
closed when the consumer/source withdraws.

Benefit: reposted-feed, highlighted-articles, commented-articles, and
engagement-aggregator UIs can be expressed without giving up planner-level
dedup, outbox routing, cache-serve, or close semantics.

## 7. Current NMP sketch

The implementation shape is a read-model/protocol module, not a new substrate
trait:

```text
open pointer source
  -> register one normal tailing LogicalInterest
ingest pointer events
  -> reducer extracts event ids / naddr coords / tag refs
  -> dependent-interest owner replaces the materialized target set
targets arrive or cache serves
  -> projection recomputes pointed-to items and pointedBy index
sort changes
  -> projection state changes; target interests stay deduped and cache-first
close consumer
  -> owner releases pointer interest and dependent target children
```

The existing `InterestShape` already has `event_ids` and `addresses`, so the
event/address flavor of metaSubscribe composes with the planner today. #2092
extends the same family of mechanics to source reductions whose output is an
author or tag-value set, so home/follow feeds, mute-list feeds, follow-pack
feeds, and pointer-target hydration stop being separate bespoke paths.

## 8. Test surface

The correctness tests should assert the substrate properties, not a view trait:

1. **Hydration via event ids.** Pointer event with an `e` tag materializes one
   dependent `{ids:[X]}` interest, routed and deduped through the planner.
2. **Hydration via addresses.** Pointer event with an `a` tag materializes one
   dependent `addresses:{coord}` interest routed to the addressed author's
   write relays.
3. **Cross-consumer dedup.** Two projections that need the same target produce
   one merged REQ per relay.
4. **Source shrink closes.** When the pointer/source reducer removes a target,
   the dependent child closes after normal owner/warmth rules.
5. **Empty output fails closed.** An empty pointer/source reduction produces no
   wildcard target query.
6. **Sort is projection state.** Changing sort recomputes payload order without
   rebuilding unchanged pointer/target interests.
