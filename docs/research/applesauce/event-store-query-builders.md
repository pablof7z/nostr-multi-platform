# Applesauce — EventStore & Query Builders (the "load-bearing magic")

> Source: `/private/tmp/nostr-research/applesauce` @ `da5ec22b` (May 2026).
> All file:line citations are against that tree.

## The thesis

There is no special path for kind:3. The kind:3 / outbox / profile reactivity that makes Applesauce feel magical is **emergent from four generic mechanisms composing**, all hooked to `EventStore.insert$` / `update$` / `remove$`:

1. A typed pointer-indexed in-memory store (`EventMemory`) with NIP-01 replaceable semantics.
2. Per-model `share()` with hash-keyed dedup so N callers asking the same query get one upstream subscription.
3. Refcount-based event "claims" (separate from RxJS refcounts) so the LRU pruner cannot evict events still bound to live UI.
4. Reactive operators (`ContactsModel` → `OutboxModel` → `includeMailboxes`) that **switchMap** into per-element subscriptions, so a contact list change automatically re-resolves every downstream mailbox sub.

NMP must reproduce all four. Skipping any one yields either leaks, thrashing, or a broken outbox model on follow/unfollow.

## 1. Two-layer store: `EventStore` ⇒ `EventMemory`

`packages/core/src/event-store/event-store.ts:55-133` — `EventStore` extends `EventModels` (the query-builder mixin) and composes:
- a pluggable `IEventDatabase` (`event-store.ts:108-114`, falls back to `EventMemory` if none given),
- a separate `EventMemory` instance used as a singleton-instance cache (`event-store.ts:111` — when a real DB is set, memory is created in addition to it),
- a `DeleteManager` (`event-store.ts:123-126`),
- an `ExpirationManager` (`event-store.ts:129-132`),
- three `Subject<NostrEvent>` streams: `insert$`, `update$`, `remove$` (`event-store.ts:93-99`),
- an optional `eventLoader` fallback (`event-store.ts:102-104`) for "subscribe to an id we don't have yet."

The `mapToMemory()` helper (`event-store.ts:136-142`) is the **single-instance invariant enforcer**: every event returned from the database is normalized through `EventMemory.add()` so that two reads of the same id return the exact same object reference. This is what makes `distinctUntilChanged((a,b) => a?.id === b?.id)` in `EventModel` (`models/base.ts:116`) cheap and what lets `claimLatest` track an event by identity.

### `EventMemory` indexes (`event-store/event-memory.ts`)

- `events: LRU<NostrEvent>` keyed by id, lines 27, 42-44 — touchable, with LRU semantics for prune.
- `kinds: Map<number, Set<NostrEvent>>`, `authors: Map<string, Set<NostrEvent>>`, `kindAuthor: Map<string, Set<NostrEvent>>` (lines 18-24).
- `tags: LRU<Set<NostrEvent>>` — **lazily built** per `tag:value` key on first query, `getTagIndex()` lines 258-275 (with a warning log if it takes >100ms — line 270).
- `created_at: NostrEvent[]` sorted descending, `iterateTime()` uses `binarySearch` for since/until window (lines 361-398).
- `replaceable: Map<string, NostrEvent[]>` — sorted-descending array of all versions for a `(kind, pubkey, identifier)` address (line 30, 95-109).

The filter resolver (`getEventsForFilter`, lines 408-510) implements iterative AND with a key optimization at lines 470-486: when `authors × kinds <= 20`, use the composite `kindAuthor` index in a single pass instead of intersecting two large sets. This is the kind of micro-optimization NMP should plan for in `nmp-core`'s LMDB index choice.

NIP-91 AND/OR tag filters are first-class: `&tag` keys are intersected, `#tag` keys are unioned, and `#tag` values that also appear in `&tag` are dropped per NIP-91 (lines 437-467).

## 2. Replaceable semantics + NIP-01 tie-break

`EventStore.add()` (`event-store.ts:213-307`) is the heart of correctness. Order of operations:

1. Kind-5 events are forwarded to `DeleteManager` and short-circuit (lines 215-218).
2. If `deleteManager.check(event)` says the author already deleted this id/address → swallow (line 221).
3. If keepExpired=false and event already past expiration → reject (line 225).
4. Stamp `seenRelay` if `fromRelay` was provided (line 228).
5. **NIP-01 tie-break, incoming-side** (lines 235-252): for replaceable kinds, compute the existing "winner" via `(created_at desc, id asc)`. If incoming does not beat the winner, swallow and (importantly) copy any cached symbols from the incoming event onto the winning instance.
6. Verify signature (line 255). Default `verifyEvent` from nostr-tools; can be overridden or disabled, with a console warning on `set verifyEvent(undefined)` (lines 88-90). NMP should mirror that warning — silent signature-disable is a footgun.
7. Insert into memory; if memory returned an existing instance, this was a duplicate — copy symbols and notify update (lines 258-266).
8. Insert into the database; if returned instance === input, stamp `EventStoreSymbol` and emit `insert$` (lines 269-277).
9. **NIP-01 tie-break, outgoing-side** (lines 285-301): re-compute the winner from the now-stored set, remove every loser via `remove$`. This is what fixes the regression in commit `90d525af` (gotchas §G1): two clients within the same second each had been silently dropping the other's update.
10. Schedule expiration if expiration tag present (line 304).

The async variant (`event-store/async-event-store.ts:176-269`) is line-for-line equivalent with `await` sprinkled, including the duplicated tie-break logic. **NMP should factor this into one function** rather than copy-pasting.

## 3. `EventMemory` claims — refcount-based GC anchor

`event-memory.ts:176-242`:
- `claims: WeakMap<NostrEvent, number>` (line 176, **a counter, not a boolean** — see gotchas §G2).
- `claim()` increments + touches LRU (line 188).
- `removeClaim()` decrements; on 0, deletes the entry so `isClaimed()` returns false (lines 201-211).
- `prune(limit)` iterates `unclaimed()` (which walks the LRU oldest-first) and removes (lines 217-242).

This is **independent of RxJS refcounting**. RxJS share() decides "should this upstream subscription still exist?" Claims decide "is this event still safe to evict from memory?" The two operate on different objects (subscription vs event) and a single eviction-safe operation requires both to agree.

The two RxJS operators that anchor claims:

- `observable/claim-events.ts` — `claimEvents(claims)`: for streams of `NostrEvent | NostrEvent[]`, claim each unseen event in `tap`, release all on `finalize`. Used by `TimelineModel`.
- `observable/claim-latest.ts` — `claimLatest(claims)`: for streams of `NostrEvent | undefined`, swap claim on every change (release old, claim new). Used by `EventModel` and `ReplaceableModel`.

## 4. `EventModels` — the query-builder mixin

`packages/core/src/event-store/event-models.ts`:

```ts
// :44-46
models = new Map<ModelConstructor<any, any[], TStore>, Map<string, Observable<any>>>();
modelKeepWarm = 60_000;
```

`model(constructor, ...args)` (lines 50-86):
1. `hash_sum(args)` keys the cache (or `constructor.getKey(...args)` if provided — `OutboxModel` defines one at `models/outbox.ts:26-29` because its options object would otherwise key poorly).
2. If a model already exists for `(constructor, key)`, return it.
3. Otherwise call `constructor(...args)(this)` to materialize the observable, then wrap with:
   - `finalize(cleanup)` — removes the entry from the cache when the inner observable terminates.
   - `share({ connector: () => new ReplaySubject(1), resetOnComplete: () => timer(60_000), resetOnRefCountZero: () => timer(60_000) })`.

That `share` config is the **subscription dedup magic**: all subscribers to the same `(constructor, key)` share one upstream and one ReplaySubject(1) cache. When the last subscriber unsubscribes, a 60-second warm-keep timer starts; if a new subscriber arrives within that window, no re-materialization happens. This is what makes calling `store.profile(pk)` from 50 React components result in one upstream pipeline.

Public sugar methods (lines 93-149): `filters()`, `event()`, `replaceable()`, `addressable()`, `timeline()`, `profile()`, `contacts()`, `mailboxes()`. The `IEventSubscriptions` interface (`event-store/interface.ts:110-130`) is the contract; downstream packages can extend `EventModels.prototype` via TypeScript module augmentation (doc comment at `event-models.ts:18-38`).

## 5. The four base models (`packages/core/src/models/base.ts`)

### `EventModel(pointer)` — lines 92-120
- `defer(getEventFromStores)` lazily reads the store on each subscription.
- Optional fallback loader injected via `loadEventUsingFallback` (lines 73-89) that emits `undefined` first then the loaded event — important for UI: don't render stale, render loading.
- Merges three streams: initial fetch, `insert$.filter(id)`, and `remove$.filter(id).map(() => undefined).take(1)`.
- `distinctUntilChanged` by id, `claimLatest` to pin.

### `ReplaceableModel(pointer)` — lines 123-173
- Same shape but filters `insert$` by `(pubkey, kind, identifier)` (lines 137-143).
- The `current` variable + `tap` at line 146 is acknowledged as hacky — it tracks the current event so the `remove$` filter can match by id (you can't know which id was current until something else updated it).
- `distinctUntilChanged` compares `created_at` to ignore older versions arriving late (lines 160-168). Note this trusts the store's tie-break in `add()`; if NMP's store doesn't tie-break, this comparator alone won't either.

### `TimelineModel(filters, includeOldVersion?)` — lines 176-251
- Initial bulk fetch via `getTimeline`, claimed with `claimEvents`.
- Merges with `insert$.filter(matchFilters)` and `remove$.filter(matchFilters).map(id)`.
- `scan` builds the timeline; for replaceable kinds tracks `seen: Map<UID, Event>` so a newer version replaces the older one in the array (lines 224-243). `finalize` clears `seen` on unsubscribe (line 248) — leaks-by-default would otherwise be obvious here.
- **Always returns a new array instance** on every emit (line 220, 230) so React reference-equality memos work.

### `FiltersModel(filters, onlyNew?)` — lines 254-271
- Just merges initial-set (or EMPTY if onlyNew) with `insert$.filter(matchFilters)`. Streams individual events, not an array. Useful for processing.

## 6. The kind:3 + outbox composition — end-to-end

When the application does `store.model(OutboxModel, user, { type: 'outbox', maxConnections: 5 })`:

```
EventStore.insert$ for kind:3 from user
   └─> ReplaceableModel({kind:3, pubkey:user}) re-emits the new event   (models/base.ts:136-143)
       └─> ContactsModel(user).map(getContacts) re-emits ProfilePointer[]   (models/contacts.ts:13-19)
           └─> OutboxModel:    (models/outbox.ts:14-24)
                ignoreBlacklistedRelays  -> (observable/relay-selection.ts:52-61)
                includeMailboxes(store, 'outbox')  -> switchMap over the contact array, each
                                                      contact subscribes to its OWN
                                                      kind:10002 ReplaceableModel and merges
                                                      relays into the ProfilePointer
                                                      (observable/relay-selection.ts:19-49)
                selectOptimalRelays  -> set-cover heuristic                (helpers/relay-selection.ts:14-93)
```

Key observations for NMP:

- `includeMailboxes` uses `combineLatest(contacts.map(user => store.replaceable({kind:10002,pubkey:user.pubkey})...))`. That means **adding one contact spawns one new inner observable that itself goes through the shared-model cache**. If the same user is already mailbox-tracked by another model, the inner observable is the same shared instance — zero extra relay traffic.
- The map-projection at `relay-selection.ts:32-43` returns the original `user` pointer when the kind:10002 hasn't loaded yet; it does **not** drop the contact. That's intentional — partial outbox knowledge still resolves *something*.
- `selectOptimalRelays` (`helpers/relay-selection.ts:14-93`) is a greedy set-cover: each iteration picks the relay covering the most uncovered users, optionally biased by `score(relay, coverage, popularity)`. `maxRelaysPerUser` caps per-user concentration. The TODO at line 89 acknowledges that the final per-user relay list may exceed `maxRelaysPerUser` because the count is enforced during selection, not after.
- `OutboxModel.getKey` (`models/outbox.ts:26-29`) hashes only `[pubkey, type, maxConnections, maxRelaysPerUser]` — the blacklist and `score` function are intentionally excluded so they can be changed without forcing a new pipeline.

This composition pattern is the actual deliverable for NMP. Reproducing the kind:3 special case alone misses the point — what NMP needs is the **mixin + share() + claim() + switchMap-into-models** substrate that allows users to write `MailboxesModel`, `OutboxModel`, and ten future models without each one wiring up its own insert/remove/dedup/share machinery.

## 7. Direct API surface NMP must expose (or analogues)

| Applesauce surface | NMP analogue (in spec terms) |
| --- | --- |
| `store.add(event, fromRelay)` returning the canonical instance | `EventStore::ingest(event, provenance) -> &Event` |
| `store.event(pointer)` / `store.replaceable(...)` / `store.timeline(...)` / `store.filters(...)` | typed `AppView::*` subscriptions per spec §7.4 |
| `store.model(Constructor, ...args)` mixin extension point | the open question for `nmp-core`: how do downstream crates add a typed view kind without forking? |
| `EventStore.eventLoader` fallback | the "subscribe-by-pointer, fetch if missing" path |
| `claim/touch/prune/unclaimed` LRU+refcount API | spec §7.5 claim-based GC |
| `insert$`/`update$`/`remove$` streams | spec §7.4 reactive view emission |
| `EventModels` mixin pattern | the LLM-friendliness test from `plan.md:148` — adding a new view kind without touching `nmp-core` |

## 8. What this means for NMP's M2

If `nmp-core` ships only the EventStore + planner without the model-cache + claim layer, every consumer will independently materialize its own per-view pipeline, and the framework will not deliver the "reactive-by-default, GC-safe-by-default" experience the spec promises. The model-cache layer is small (`event-models.ts` is 150 LOC, `claim-events.ts`+`claim-latest.ts` are <80 combined) but architectural — it must be part of Phase 1, not deferred.
