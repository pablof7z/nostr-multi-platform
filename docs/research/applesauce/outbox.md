# Applesauce — Outbox Model (NIP-65 routing)

> Source: `/private/tmp/nostr-research/applesauce` @ `da5ec22b`.

## TL;DR

Applesauce's outbox is not a relay-resolver service. It is a **reactive composition of three RxJS operators** over the EventStore. There is no central "outbox service" — there is just `OutboxModel = contacts() ⇒ includeMailboxes() ⇒ selectOptimalRelays()`. Every consumer that wants outbox routing subscribes to the model directly.

## 1. The three observables that make it work

### `MailboxesModel(user)` — `packages/core/src/models/mailboxes.ts:8-23`

```ts
return events.replaceable({ kind: kinds.RelayList, pubkey: user.pubkey, relays: user.relays })
  .pipe(map(event => event && { inboxes: getInboxes(event), outboxes: getOutboxes(event) }));
```

Reads a kind:10002 via the generic `replaceable()` query-builder and projects into `{inboxes, outboxes}`. Re-emits on any kind:10002 update.

### `ContactsModel(user)` — `packages/core/src/models/contacts.ts:10-20`

```ts
return events.replaceable({ kind: kinds.Contacts, pubkey: user.pubkey, relays: user.relays })
  .pipe(watchEventUpdates(events), map(e => e ? getContacts(e) : []));
```

`getContacts` returns the merge of public p-tags **and** unlocked hidden p-tags (`helpers/contacts.ts:56-58`). The `watchEventUpdates` operator (`observable/watch-event-updates.ts:6-17`) re-emits when `update$` fires for the latest event id — this is how hidden-tag unlocking propagates without a new kind:3 arriving (gotcha: see §G6 of gotchas.md).

### `OutboxModel(user, opts)` — `packages/core/src/models/outbox.ts:14-24`

```ts
return store.contacts(user).pipe(
  opts?.blacklist ? ignoreBlacklistedRelays(opts.blacklist) : identity,
  includeMailboxes(store, opts.type),
  map(users => selectOptimalRelays(users, opts)),
);
```

That's the entire outbox model.

## 2. `includeMailboxes` — the switchMap that does the heavy lifting

`packages/core/src/observable/relay-selection.ts:19-49`:

```ts
return switchMap((contacts) =>
  combineLatest(
    contacts.map((user) =>
      store.replaceable({ kind: 10002, pubkey: user.pubkey }).pipe(
        map((event) => {
          if (!event) return user;
          const relays = type === "outbox" ? getOutboxes(event) : getInboxes(event);
          if (!relays) return user;
          return addRelayHintsToPointer(user, relays);
        }),
      ),
    ),
  ),
);
```

Three properties matter:

1. **Per-contact independent subscription.** Each contact becomes its own `replaceable()` subscription via the shared model cache. Following a new user triggers exactly one new kind:10002 subscription (none if another model already had that user mailbox-tracked).
2. **Graceful degradation on missing mailbox.** If no kind:10002 has loaded yet, the original `ProfilePointer` is returned with whatever relays it had. The outbox model never blocks on missing mailboxes; it routes with partial knowledge and re-emits when the kind:10002 arrives.
3. **Re-emission on every change.** `combineLatest` re-emits on any inner change. NMP must size its planner debouncer accordingly — naive re-resolution on each kind:10002 will thrash; `OutboxModel` itself doesn't debounce, downstream consumers do via the model-cache 60s keep-warm.

## 3. `selectOptimalRelays` — greedy set-cover heuristic

`packages/core/src/helpers/relay-selection.ts:14-93`. Sketch:

1. Filter out users with zero relays.
2. Build a "popularity" map: relay → count of users mentioning it (line 21-25).
3. Loop until `selection.size >= maxConnections` or pool is exhausted:
   - For each pool user, count how many of their relays are not yet selected.
   - For each candidate relay, compute coverage = `users-mentioning-it / pool-size`.
   - Sort by `score(relay, coverage, popularity)` if provided, else by coverage.
   - Pick the top relay; add to selection.
   - If `maxRelaysPerUser` is set, increment per-user count and drop users at the limit.
4. Project original users → users with `relays` filtered to the selected set.

The pluggable `score` function (`SelectOptimalRelaysOptions.score` at line 9-10) is critical — it's how an app injects WoT bias, RTT, blocklists, paid-relay preference, etc.

The TODO at line 89 acknowledges a known imprecision: `maxRelaysPerUser` is enforced during the greedy pass but not as a post-filter, so a user can end up with more than `maxRelaysPerUser` final relays if their first picks happened to cover the pool.

## 4. Modifier operators

`packages/core/src/observable/relay-selection.ts`:

- `ignoreBlacklistedRelays(blacklist | Observable<blacklist>)` (lines 52-61) — pipeable, supports a reactive blacklist via `combineLatestWith`.
- `includeFallbackRelays(fallbacks | Observable<fallbacks>)` (lines 64-73) — fills in `[]` users with fallback relays.
- `filterOptimalRelays(maxConnections, maxRelaysPerUser)` (lines 76-91) — standalone operator version of the final `selectOptimalRelays` step.

## 5. From `ProfilePointer[]` to actual REQs — `OutboxMap`

`packages/core/src/helpers/relay-selection.ts:111-145`:

- `OutboxMap = Record<string, ProfilePointer[]>` — relay URL → users routed there.
- `groupPubkeysByRelay(pointers)` (lines 118-132) — pivots `ProfilePointer[]` to `OutboxMap`.
- `createFilterMap(outboxMap, filter)` (lines 138-145) — pivots to `{ relay: { authors: [pubkeys], ...filter } }` for batched per-relay REQs.

This is the bridge from the reactive model output to a relay-pool subscription.

## 6. Timeline loaders consume the outbox

`packages/loaders/src/loaders/timeline-loader.ts`:

- `loadBlocksFromOutboxMap(pool, outboxes$, filter, opts)` (lines 290-312) — accepts an `Observable<OutboxMap>` and re-projects to a `FilterMap` on every change. Internally caches per-relay loaders so unchanged relay+filter pairs reuse their existing forward/backward block state (line 269-281).
- `loadBlocksFromOutboxMapCache` (lines 315-352) — same but for the local cache, with a single merged-pubkey query rather than per-relay splitting.
- `createOutboxTimelineLoader` (lines 455-477) — public wrapper that combines cache and relay loaders.

The `cache` at lines 269-281 is the **incremental-resubscription cache**: when the OutboxMap changes (e.g., one contact's mailbox updated), only the affected relays get new loaders; the rest reuse cached forward/backward cursor state. NMP's planner must do the same to avoid full re-pagination on every kind:10002.

## 7. Writes / publishing

Applesauce's `ActionContext.publish` (`packages/actions/src/action-runner.ts:71-95`) accepts an optional `relays` arg. The actions themselves resolve outbox before calling publish:

```ts
// packages/actions/src/actions/contacts.ts:6-13
async function modifyContacts({ user }: ActionContext) {
  const [event, outboxes] = await Promise.all([
    user.replaceable(kinds.Contacts).$first(1000, undefined),
    user.outboxes$.$first(1000, undefined),
  ]);
  return [event ? ContactsFactory.modify(event) : ContactsFactory.create(), outboxes];
}

// :17-22
export function FollowUser(pointer): Action {
  return async (context) => {
    const [factory, outboxes] = await modifyContacts(context);
    const signed = await factory.addContact(pointer).sign(context.signer);
    await context.publish(signed, outboxes);
  };
}
```

Note that this is **the caller's responsibility**, not the framework's — Applesauce does not have a global "publish-uses-outbox-by-default" hook. NMP's spec `aim.md:123` ("by default and automatically") is a stronger guarantee than Applesauce delivers; the spec's design is correct, but NMP cannot just copy Applesauce's `publish(event, relays?)` — it must default to outbox resolution and require an opt-out, not opt-in.

## 8. Inbox routing for DMs

Symmetrically, `MailboxesModel` returns `inboxes` and `includeMailboxes(store, 'inbox')` does the same composition for the inbox side. NMP's deferred DM milestone needs to wire publishes to a recipient via `addressable -> recipient kind:10002 -> inboxes`. Applesauce expects the caller to do this manually per-action; NMP should bake it into the publish path itself when DMs un-defer.

## 9. What Applesauce intentionally does NOT do

- **No relay scoring beyond user-supplied score().** No RTT tracking, no NIP-66 monitor support, no per-relay error rate. The `applesauce-relay/liveness.ts` package (175 LOC) tracks healthy/unhealthy/dead but doesn't feed back into outbox selection automatically.
- **No write-set bias.** `OutboxModel` projects every contact onto a single max-N relay set, regardless of which contacts the consumer is actually reading. There's no "filter the outbox to only contacts I'm displaying right now" — that's the consumer's job.
- **No outbox cache TTL or staleness check.** Once `MailboxesModel` has emitted, only `update$`/`insert$` for that specific kind:10002 changes it. There's no periodic refresh.
- **No fallback to legacy NIP-02 relay JSON.** `getRelaysFromContactsEvent` exists at `helpers/contacts.ts:19-38` and is parsed by `OutboxModel` callers manually if they want to fall back, but `MailboxesModel` strictly reads kind:10002. The user must opt in via the `includeLegacyWriteRelays` modifier (living in `applesauce-loaders/src/operators/outbox-modifiers.ts`).

## 10. NMP implications

1. The outbox engine in `nmp-core` must be reactive at every layer, not just at "give me a relay list" boundary. Applesauce's `OutboxModel` re-emits on contacts-change AND on per-contact-mailbox-change. A snapshot API will not suffice for the spec's "subscriptions automatically re-resolve" guarantee (`aim.md:125`).
2. Per-relay loader caching (`timeline-loader.ts:269-281`) is non-trivial but necessary. Without it, a single new contact triggers full backward-pagination on every relay in the resulting OutboxMap, not just the new ones.
3. `selectOptimalRelays`'s `score` callback hook is the right abstraction for WoT/NIP-66/RTT injection. NMP's `OutboxState` should expose the same shape.
4. The "publish to outbox + recipient inboxes by default" guarantee is **stronger than Applesauce's**. Don't copy Applesauce's caller-responsibility model — bake it into the planner.
