# Subscription Compilation: Filters → Relay-Specific REQs

How a single `ndk.subscribe({ kinds, authors })` call becomes a set of relay-specific REQ messages.

Primary refs: `core/src/subscription/index.ts`, `core/src/subscription/grouping.ts`, `core/src/subscription/manager.ts`, `core/src/relay/sets/calculate.ts`.

## Entry point

`NDK.subscribe(filters, opts, handlers)` (`core/src/ndk/index.ts` ~line 855):

1. Construct `NDKSubscription` — sets up `internalId`, debug context, options.
2. Register in `this.subManager.subscriptions` (Map).
3. If `outboxPool` enabled and filter has `authors`: `outboxTracker.trackUsers(authors)` (fire-and-forget).
4. `setTimeout(async () => { await cacheAdapter.initializeAsync?; subscription.start(); }, 0)` — deferred start lets the caller chain.

## start() decision tree

```
core/src/subscription/index.ts (start ~ ln 600-690)
```

1. If `cacheUsage !== ONLY_RELAY` and `cacheAdapter` exists: call `cacheAdapter.query(this)` (sync or Promise).
2. Process cached events through `eventReceived` path.
3. If cached results "fully fill" the query (`queryFullyFilled`), emit `eose` and stop. Otherwise: `loadFromRelays()`.
4. `loadFromRelays()` decides relay grouping (see Grouping below) and calls `startWithRelays()`.

## Cache modes

`NDKSubscriptionCacheUsage`:
- `CACHE_FIRST` (default) — query cache, then relays.
- `PARALLEL` — fire both at once.
- `ONLY_CACHE` — never hit relays.
- `ONLY_RELAY` — never hit cache.

Ephemeral kinds (20000-29999) skip the cache entirely (`subscription/index.ts` change in `5afbd245`).

## addSinceFromCache

`opts.addSinceFromCache: true` (`subscription/index.ts:537`):
- Implies `CACHE_FIRST`.
- Tracks `mostRecentCacheEventTimestamp` while processing cache results.
- When opening relay REQs, rewrites each filter to `since = max(filter.since, mostRecentCacheEventTimestamp + 1)`.
- Used by sessions store start (`sessions/src/store.ts` and `react/src/session/store/start-session.ts:183`) to avoid re-fetching events the cache already has.

## cacheUnconstrainFilter

Default `["limit", "since", "until"]`. When querying the cache, these keys are stripped from the filter — the cache returns *everything* matching the structural filter, then post-filter is applied. Prevents the cache from artificially limiting results when re-opened.

## Grouping

`core/src/subscription/grouping.ts:filterFingerprint()` produces a deterministic ID for a filter array:

- Each filter's keys are sorted and joined (`-`).
- For `since`/`until`, the value is included (so different time windows don't collide).
- Closed-on-eose subscriptions are prefixed with `+` to prevent grouping with long-lived ones.
- Filters from multiple filters are joined with `|`.

Subscriptions with matching fingerprints (and `groupableDelay`) get merged into a single REQ via `mergeFilters()`:

- Filters with `limit` are not merged (each kept separate).
- For limitless filters: arrays are unioned per key; scalars overwritten last-wins.

Two grouping strategies (`groupableDelayType`):
- `at-least` — wait at least N ms before sending; collect any matching sibs.
- `at-most` — send no later than N ms; collect what's there.

Default delay is per-subscription. Mismatched delay types prevent grouping.

## Per-relay filter splitting

When a sub starts and has no explicit `relaySet`, `startWithRelays` (`subscription/index.ts:747-780`) calls `calculateRelaySetsFromFilters` → returns `Map<relayUrl, NDKFilter[]>`. Each relay gets only the filters relevant to it.

For authors-bearing filters, `calculate.ts:131-165` intersects each filter's `authors` with each relay's pubkey coverage so a relay only receives REQ frames for authors it actually carries. Filters without authors fan out to every chosen relay.

## Subscribing to a relay

`relay.subscribe(this, filters)` (`relay/index.ts`) hands off to a per-relay subscription executor that:
- Bundles compatible REQs into one socket frame where possible.
- Tracks `eose` per relay.
- On disconnect, marks the per-relay sub for resume on reconnect.

Empty REQs are prevented (`ad7936b6` race fix): if a grouped subscription's only members are closed before the scheduled fire time, the executor cleans up and skips the REQ.

## Pool monitor for new relays

`startPoolMonitor` (`subscription/index.ts:696-716`):

```
this.poolMonitor = (relay: NDKRelay) => {
    if (this.relayFilters?.has(relay.url)) return;
    const calc = calculateRelaySetsFromFilters(...);
    if (calc.get(relay.url)) {
        this.relayFilters?.set(relay.url, this.filters);
        relay.subscribe(this, this.filters);
    }
};
this.pool.on("relay:connect", this.poolMonitor);
```

Whenever any relay connects after the subscription started, the sub re-evaluates whether that relay should carry its filters.

## Event delivery and dedup

`NDKSubscriptionManager.dispatchEvent` (`subscription/manager.ts:75-120`) is the **single matching point** for all incoming events:

1. `matchFilters(sub.filters, event)` against every active subscription.
2. For each match, if `sub.exclusiveRelay && sub.relaySet`: verify event came from a relay in the set (or from cache where a known-relay-list overlaps; or from optimistic publish if allowed).
3. Call `sub.eventReceived(event, relay)`.

Per-subscription dedup is via `sub.eventFirstSeen` (set per subscription) — so a new subscription joining mid-stream sees events even if another subscription already received them (`8b9a37cb` fix).

`seenEvents` is global, LRU 10k entries / 5min TTL (`manager.ts:14-19`, fix `e5901c98`). Tracks which relays delivered which event; supports `addRelay` semantics on a discovered NDKEvent.

## Cached events bypass seenEvent

`3215e51e` removed the `seenEvent()` call from cached event paths — saves ~0.24-0.64ms per event, dramatic at scale (1.4s saved on 5700 events).

## Lifecycle

- `sub.stop()` — emits `close`, removes pool monitor, calls onStopped (subManager auto-cleanup).
- `sub.start()` is idempotent only at the cache level; calling twice re-fires cache then re-opens REQs.
- subManager auto-removes on `close` event or `onStopped` (`manager.ts:31-37`).

## Exclusive relay subscriptions

`opts.exclusiveRelay: true` + explicit `relayUrls`/`relaySet`: subscription only accepts events whose relay provenance is in the declared set. Used to enforce "this subscription is for relay X only" semantics even though other subscriptions may pull the same event from elsewhere.
