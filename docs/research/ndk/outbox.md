# NDK Outbox Model (NIP-65)

Primary refs: `core/src/outbox/`, `core/src/relay/sets/calculate.ts`, `core/src/ndk/index.ts`, `core/src/utils/get-users-relay-list.ts`.

## Big picture

NDK implements outbox/gossip routing per NIP-65: each user advertises read/write relays (kind 10002, fallback to kind 3 content), and clients address queries to the relays where each author actually publishes — not to a fixed soup of "popular" relays.

Three moving parts:

1. **`OutboxTracker`** (`core/src/outbox/tracker.ts`) — LRU-cached map `pubkey → OutboxItem { readRelays, writeRelays, relayUrlScores }`. Fetches relay lists in batches of 400, emits `user:relay-list-updated` when a list arrives.
2. **`calculateRelaySetsFromFilters`** (`core/src/relay/sets/calculate.ts:194`) — splits a filter that has `authors` into per-relay sub-filters: each chosen relay only receives the subset of authors that publish to it.
3. **`chooseRelayCombinationForPubkeys`** (`core/src/outbox/index.ts:45`) — given N pubkeys + a per-author goal (default 2 relays each), picks the minimum relay set that covers every author. Prefers already-connected relays, then pool-permanent relays, then sorted by relay ranking.

## Outbox pool

```
core/src/ndk/index.ts:450-456
if (!(opts.enableOutboxModel === false)) {
    this.outboxPool = new NDKPool(opts.outboxRelayUrls || DEFAULT_OUTBOX_RELAYS, ...);
    this.outboxTracker = new OutboxTracker(this);
```

A separate `NDKPool` is used for relay-list lookups so metadata fetches don't pollute the main read pool. Configured by `outboxRelayUrls` constructor opt; otherwise `DEFAULT_OUTBOX_RELAYS`. Disabled wholesale with `enableOutboxModel: false`.

## Tracking lifecycle

Every `ndk.subscribe()` call with an `authors` filter triggers tracking:

```
core/src/ndk/index.ts:910-916
if (this.outboxPool && subscription.hasAuthorsFilter()) {
    const authors = subscription.filters
        .filter(f => f.authors?.length > 0)
        .flatMap(f => f.authors!);
    this.outboxTracker?.trackUsers(authors);
}
```

`trackUsers()` (`tracker.ts:73`):
- Skips pubkeys already in the LRU cache (placeholder set immediately to prevent duplicate in-flight fetches).
- Calls `getRelayListForUsers()` which subscribes to `{ kinds: [10002, 3], authors }` on the outbox pool.
- On reply, applies `ndk.relayConnectionFilter` to drop blocked relays from the discovered set (`tracker.ts:112-130`).
- Emits `user:relay-list-updated` per pubkey (`tracker.ts:135`).

## Live subscription refresh

The wiring that closes the loop:

```
core/src/ndk/index.ts:459-471
this.outboxTracker.on("user:relay-list-updated", (pubkey, _outboxItem) => {
    for (const subscription of this.subManager.subscriptions.values()) {
        const isRelevant = subscription.filters.some(f => f.authors?.includes(pubkey));
        if (isRelevant && typeof subscription.refreshRelayConnections === "function") {
            subscription.refreshRelayConnections();
        }
    }
});
```

`refreshRelayConnections()` (`subscription/index.ts:787-812`) recomputes the relay map via `calculateRelaySetsFromFilters` and **only adds** newly-discovered relays. It does not remove relays or change `filter.authors`. This is critical for understanding the limits of the auto-rewire (see kind3-auto-tracking.md).

Also: a `relay:connect` pool monitor (`subscription/index.ts:696-716`) checks whether each newly-connected relay belongs in any open subscription, and subscribes from it if so.

## Write-side outbox

`calculateRelaySetFromEvent` (`relay/sets/calculate.ts:31`) picks publish targets:

1. Author's write relays (from tracker).
2. Up to 5 unique `wss://` URLs from `["a","e"]` tag relay hints.
3. If event has <5 `p` tags: read relays of mentioned users (`chooseRelayCombinationForPubkeys(pTags, "read")`).
4. Pool's permanent + connected relays.
5. `ndk.devWriteRelaySet` if configured.

## Read-side splitting

`calculateRelaySetsFromFilter` (`relay/sets/calculate.ts:120-185`) — the heart of read routing:

```
if (authors.size > 0) {
    const authorToRelaysMap = getRelaysForFilterWithAuthors(ndk, Array.from(authors), relayGoalPerAuthor);
    for (const filter of filters) {
        if (filter.authors) {
            for (const [relayUrl, authors] of authorToRelaysMap.entries()) {
                const intersection = filter.authors.filter(a => authors.includes(a));
                result.set(relayUrl, [...result.get(relayUrl)!, { ...filter, authors: intersection }]);
            }
        }
    }
}
```

Falls back to `ndk.explicitRelayUrls` if no authors; then to `pool.permanentAndConnectedRelays().slice(0, 5)` if still empty.

## Cache integration

Outbox tracker state is *not* persisted to cache adapters — there is a `TODO` in `tracker.ts:48-49` noting rehydration is unimplemented. On every fresh app boot, the tracker is empty and the first wave of subscriptions pays full discovery cost.

## Pitfalls

- LRU max 100k entries / 2-minute TTL (`tracker.ts:62-65`). Long-lived sessions may evict and re-fetch relay lists for the same author.
- `chooseRelayCombinationForPubkeys` defaults to count=2 relays per author. Increase via `relayGoalPerAuthor` subscription option for higher redundancy.
- `enableOutboxModel: false` short-circuits everything: tracker null, no per-author splitting, all subscriptions go to `explicitRelayUrls` only.
- Relay hints are only extracted from `["a","e"]` tags, not `["p"]` (`calculate.ts:44-46`).
- Blocked relays (from session's `blockedRelays`) are filtered out at tracker write time, but only if `ndk.relayConnectionFilter` is set before the relay list arrives — see race in `a912a2c2`.
