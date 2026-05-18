# Applesauce — app/default/indexer relay handling

> **Status:** Research findings
> **Date:** 2026-05-18
> **Source pin:** applesauce master, cloned 2026-05-18. File:line references below are against that snapshot.
> **Builds on:** `docs/design/ndk-applesauce-lessons.md`

## Summary

Applesauce **does not** have a single, framework-level "app relay" or "default relay" primitive. It has a sharply scoped vocabulary instead:

- **`RelayPool`** is connection-bookkeeping only — no defaults baked in.
- **`extraRelays`** = always-fetch additive relays, attached to *loaders*, not to the pool.
- **`lookupRelays`** = fallback indexer relays, scoped to the **AddressLoader** (replaceable events: kind:0, kind:10002, addressables).
- **`OutboxMap` / `outboxSubscription`** = NIP-65 outbox routing as an explicit data structure, not a hidden default.
- **`setFallbackRelays` / `includeFallbackRelays`** = cold-start primitive: substitutes fallbacks *only when a user has zero relays in their NIP-65*.
- **Kind 10086 `LookupRelayList`** — applesauce defines (and ships factories/casts for) a user-published "indexer relays" list. Not in upstream NIPs.

Versus NMP's five-role model:

| NMP role | Applesauce analogue | Status |
| --- | --- | --- |
| 1. NIP-65 outbox | `getOutboxes` + `OutboxMap` + `pool.outboxSubscription` | Direct match |
| 2. NIP-65 inbox | `getInboxes` + `includeMailboxes(store, "inbox")` | Direct match |
| 3. Indexer always-on (k0, k3, k1xxxx, r/w) | `lookupRelays` on AddressLoader **read side only**; no write-side counterpart | **Partial — gap on writes** |
| 4. App fallback (cold-start + additive) | `setFallbackRelays` (cold-start) + `extraRelays` (additive) | Two distinct primitives |
| 5. NIP-51 specialised | Cast/factory classes for k:10006/10007/10012/10050/10086 | Modelled but not auto-routed |

Key delta from NDK: applesauce **demotes** automatic relay-policy magic to a per-loader composition. The application developer wires `lookupRelays`, `extraRelays`, and the `OutboxMap` explicitly. NDK hides those decisions in `NDKRelaySet.fromNostrEvent(...)`; applesauce surfaces them.

## 1. Concepts and naming

All paths relative to `applesauce/` repo root.

### 1.1 `RelayPool` — connection-only, **no defaults**

`packages/relay/src/pool.ts`

```ts
constructor(public options?: RelayOptions)

relay(url: string): Relay {
  url = normalizeURL(url);
  let relay = this.relays.get(url);
  if (relay) return relay;
  relay = new Relay(url, this.options);
  this.relays.set(url, relay);
  this.relays$.next(this.relays);
  this.add$.next(relay);
  return relay;
}
```

The constructor takes only `RelayOptions` (timeouts, reconnect policy, NIP-42 auth behaviour). There is **no** `defaultRelays`, `bootstrapRelays`, or `indexerRelays` constructor argument. Relays are created on-demand by URL.

### 1.2 `extraRelays` — additive, per-loader

`packages/loaders/src/loaders/address-loader.ts:154` and identical fields on `event-loader.ts:111`, `tag-value-loader.ts:36`, `user-lists-loader.ts:27`, `social-graph.ts:36`.

```ts
/** An array of relays to always fetch from */
extraRelays: string[] | Observable<string[]>;
```

Two semantics in practice:

- **AddressLoader / EventLoader:** queried as one of an ordered fallback sequence (see §3).
- **UserListsLoader / TagValueLoader:** *merged into the per-author relay set*. E.g. `user-lists-loader.ts:47`:

  ```ts
  const relays = mergeRelaySets(user.relays, extraRelays);
  ```

  So for these loaders, `extraRelays` is genuinely *additive* to NIP-65.

### 1.3 `lookupRelays` — indexer-style fallback for replaceables

`packages/loaders/src/loaders/address-loader.ts:155-156`

```ts
/** Fallback lookup relays to check when event cant be found */
lookupRelays: string[] | Observable<string[]>;
```

Critical scoping: `lookupRelays` exists **only on the AddressLoader** (which handles replaceable events: kind:0 profiles, kind:10002 mailboxes, kind:30000–39999 addressables). The plain `EventLoader` (event-by-id) does not accept it (`event-loader.ts:99-112`). The `UnifiedEventLoader` accepts it but forwards it only to the AddressLoader branch (`unified-event-loader.ts:35`).

### 1.4 NIP-65 outbox primitives

`packages/core/src/helpers/relay-selection.ts`

```ts
// line 112
export type OutboxMap = Record<string, ProfilePointer[]>;

// line 118
export function groupPubkeysByRelay(pointers: ProfilePointer[]): OutboxMap {
  const outbox: OutboxMap = {};
  for (const pointer of pointers) {
    if (!pointer.relays) continue;
    for (const relay of pointer.relays) {
      if (!outbox[relay]) outbox[relay] = [];
      outbox[relay]!.push(pointer);
    }
  }
  return outbox;
}

// line 135
export const createOutboxMap = groupPubkeysByRelay;
```

And `packages/relay/src/pool.ts:216-229`:

```ts
/** Open a subscription for an {@link OutboxMap} and filter */
outboxSubscription(
  outboxes: OutboxMap | Observable<OutboxMap>,
  filter: Omit<Filter, "authors">,
  options?: ...,
): Observable<NostrEvent> {
  const filterMap = isObservable(outboxes)
    ? outboxes.pipe(map((outboxes) => createFilterMap(outboxes, filter)))
    : createFilterMap(outboxes, filter);
  return this.subscriptionMap(filterMap, options);
}
```

The `OutboxModel` (`packages/core/src/models/outbox.ts:14-24`) composes `contacts → ignoreBlacklistedRelays → includeMailboxes → selectOptimalRelays` into a reactive `Observable<ProfilePointer[]>`.

### 1.5 Cold-start fallback primitive

`packages/core/src/helpers/relay-selection.ts:95-101`

```ts
/** Sets relays for any user that has 0 relays */
export function setFallbackRelays(users: ProfilePointer[], fallbacks: string[]): ProfilePointer[] {
  return users.map((user) => {
    if (!user.relays || user.relays.length === 0) return { ...user, relays: fallbacks };
    else return user;
  });
}
```

Reactive operator at `packages/core/src/observable/relay-selection.ts:64-73`:

```ts
/** Sets fallback relays for any user that has 0 relays */
export function includeFallbackRelays(
  fallbacks: string[] | Observable<string[]>,
): MonoTypeOperatorFunction<ProfilePointer[]> {
  return pipe(
    combineLatestWith(isObservable(fallbacks) ? fallbacks : of(fallbacks)),
    map(([users, fallbacks]) => setFallbackRelays(users, fallbacks)),
  );
}
```

Note: this operator is **not** automatically included in `OutboxModel`. The app must compose it. See §4.

### 1.6 Kind 10086 — applesauce-defined indexer relay list

`packages/common/src/helpers/relay-list.ts:6-7`

```ts
/** Indexer / lookup relays: where to fetch or publish kinds 0 and 10002 (NIP-51 `relay` tags). */
export const LOOKUP_RELAY_LIST_KIND = 10086;
```

Cast class at `packages/common/src/casts/relay-lists.ts:101-107`:

```ts
/** Class for lookup / indexer relays lists (kind 10086) */
export class LookupRelayList extends RelayListBase<LookupRelayListEvent> { ... }
```

Factory at `packages/common/src/factories/relay-lists.ts:92-106`:

```ts
/** A factory class for building kind 10086 lookup / indexer relays list events */
export class LookupRelayListFactory extends NIP51RelayListFactory<...> { ... }
```

Web search for "NIP-51 kind 10086 lookup indexer relays" returned no upstream NIP defining 10086. `nips.nostr.com/51` lists 10006/10007/10012/10015 etc., but not 10086. Conclusion: **applesauce-invented convention**, not a ratified NIP. The changeset `.changeset/forty-socks-lay.md` is a one-liner: "Add support for NIP-51 kind 10086 lookup relay lists" — no further rationale.

### 1.7 NIP-51 specialised lists modelled

From `packages/common/src/casts/relay-lists.ts` and `factories/relay-lists.ts`:

- `FAVORITE_RELAYS_KIND = 10012` → `FavoriteRelays` cast + `FavoriteRelaysListFactory`
- `kinds.SearchRelaysList = 10007` → `SearchRelays`
- `kinds.BlockedRelaysList = 10006` → `BlockedRelays`
- `kinds.DirectMessageRelaysList = 10050` → `DMRelaysListEvent`
- `LOOKUP_RELAY_LIST_KIND = 10086` → `LookupRelayList`

These are *data-model classes only*. No loader, no `RelayPool` method, and no built-in routing policy consults them. They are surfaced to the app developer, who must wire them in.

## 2. Configuration surface

The canonical setup pattern repeated across the docs (`apps/docs/index.md:117-133`, `core/event-store.md:399-406`, `loading/loaders/unified-loader.md:27-33`, `apps/actions/action-runner.md:270-272`):

```ts
import { createEventLoaderForStore } from "applesauce-loaders/loaders";
import { EventStore } from "applesauce-core";
import { RelayPool } from "applesauce-relay";

const eventStore = new EventStore();
const pool = new RelayPool();

// Connect the event store to the relay pool for automatic event loading
createEventLoaderForStore(eventStore, pool, {
  // Fallback relays to find profiles and NIP-65 events
  lookupRelays: ["wss://purplepag.es/", "wss://index.hzrd149.com/"],
});
```

Worked example from `apps/examples/src/examples/outbox/relay-selection.tsx:30-33`:

```ts
createEventLoaderForStore(eventStore, pool, {
  lookupRelays: ["wss://purplepag.es/", "wss://index.hzrd149.com/", "wss://indexer.coracle.social/"],
  extraRelays: ["wss://relay.damus.io", "wss://nos.lol", "wss://relay.primal.net"],
});
```

For publishing, `apps/docs/apps/actions/action-runner.md:222-236`:

```ts
const pool = new RelayPool();
const defaultRelays = ["wss://relay.damus.io", "wss://nos.lol"];

const publish = async (event, relays) => {
  await pool.publish(relays || defaultRelays, event);
};

const actions = new ActionRunner(eventStore, signer, publish);
```

**Observation:** `defaultRelays` here is a plain `const` owned by the app, not a framework concept. The framework provides the `publish` callback shape but doesn't define what "default" means.

## 3. Routing decisions by kind

### 3.1 No global by-kind routing table

There is no kernel-style `if event.kind == 0 then route to indexer` switch anywhere. Routing is decided by *which loader you use*, which corresponds to *which pointer shape* you feed in:

- `EventPointer { id, relays? }` → `EventLoader` (no `lookupRelays`)
- `AddressPointer { kind, pubkey, identifier?, relays? }` → `AddressLoader` (with `lookupRelays`)
- `LoadableProfile { pubkey, relays? }` → `UserListsLoader` (`extraRelays` merged with user relays)

`UnifiedEventLoader` (`packages/loaders/src/loaders/unified-event-loader.ts:14-40`) dispatches by pointer shape:

```ts
const eventLoader = createEventLoader(pool, {
  followRelayHints: opts?.followRelayHints,
  extraRelays: opts?.extraRelays,
});
const addressLoader = createAddressLoader(pool, {
  followRelayHints: opts?.followRelayHints,
  extraRelays: opts?.extraRelays,
  lookupRelays: opts?.lookupRelays,    // only on the address branch
});
```

So kind:0 and kind:10002 get `lookupRelays` because they are replaceable (AddressPointer). Kind:1 events fetched by ID don't. This is **implicit** routing-by-shape rather than routing-by-kind.

### 3.2 AddressLoader four-step sequence

`packages/loaders/src/loaders/address-loader.ts:160-185`:

```ts
return batchLoader(
  bufferTime(opts?.bufferTime ?? 1000, undefined, opts?.bufferSize ?? 200),
  addressPointerLoadingSequence(
    // Step 1. load from cache if available
    opts?.cacheRequest ? cacheAddressPointersLoader(opts.cacheRequest) : undefined,
    // Step 2. load from relay hints on pointers
    opts?.followRelayHints !== false ? relayHintsAddressPointersLoader(request) : undefined,
    // Step 3. load from extra relays
    opts?.extraRelays ? relaysAddressPointersLoader(request, opts.extraRelays) : undefined,
    // Step 4. load from lookup relays
    opts?.lookupRelays ? relaysAddressPointersLoader(request, opts.lookupRelays) : undefined,
  ),
  ...
);
```

`addressPointerLoadingSequence` (`address-loader.ts:75-107`) walks the steps and only invokes step N if step N-1 didn't return the wanted event. Lookup relays only fire on misses, exactly the "indexer of last resort" pattern.

### 3.3 Write side: no kind-based indexer routing

`packages/actions/src/actions/profile.ts:7-15`:

```ts
export function CreateProfile(content: ProfileContent): Action {
  return async ({ user, signer, publish }) => {
    const existing = await user.replaceable(kinds.Metadata).$first(1000, undefined);
    if (existing) throw new Error("Profile already exists");

    const signed = await ProfileFactory.create().override(content).sign(signer);
    // No outboxes to publish to since this is probably a new user
    await publish(signed);
  };
}

/** An action that updates a kind 0 profile event for a user */
export function UpdateProfile(content: Partial<ProfileContent>): Action {
  return async ({ user, signer, publish }) => {
    const [profile, outboxes] = await Promise.all([
      user.profile$.$first(1000, undefined),
      user.outboxes$.$first(1000, undefined),
    ]);
    if (!profile) throw new Error("Unable to find profile metadata");
    const signed = await ProfileFactory.modify(profile.event).update(content).sign(signer);
    await publish(signed, outboxes);
  };
}
```

`packages/actions/src/actions/mailboxes.ts:15-27` for `CreateMailboxes`:

```ts
await publish(signed, relaySet(getOutboxes(signed)));
```

`AddInboxRelay` (line 39):

```ts
// Publish to both old and new outboxes so the event propagates
await context.publish(signed, relaySet(getOutboxes(signed), oldOutboxes));
```

**Key finding:** kind:0 and kind:10002 writes go **only to the user's own outboxes**. There is no write-side equivalent of `lookupRelays` — no "also publish kind:0 to the indexers." This is asymmetric with the read side. NMP's role-3 ("indexers are always-on, both read AND write") is **the explicit improvement opportunity** the applesauce design leaves on the table.

### 3.4 Recipient-aware writes

`packages/actions/src/actions/legacy-messages.ts` and `wrapped-messages.ts` are recipient-aware (use `getInboxes(recipient)`). That confirms inbox/outbox asymmetry is modelled — but indexer routing is not.

## 4. Cold-start / no-NIP-65 fallback path

Applesauce's cold-start is **not** automatic at the loader level. The primitives exist; the composition is the app's job.

### 4.1 What happens by default

- `OutboxModel` (`packages/core/src/models/outbox.ts:14-24`) returns `ProfilePointer[]` where each user's `.relays` comes from `includeMailboxes(store)` → `getOutboxes(event)` for that user's kind:10002.
- If the user has no kind:10002 in the store yet, `includeMailboxes` emits the original `ProfilePointer` unchanged (relays undefined/empty). See `relay-selection.ts:35-44`:

  ```ts
  map((event) => {
    if (!event) return user;
    const relays = type === "outbox" ? getOutboxes(event) : getInboxes(event);
    if (!relays) return user;
    return addRelayHintsToPointer(user, relays);
  }),
  ```
- The downstream `selectOptimalRelays` will then produce a user with no relays selected.
- `pool.outboxSubscription(...)` ends up with an empty `FilterMap` for that user. **Nothing is fetched.**

### 4.2 What the app must add

To make cold-start work, the app composes `includeFallbackRelays` into the pipeline. From the live example pattern (this is the documented way, though not bundled by default):

```ts
import { OutboxModel } from "applesauce-core/models";
import { includeFallbackRelays } from "applesauce-core/observable";

const routed$ = eventStore
  .model(OutboxModel, user, { maxConnections: 10, maxRelaysPerUser: 3 })
  .pipe(includeFallbackRelays(["wss://purplepag.es/", "wss://index.hzrd149.com/"]));
```

For replaceable-event reads (kind:0, kind:10002 themselves), the `lookupRelays` configured on the loader **always** acts as the discovery fallback regardless of NIP-65 state, because it's step 4 of the AddressLoader sequence.

### 4.3 Practical cold-start sequence for a fresh user

1. App boots, eventStore is empty, no kind:10002 for active user.
2. App requests `eventStore.profile(pubkey)`. AddressLoader fires:
   - Step 1 cache miss.
   - Step 2 relay hints: none on the bare pubkey.
   - Step 3 `extraRelays` — if configured, hits them.
   - Step 4 `lookupRelays` — hits `purplepag.es` / `index.hzrd149.com`. Likely finds the profile.
3. App requests `eventStore.mailboxes(pubkey)` (kind:10002). Same four steps. Lookup relays surface the NIP-65 list if it exists.
4. Now downstream `OutboxModel` for that user starts returning real relays, and the rest of the app's reads route correctly.

This is a clean cold-start *for reads*. For *writes*, applesauce just refuses (`UpdateProfile` throws if no profile exists) or publishes nowhere meaningful (`CreateProfile` has the explicit comment: `// No outboxes to publish to since this is probably a new user` and just calls `publish(signed)` with no relays — the app's `publish` callback decides where).

## 5. Outbox model interaction

### 5.1 Strict layering

The architecture is layered:

1. **Pool layer (`applesauce-relay`)**: dumb connection store. No defaults.
2. **Outbox layer (`applesauce-core/helpers/relay-selection`)**: pure functions over `ProfilePointer[]`. Produces `OutboxMap`.
3. **Loader layer (`applesauce-loaders`)**: composes cache, relay hints, `extraRelays`, `lookupRelays` per request.
4. **Action layer (`applesauce-actions`)**: domain-level writes; calls `publish(event, relays?)` with relays chosen by the action.

### 5.2 Interaction matrix — applesauce semantics

| Question | Answer |
| --- | --- |
| Are `lookupRelays` *added to* NIP-65 outbox relays on reads? | **No.** They are a separate, ordered fallback step (step 4 of AddressLoader sequence). Outbox is consulted via relay hints in step 2; lookup only fires if earlier steps missed. |
| Are `extraRelays` additive to NIP-65 on reads? | **Yes, where used.** `UserListsLoader`, `TagValueLoader` call `mergeRelaySets(user.relays, extraRelays)`. AddressLoader and EventLoader treat `extraRelays` as a separate step, not merged. **Inconsistent across loaders.** |
| Are any defaults consulted for writes? | **No.** Each `Action` chooses relays explicitly. The `publish` callback the app provides may fall back to `defaultRelays`, but that's app-level convention. |
| Is the user's published kind 10086 list consulted by loaders? | **No.** The `LookupRelayList` cast exists, but no loader reads from the user's 10086 to populate its `lookupRelays`. The app would have to wire that. |

### 5.3 `outboxSubscription` is opt-in

`pool.outboxSubscription` is exposed for advanced users who want to manually drive a `FilterMap`-based subscription. The default `pool.subscription(relays, filter)` does NOT consult any outbox map — it just sends the same filter to every relay. So:

- **Default ergonomics:** uniform-fan-out (the "blast every relay" pattern NDK warns against).
- **Outbox-correct ergonomics:** requires the app to build an `OutboxMap` (via `OutboxModel` or `createOutboxMap`) and call `outboxSubscription` explicitly.

This is the most striking design choice in applesauce: **outbox routing is available but not the default**. NDK aspires to make it automatic. Applesauce explicitly demotes it to a composition primitive.

## 6. What applesauce does differently from NDK

Concrete deltas:

- **NDK has a stateful `NDKRelaySet` that lifecycles per subscription.** Applesauce has `OutboxMap`, a plain `Record<string, ProfilePointer[]>` — pure data, no lifecycle. Outbox decisions are reified as values, not as objects with behaviour.
- **NDK's `NDKPool` accepts `explicitRelayUrls` in its constructor** (a built-in "default" concept). Applesauce's `RelayPool` accepts no relay list at all; relays appear when requested.
- **NDK auto-fetches NIP-65 lists** when it sees a new author. Applesauce makes this an explicit composition: the app must call `eventStore.mailboxes(pubkey)` (which uses the AddressLoader, which uses `lookupRelays`).
- **NDK collapses "discover relay list" and "fetch on those relays" into one black box.** Applesauce splits them: `lookupRelays` is for *discovering* (replaceable-event reads); per-user mailbox relays are for *routing live subscriptions* — distinct concepts.
- **NDK's outbox is opt-out (config flag).** Applesauce's outbox is opt-in (`pool.outboxSubscription` vs `pool.subscription`). The "naive uniform fan-out" path is the default.
- **NDK's relay-routing API is methods on a god-object.** Applesauce's is RxJS operators (`includeMailboxes`, `includeFallbackRelays`, `ignoreBlacklistedRelays`, `filterOptimalRelays`) — composable, testable in isolation.
- **NDK does not distinguish `extraRelays` from `lookupRelays`.** Applesauce does: additive-always vs fallback-on-miss are separate parameters with separate ordering.
- **NDK does not model indexer relays as user-published state.** Applesauce ships kind 10086 (`LookupRelayList`) — a user can publish their preferred indexer set, the same way they publish kind:10002.
- **NDK warns about (and partially mitigates) "outbox bugs survive to UI because reads succeed anyway".** Applesauce's design surfaces the choice — you can't accidentally use outbox routing, because it requires calling a different method. This is a tradeoff: more explicit ≈ more correct, but less convenient.
- **NDK has `addExplicitRelay(url)` that can also fetch from it.** Applesauce's equivalent is `pool.relay(url)` (connection only) + adding `url` to a loader's `extraRelays`/`lookupRelays` array — two distinct steps.

## 7. Doctrine statements

Applesauce has **no design-doc level doctrine** about app/indexer relays. There is no blog post, no architecture doc, no glossary entry that names the role. The closest things to doctrine:

### 7.1 The single most informative code comment

`packages/common/src/helpers/relay-list.ts:6`:

```ts
/** Indexer / lookup relays: where to fetch or publish kinds 0 and 10002 (NIP-51 `relay` tags). */
export const LOOKUP_RELAY_LIST_KIND = 10086;
```

This one line is the closest applesauce has to a doctrine statement on the role NMP calls "indexer". It collocates two ideas — *indexer* and *lookup* — and explicitly scopes them to kinds 0 and 10002. (Note: the comment says "publish" but no code path actually publishes there; see §3.3.)

### 7.2 Doc-site framing of `lookupRelays`

`apps/docs/index.md:131`:

```ts
// Fallback relays to find profiles and NIP-65 events
lookupRelays: ["wss://purplepag.es/", "wss://index.hzrd149.com/"],
```

`apps/docs/loading/loaders/address-loader.md:188`:

> "Try fallback lookup relays (if configured)"
>
> "This approach ensures efficient event loading with minimal network requests while providing good fallback options for retrieving replaceable events."

`apps/docs/apps/actions/action-runner.md:76`:

> "Actions can specify which relays to publish to by passing a `relays` array as the second argument to the `publish` function in their context. If no relays are specified, the publish method will use its default behavior (often determined by the user's outboxes or default relays)."

That "user's outboxes **or** default relays" is the only mention of `defaultRelays` in the doctrine, and it's a phrase about app-level convention, not a framework primitive.

### 7.3 Code comment confessing the cold-start gap

`packages/actions/src/actions/profile.ts:14`:

```ts
// No outboxes to publish to since this is probably a new user
await publish(signed);
```

The framework has no answer for "where does a brand-new user publish their first kind:0?" It punts to the app's `publish` implementation.

### 7.4 What is NOT said anywhere

- No statement that indexer relays should be always-on.
- No statement that they should be both read AND write.
- No statement about kind:3 (contact list) routing — applesauce treats kind:3 as an ordinary replaceable, routes via NIP-65 outbox.
- No statement about kind ranges 10000–19999 as a special "universal data" class. The five distinguished kinds (10006/10007/10012/10050/10086) are modelled individually, not as a category.

## 8. What NMP can learn — concrete recommendations

### 8.1 Borrow the named primitives, but redefine where applesauce is fuzzy

For NMP's five roles:

- **Role 1 (NIP-65 outbox) — direct port.** Adopt the `OutboxMap` shape conceptually: `HashMap<RelayUrl, Vec<ProfilePointer>>` keyed by relay, with per-author scoping. Reuse `selectOptimalRelays`-style coverage scoring (popularity + per-user cap). Validate against applesauce's existing test suite by running comparable inputs.

- **Role 2 (NIP-65 inbox) — direct port.** Mirror `includeMailboxes(store, "inbox")`. Treat inbox routing as a strict policy for recipient-addressed events (replies with `p`, kind:4/1059/etc.).

- **Role 3 (indexer always-on, R+W) — improve on applesauce.**
  - On reads: equivalent to `lookupRelays` but **mandatory not opt-in**, and applied to **all** kind:0/kind:3/kind:10000–19999, not just AddressLoader pointer shape. NMP can do this because Rust owns kind classification.
  - On writes: **this is the gap NMP fills.** Indexers must also receive writes of kind:0, kind:3, all NIP-51 lists. Codify it: every replaceable-kind publish goes to `outboxes ∪ indexers`, not just outboxes. The applesauce `UpdateProfile` action publishing only to outboxes is the explicit anti-pattern NMP corrects.
  - Recognise kind 10086 as a candidate user-overridable indexer list, but treat the operator-configured indexer set as the floor (always present even if user doesn't publish 10086).

- **Role 4 (app fallback) — split into two primitives, as applesauce did.**
  - `setFallbackRelays` semantics for cold-start (user has zero NIP-65). Substitutive — only kicks in when the slot is empty.
  - `extraRelays` semantics for additive merge at login. NMP can call them `appAdditiveRelays` to avoid the ambiguity applesauce has (where AddressLoader treats `extraRelays` as a step and UserListsLoader treats it as a merge).
  - Make this distinction visible in the operator config schema. The applesauce doctrine fails to make it explicit — see how `extraRelays` is used inconsistently across loaders.

- **Role 5 (NIP-51 specialised) — adopt the cast-class pattern but auto-route.**
  - Build dedicated routing for kind:10007 (search), kind:10013 (drafts), kind:10102 (good wiki) at the planner stage. Applesauce models these as data classes but the *router doesn't consult them*. NMP's planner should.

### 8.2 Structural lessons from the applesauce architecture

- **Keep `RelayPool` connection-only.** No defaults baked into the transport layer. NMP's `Substrate` mirror should keep relay defaults out of the websocket layer and in the actor's policy layer.
- **Make outbox routing the default, not opt-in.** Applesauce's `pool.subscription` vs `pool.outboxSubscription` split is convenient for the library author but bug-prone for app developers. NMP's safe-path planner should *always* compile to relay-scoped filters.
- **Reify routing decisions as values, not objects.** `OutboxMap` as `Record<RelayUrl, ProfilePointer[]>` is a great pattern — durable, serializable, testable. NMP's `RoutingPlan` should be similarly inspectable.
- **Distinguish "discover the relay list" from "subscribe via those relays."** Applesauce splits them via `lookupRelays` (discovery) vs `OutboxModel` (routing). NMP should preserve this split.
- **Asymmetric publish routing is a bug.** Applesauce reads from indexers but never publishes to them. NMP should fail tests that exhibit this asymmetry.

### 8.3 Specific applesauce constructs worth copying in Rust idiom

- `selectOptimalRelays(users, { maxConnections, maxRelaysPerUser, score? })` — `relay-selection.ts:14`. Greedy coverage-maximising selection with per-user caps. Direct algorithm port.
- `setFallbackRelays(users, fallbacks)` — substitutive cold-start. One-liner Rust function.
- `removeBlacklistedRelays(users, blacklist)` — kind:10006 enforcement.
- `groupPubkeysByRelay(pointers)` — produces the `OutboxMap`. The single most reusable primitive.
- `createFilterMap(outboxMap, filter)` — projects to per-relay filter, automatically attaching `authors` to each relay's filter (`relay-selection.ts:138-145`). This is the wire-level shape NMP's compiler should emit.

### 8.4 What to NOT copy

- The opt-in nature of outbox routing. Make it always-on.
- The asymmetry between read-side and write-side indexer handling.
- `extraRelays` having two different semantics across loaders. Pick one (additive) and stick with it.
- Leaving the cold-start composition (`OutboxModel` + `includeFallbackRelays`) to the app developer. NMP's planner should auto-wire this.
- Modelling kind 10086 as a NIP-51 entry without a write path. If NMP ships indexer-list support, it should publish updates to the indexers themselves (eat your own dog food).

### 8.5 Open questions surfaced by this research

- Should NMP define its own kind for "operator indexer set" distinct from kind 10086 (user-published)? Probably yes — operator config and user preferences are different layers.
- Should the indexer set be writable to per-event-kind (e.g. a different indexer for kind:3 vs kind:0)? Applesauce doesn't split here; the single `lookupRelays` covers all replaceables. NMP can choose finer-grained.
- Kind 10086 has not been ratified as a NIP. NMP should track whether to (a) adopt it as a convention, (b) propose ratification, or (c) define its own. Filing this as a pending decision in `docs/perf/pending-user-decisions.md` is appropriate.
