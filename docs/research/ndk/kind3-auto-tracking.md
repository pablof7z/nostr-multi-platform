# Kind:3 Auto-Tracking and Subscription Auto-Update

This file documents the two NMP-load-bearing claims: (1) native listening to kind:3 changes for the active user, and (2) auto-update of open subscriptions when the follow list changes.

The honest answer: **there is no unified "kind:3 → auto-rewire" mechanism in core NDK**. There are three different layers, each doing part of the job. NMP needs to know exactly which layer does what.

## Mechanism 1: Active-user kind:3 listener (session layer)

Lives in `@nostr-dev-kit/sessions` (and the parallel React port `@nostr-dev-kit/react/session`). NOT in core.

### What's listened for

On `sessions.login(signer, { follows: true })`, the session manager opens **one long-lived REQ subscription** on the active user's pubkey for a configurable kind set:

```
sessions/src/store.ts:184-194
const subscription = ndk.subscribe(
    { kinds, authors: [pubkey] },
    { closeOnEose: false, subId: "session" },
    { onEvent: (event) => handleIncomingEvent(event, pubkey, constructorMap, get) },
);
```

`kinds` is assembled in `buildSubscriptionKinds()` (`store.ts:417-444`):

```
if (opts.follows)       kinds.push(NDKKind.Contacts);     // 3
if (opts.mutes)         kinds.push(NDKKind.MuteList);     // 10000
if (opts.blockedRelays) kinds.push(NDKKind.BlockRelayList); // 10001
if (opts.relayList)     kinds.push(NDKKind.RelayList);    // 10002
if (opts.wallet)        kinds.push(NDKKind.CashuWallet, NDKKind.CashuMintList);
```

`closeOnEose: false` keeps it open indefinitely. Every newer kind:3 from any connected relay flows back through `onEvent`.

### What happens on a new kind:3

`handleIncomingEvent` (`store.ts:450-487`) dispatches by kind. For contacts:

```
sessions/src/store.ts:492-512
function handleContactListEvent(event, pubkey, getState) {
    const session = getState().sessions.get(pubkey);
    if (!session || event.kind === undefined) return;

    const existingEvent = session.events.get(event.kind);
    if (existingEvent) {
        if (existingEvent.id === event.id) return;
        if ((existingEvent.created_at ?? 0) > (event.created_at ?? 0)) return;
    }

    const followSet = new Set<Hexpubkey>();
    for (const tag of event.tags) {
        if (tag[0] === "p" && tag[1] && isValidPubkey(tag[1])) {
            followSet.add(tag[1]);
        }
    }

    session.events.set(event.kind, event);
    getState().updateSession(pubkey, { followSet, events: new Map(session.events) });
}
```

Three guarantees worth noting:
- **Idempotent on event.id** — same event won't churn the store.
- **Monotonic by created_at** — older replays from a slow relay are dropped.
- **Strict pubkey validity** — `isValidPubkey()` rejects malformed `p` tags.

Result: `session.followSet` is a `Set<Hexpubkey>` that always reflects the newest kind:3 received from any relay. Stored in a zustand store, so any consumer subscribed to that slice gets a fresh reference (`new Map(session.events)` and a fresh `followSet`) every update.

### React port

Same logic at `react/src/session/store/start-session.ts:44-47`. Exposed via `useFollows()` hook:

```
react/src/session/hooks/index.ts:59-63
export const useFollows = (): Set<Hexpubkey> => {
    return useNDKSessions((s) =>
        s.activePubkey ? (s.sessions.get(s.activePubkey)?.followSet ?? EMPTY_SET) : EMPTY_SET,
    );
};
```

Component re-renders when the selector's referential identity changes.

### Svelte port

`svelte/src/lib/ndk-svelte.svelte.ts:535-538` exposes `ndk.$follows` as a reactive Array-with-methods that wraps the session's `followSet`. Touching `ndk.$follows` inside a `$derived` or `$effect` tracks the underlying signal.

### What this layer does NOT do

It does NOT mutate any other open subscription. Updating `session.followSet` is a pure state write. The caller still has to read that state and act on it.

## Mechanism 2: Open-subscription "auto-update" when follows change

This is the user's second claim. The honest version:

### Core NDK has no follow-list rewire

Search confirmed: no code in `core/` watches `session.followSet`, watches any "follows changed" event, or mutates `subscription.filters.authors` on the fly. Filters are immutable after `ndk.subscribe()` returns; the only post-creation mutation is `refreshRelayConnections` adding relays (see Mechanism 3 below).

### Svelte gets it "for free" via runes

`svelte/src/lib/builders/subscription.svelte.ts:164-177`:

```
$effect(() => {
    const newFilters = derivedFilters;
    const newNdkOpts = derivedNdkOpts;
    if (newFilters.length === 0) { stop(); return; }
    currentFilters = newFilters;
    currentNdkOpts = newNdkOpts;
    restart();
});
```

`derivedFilters` is a `$derived.by(() => ...)` computed from a caller-supplied factory. If the factory reads `ndk.$follows`, Svelte's signal graph notices, the `$derived` invalidates, the `$effect` re-fires, `restart()` is called — which does `subscription?.stop()` followed by a brand-new `ndk.subscribe(currentFilters, ...)`. Not a delta patch; a full teardown and rebuild.

So the developer writes:

```
const feed = ndk.$subscribe(() => ({
    filters: [{ kinds: [1], authors: ndk.$follows, limit: 50 }]
}));
```

and when `followSet` mutates, the closure re-evaluates, the new authors array is captured, the subscription is restarted. **Magic provided by Svelte 5 runes, not by NDK.**

### React gets it ONLY if the caller wires deps

`react/src/subscribe/hooks/subscribe.ts:36-110`:

```
useEffect(() => {
    if (!ndk || !filters) return;
    if (subRef.current) { subRef.current.stop(); subRef.current = null; }
    // ... setupSubscription ...
}, [ndk, muteFilter, !!filters, ...dependencies]);
```

`dependencies` is an explicit third parameter the caller must populate. Pattern:

```
const follows = useFollows();
const followsArray = useMemo(() => Array.from(follows), [follows]);
useSubscribe(
    [{ kinds: [1], authors: followsArray }],
    {},
    [followsArray] // <-- required
);
```

Without that, the effect won't re-fire and the subscription stays stale. This is documented inline (lint biome-ignore comment).

### Implication for NMP

NMP (Swift/Kotlin native, no JS framework runtime) is in the same boat as React: there is no implicit reactivity. Whatever client owns the open feed subscription has to:

1. Observe the session's follow-list signal (Combine/Flow).
2. When it changes, call `subscription.stop()` and re-subscribe with the new authors.

This is not "no app code involved." It is "framework-glue code involved."

**For NMP, the framework-magic contract should hide this glue entirely behind a primitive like `FollowsFeed` or a generated wrapper like `useFollowsFeed()` — so the app DOES dispatch zero code while the kernel does the stop/restart on kind:3 arrival.** This is the load-bearing NMP-side addition; see `docs/design/framework-magic.md` for the contract.

## Mechanism 3: Relay-set rewire (the real auto-update in core)

The closest thing core NDK has to auto-update is **adding relays** to an open subscription when a tracked author's NIP-65 list arrives:

```
core/src/ndk/index.ts:458-471
this.outboxTracker.on("user:relay-list-updated", (pubkey, _outboxItem) => {
    for (const subscription of this.subManager.subscriptions.values()) {
        const isRelevant = subscription.filters.some(f => f.authors?.includes(pubkey));
        if (isRelevant && typeof subscription.refreshRelayConnections === "function") {
            subscription.refreshRelayConnections();
        }
    }
});
```

`refreshRelayConnections` (`subscription/index.ts:787-812`):

```
public refreshRelayConnections(): void {
    if (this.relaySet && this.relaySet.relays.size > 0) return; // skip if explicit relaySet
    const updatedRelaySets = calculateRelaySetsFromFilters(
        this.ndk, this.filters, this.pool, this.opts.relayGoalPerAuthor,
    );
    for (const [relayUrl, filters] of updatedRelaySets) {
        if (!this.relayFilters?.has(relayUrl)) {
            this.relayFilters?.set(relayUrl, filters);
            const relay = this.pool.getRelay(relayUrl, true, true, filters);
            relay.subscribe(this, filters);
        }
    }
}
```

Three properties to understand:

- **Only adds.** Never removes relays that became irrelevant.
- **Only triggered by relay-list arrivals.** Not by kind:3 changes.
- **Bypassed when `relaySet` is explicit** — if your subscription was opened with a fixed `relaySet`, no auto-refresh.

So this mechanism solves: "I subscribed to authors=[alice, bob] before knowing where they publish; once their NIP-65 lists arrive, also subscribe on their write relays." It does NOT solve: "alice just followed carol; also fetch carol's content."

## Summary table

| Question | Answer | File:line |
|---|---|---|
| Does NDK listen for new kind:3 events for the active user? | Yes, via sessions package, when `follows: true` | `sessions/src/store.ts:184-194`, `462-465` |
| Is `session.followSet` updated automatically? | Yes, on every newer kind:3 | `sessions/src/store.ts:492-512` |
| Do open subscriptions auto-update their `authors` filter when followSet changes? | No in core. Yes-in-Svelte via runes. No in React without explicit deps. | `svelte/.../subscription.svelte.ts:164-177`; `react/.../subscribe.ts:110` |
| Do open subscriptions auto-add relays when an author's NIP-65 list arrives? | Yes, via `refreshRelayConnections` | `core/src/ndk/index.ts:459-471`; `core/src/subscription/index.ts:787-812` |
| Are removed authors / removed relays handled? | No. Filters are append-only at runtime. | — |
