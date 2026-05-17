# Missing Features for NMP (Nostr Multi-Platform)

What NDK provides, what NMP M2 needs, and the delta.

## Top 3 architecturally-load-bearing gaps

### 1. Reactive subscription auto-rewire when followSet changes — DOES NOT EXIST in core

**Claim under test**: "Auto-update of open subscriptions when the follow list changes (no app code involved)."

**Reality**:
- Core NDK: filters are immutable post-`subscribe()`. There is no follow-list watcher. The closest mechanism (`outboxTracker.on("user:relay-list-updated", …) → refreshRelayConnections`) only adds *relays*, not *authors*.
- Svelte: the appearance of "for free" rewire comes from Svelte 5 runes (`$derived` + `$effect` in `svelte/src/lib/builders/subscription.svelte.ts:164-177`). When the caller's filter factory reads `ndk.$follows`, the framework re-runs it and `restart()` is called (full stop+start, not a delta patch). Caller code IS involved.
- React: `useSubscribe` requires explicit `dependencies` array (`react/src/subscribe/hooks/subscribe.ts:110`). Without `[follows]` in deps, no rewire.

**What NMP needs to build**:
- A Swift/Kotlin equivalent of the Svelte builder: take a filter-producing closure that's evaluated reactively against `followSet` (and other session signals). On signal change, stop + restart the underlying NDK subscription with new filters.
- Or, for opinionated simplicity: a dedicated `FollowsFeed` primitive that internally listens to `session.followSet` and manages its own subscription churn.
- Either way: this is at least one file (~200 LOC) of NMP-specific glue per platform — AND the framework-magic contract (`docs/design/framework-magic.md`) must promise the app dispatches zero code while the kernel does the stop/restart on kind:3 arrival.

### 2. No delta-patch subscription updates

When a follow is added, NMP cannot say "open subscriptions, please also include carol's events." It must:
- Stop the current subscription.
- Open a new one with `authors: [...old, carol]`.
- Accept the bounce: brief gap in incoming events, re-EOSE, re-fetch from relays.

For 500-follow feeds with a slow connection, this is user-visible. A "patch authors" API on NDKSubscription would help; it doesn't exist.

Workaround pattern Svelte uses: keep the eventMap intact across restart, dedupe by id, sort. Visual continuity at the cost of (possibly) re-receiving events.

### 3. No persisted outbox tracker

`OutboxTracker.data` is an in-memory LRU (`core/src/outbox/tracker.ts:62-65`). On every cold app start, NDK re-fetches relay lists for every author the user cares about. For mobile NMP, this is a cold-start cost ~= 1 round-trip to outbox relays per ~400 authors.

Source explicitly notes the gap: `// TODO: The state of this tracker needs to be added to cache adapters` (`tracker.ts:48-49`).

NMP should:
- Persist `OutboxItem` data per-author in its cache (alongside kind 10002 events themselves).
- Pre-populate the tracker on session restore so the first subscription wave has its routing pre-warmed.

## Other notable gaps for NMP M2

### Session monitor for React, but not for raw NDK

`react/src/session/store/start-session.ts` and the React `useNDKSessionMonitor` hook are React-only conveniences. NMP will need to replicate "auto-save on store change, auto-restore on app start" logic without the React hook layer. The sessions package gives the building blocks (`PersistenceManager.restore/persist`) but the "monitor" wiring is in react/mobile.

### Mute filtering only at app-display level

`ndk.muteFilter` is a function. Subscription manager doesn't filter by it; only the UI layer (React's `useSubscribe`) explicitly checks `muteFilter(event)`. NMP needs to apply this at its own delivery boundary.

### WoT not persisted

`NDKWoT` graph is in-memory. Rebuild on every app launch unless caller serializes (no helper). For mobile, depth-2 graph build is 5-15s minimum. Persist the graph yourself.

### No first-class "session is fully loaded" signal

There's no event for "follows, mutes, relay list, profile all arrived." Each kind arrives independently. NMP UX should likely synthesize this signal by tracking which expected kinds have produced events.

### NIP-55 (Amber) only; no iOS external-signer story

`mobile/src/signers/nip55.ts` covers Android. No iOS equivalent in tree. NMP iOS will need to either ship its own signer or interop with another iOS bunker-style app via URL scheme.

### Outbox model write-side gaps

`calculateRelaySetFromEvent` doesn't fully account for relays where specific hashtags are popular (`relay/sets/calculate.ts:18-19` TODO). For tag-heavy posts (e.g., #bitdevs), publishing is suboptimal.

### Relay scoring is a stub

`getTopRelaysForAuthors` has TODO to incorporate quality metrics. Currently ranks by author-count only.

### No NIP-65-aware "this account's read relays for THIS account's feed"

Sessions store fetches `relayList` for the user, but doesn't automatically use it to compute *the user's own read relays* as the default reading pool for their feed. NMP can connect this dot.

### Sync package: still requires explicit opt-in

`NDKSync.sync` / `syncAndSubscribe` are not the default path. Regular `ndk.subscribe` doesn't try negentropy. For large historical fetches (e.g., loading the last week of a 500-follow feed cold), NMP should reach for sync directly.

### No background-refresh API

There's no scheduled "tick" or "fetch since last seen" helper. NMP needs to build its own background-refresh loop using `since: lastSeen` filters.

### No multi-relay-quality A/B fan-out

When the outbox model picks 2 relays per author, those 2 may both be slow. No automatic fallback or expansion mid-stream. NMP could add: monitor per-relay event-arrival rate; if below threshold for a given author, expand to N=3.

## Things NMP gets free that bear noting

- Per-subscription dedup (correct cache-less).
- LRU-bounded seenEvents (no leak).
- Stale-connection auto-reconnect with 30s capped backoff (sleep/wake friendly).
- Auto-tracking of new authors as they appear in filter `authors[]`.
- Per-relay author intersection (filter splitting).
- Pool monitor: new relays joining the pool auto-subscribed for matching subs.
- Signer serialization via `{type, payload}` round-trip.
