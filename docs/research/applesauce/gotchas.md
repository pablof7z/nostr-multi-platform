# Applesauce — Gotchas & Bug Archaeology

> Source: `/private/tmp/nostr-research/applesauce` @ `da5ec22b` (unshallowed: 1043 commits).
> Each entry: the bug, the commit, the lesson for NMP.

## G1. NIP-01 tie-break: equal `created_at` causes silent drops

**Commit `90d525af`** ("fix(core): implement NIP-01 tie-break for replaceable events"). Pre-fix, when an incoming replaceable event had the same `created_at` as the stored one, the store rejected the incoming unconditionally. Two clients independently producing a replaceable within the same second each kept only their own; the other was silently dropped on arrival from a relay. The fix (`event-store/event-store.ts:235-301` and the mirrored block in `async-event-store.ts:198-264`) does two passes: pre-insert "does anything beat me?" and post-insert "find the winner across all stored versions and remove losers."

**Lesson:** NMP's `EventStore` insert path **must** implement NIP-01's `(created_at desc, id asc)` tie-break in both directions. Add a regression test that injects two same-second events with different ids and asserts the lex-lower id wins.

## G2. Event claims: counter vs boolean = leak vs over-eviction

**Commit `75ef7d5f`** ("Convert event claims to counter to avoid memory leaks"). Pre-fix: claims were a boolean (`Set<NostrEvent>` semantics). If two subscriptions claimed the same event and one unsubscribed, the boolean cleared and the LRU pruner could evict the event while the second subscription was still bound to it. Fix: refcount via `WeakMap<NostrEvent, number>`. Decrement to zero deletes the entry; non-zero keeps the event pinned.

**Lesson:** NMP's claim-based GC (spec §7.5) must use refcounts, not a presence flag. The `WeakMap` choice is also important — it lets events get GC'd via JS GC if every reference is dropped, which is the safety net for the case where claims are correctly balanced but the consumer holds no other reference.

## G3. Pool methods silently dropped offline relays

**Commit `d2322cee`** ("fix: pool manual methods silently dropping offline relays") + commit `c2bd9a2a` ("Ignore unreachable (ready=false) relays by default"). The pool's `subscription`/`request`/`publish`/`count`/`sync` were calling `group()` without passing `ignoreOffline=false`. `RelayPool.ignoreOffline` defaults to true, so any relay with `ready=false` was silently filtered out. The caller then hung forever waiting for events that could never arrive because no relay was actually subscribed. Later commit `65534509` ("fix(relay): wait for non-ready relays in RelayPool.group") changed semantics again so that not-yet-ready relays are *included* as soon as they become ready, rather than dropped.

**Lesson:** NMP's planner must explicitly handle three states (ready, not-yet-ready, dead) and not collapse them. "Subscription completes with no events" is an ambiguous failure mode — make it a typed error.

## G4. Timeline backward loader: inclusive `until` boundary

**Commit `b03c0d96`** ("Fix timeline backwards loader not handling `until` correctly on filters"). Pre-fix: after loading a block, the next `until` was set to the oldest event's `created_at`, but NIP-01 defines `until` as **inclusive**, so the same event would be returned again at the boundary, causing an infinite loop on singleton blocks. Fix at `loaders/timeline-loader.ts:107`: `if (minCreatedAt !== undefined) cursor = minCreatedAt - 1;`.

**Lesson:** NMP's pagination must subtract 1 from the inclusive boundary. The Applesauce test (`packages/loaders/src/loaders/__tests__/timeline-loader.test.ts`) explicitly checks "singleton event returned at the inclusive until boundary" — copy that test.

## G5. RelayPool.relay() didn't emit add$, breaking RelayLiveness

**Commit `07c2b17b`** ("Fix RelayPool.relay() not emitting add$ breaking RelayLiveness"). `add$.next(relay)` was never called when `relay()` lazily created a new Relay instance, leaving `RelayLiveness.connectToPool()` blind to lifecycle changes. `getState()` returned undefined and `healthy$`/`unhealthy$`/`dead$` never emitted.

**Lesson:** Lifecycle observables and the methods that mutate the underlying collection must be wired together at the point of mutation. NMP's analogue: every place that adds/removes a relay or session or subscription must publish the lifecycle event in the same code path, not "wherever convenient." Add a contract test: every new instance returned by a factory method must have triggered exactly one add event.

## G6. Hidden tags: encrypted content unlock didn't propagate

**Commit `a41f3b70`** ("fix bug with restoring encrypted content did not unlock events"). The encrypted-content cache existed but its restoration on rehydration didn't fire `update$`, so consumers subscribed via `watchEventUpdates` (`observable/watch-event-updates.ts`) never saw the now-unlocked hidden contacts. The `HiddenContactsModel` (`models/contacts.ts:28-36`) explicitly relies on `watchEventUpdates`.

Related: commit `a959bb22` ("Fix many NIP-60 bugs Fix bug with hidden tags cache symbol") and `c9c0aba5` ("strip stale symbol caches in EventFactory.chain()") — the symbol-cache pattern (`hidden-tags.ts`, `cache.ts`) is powerful but creates a category of bugs where a stale cache survives a mutation it should invalidate.

**Lesson:** Any cache-on-symbol pattern in NMP needs an explicit invalidation hook called from every mutation site. The notify-update pattern (`helpers/event.ts:145-150` — `notifyEventUpdate` finds the parent store and calls `update`) is the right shape.

## G7. PasswordSigner unlock-promise has no explicit resolution

**Reading `signers/password-signer.ts:39-74`** (no specific bugfix commit, design observation). `requestUnlock()` creates a deferred and stores it, but `unlock()` doesn't resolve it. Instead, the next call to a method that called `requestUnlock()` re-checks `if (this.key) return`, which now returns immediately. This works because all signing methods `await this.requestUnlock()` *then* re-read `this.key`, but it's a subtle invariant — if a future refactor caches `unlockPromise` and awaits it without re-checking `this.key`, the queued operations will hang forever.

**Lesson:** NMP's encrypted-key-at-rest signer should resolve the promise explicitly in `unlock()` and have a regression test that creates N concurrent operations, then calls unlock, and asserts all N complete.

## G8. AmberClipboardSigner: visibilitychange races with window.open

**Reading `signers/amber-clipboard-signer.ts:65-68`** (no specific commit, design observation). `visibilitychange` fires as soon as `window.open` is called. A naive `pendingRequest = createDefer()` immediately before the intent open would resolve with stale clipboard content. The workaround is a 500ms setTimeout before installing the pending request.

**Lesson:** Any external-app handoff signer needs a "guard window" between handoff and reading the response. NMP's iOS scheme-URL signer should not assume the first `applicationDidBecomeActive` after handoff is the response.

## G9. ActionRunner cached context across account swaps

**Commit `96120548`** ("Fix ActionRunner cache causing actions to have wrong `user` when accounts change"). `ActionRunner.getContext()` memoized the context (`action-runner.ts:54-68` — `if (this._context) return this._context;`). When the user swapped accounts, actions ran with the previous account's `user`/`self`. Fix (in the broader commit family for cast cleanup): the global `User.cache` (`casts/user.ts:17`) needed beforeEach clearing in tests; in production, the ActionRunner is short-lived per account.

**Lesson:** Per-account context caching is fine; cross-account caching is a bug. NMP's `ActionRunner` analogue must rebuild context (or invalidate it) on every active-account change. Make this a contract: subscribe to `AccountManager.active$`, drop the cache on every emission.

## G10. EventFactory symbol-cache leak across pipeline steps

**Commit `c9c0aba5`** ("strip stale symbol caches in EventFactory.chain() and replace removed buildEvent usages"). `EventFactory.chain()` carried symbol caches between operations. If step 1 set `HiddenTagsSymbol` and step 2 added new tags, step 2 would mutate the underlying tags but step 3 would see the stale cache. Fix: strip `PRESERVE_EVENT_SYMBOLS` after each step.

**Lesson:** NMP's event-builder pipeline should either operate on immutable inputs or have a per-step invalidation. The "cache on symbols" approach is fast but requires discipline.

## G11. SQLite store: duplicate event tags throw

**Commit `11ad823c`** ("Fix duplicate event tags throwing errors in sqlite"). Tag uniqueness constraint was applied to `(event_id, tag_name, tag_value)` — but some legitimate events have duplicate tags (e.g., multiple p-tags with the same pubkey). Fix: drop the constraint, dedupe at query time.

**Lesson:** NMP's persistent store schema (spec §7.1, LMDB/IndexedDB) must allow duplicate tag rows. Test with real-world data — gift wraps and complex event kinds genuinely repeat tags.

## G12. parseReplaceableAddress truncated at first colon

**Commit `d822aa3b`** ("Fix parseReplaceableAddress to handle URLs with colons"). The address format is `kind:pubkey:identifier` but identifiers can contain colons (e.g., URLs). Naive `split(":")` truncated. Fix: `kind, pubkey, ...rest = split(":"); identifier = rest.join(":")`.

**Lesson:** NMP must treat the parameterized-replaceable identifier as the **suffix after the second colon**, not the third field. Add a test with an identifier containing colons.

## G13. EventStore symbol set too late

**Commit `c61616e3`** ("Fix bug with event store not setting symbol early enough"). The `EventStoreSymbol` was being set after `insert$` emitted. Consumers that immediately tried to `getParentEventStore(event)` got undefined. Fix: set the symbol before emitting (now at `event-store.ts:273-277`).

**Lesson:** Symbol attachment, claim, and emission must happen in this order: attach → emit → consumer can act. Any reorder breaks consumers.

## G14. RelayPool.req/event didn't complete on socket close

**Commit `970fb035`** ("Fix .req and .event not completing on socket close Add reconnect option"). REQs and EVENT publishes hung when the underlying websocket closed. Consumers had no way to know "give up." Added explicit `complete()` on close and a `reconnect` option to control retry behaviour.

**Lesson:** Every long-lived NMP subscription/publish must surface a discrete "channel closed" terminal — never an indefinite pending state.

## G15. Default retries too high (10 → 3)

**Commit `1ff4283f`** + the changeset in `c2bd9a2a`. Default retry count was 10, leading to long blocking on dead relays. Lowered to 3.

**Lesson:** Defaults matter. NMP's default retry should be small (≤3) with explicit override for "really want to wait."

## G16. NIP-46 signers didn't reconnect

**Commit `e6d5613b`** ("Fix nostr connect signers not reconnecting to relays"). The NIP-46 subscription used a one-shot RxJS subscribe; on disconnect it died. Fix in `nostr-connect-signer.ts:152-160`: `.pipe(repeat(), retry(), filter(...))`. Without it, a bunker disconnect silently breaks signing for the lifetime of the session.

**Lesson:** Long-lived signer subscriptions must be self-healing. Don't rely on the underlying pool's reconnection — wrap with `retry/repeat`.

## G17. NIP-44 default fallback was wrong

**Commit `360e4cd0`** ("Fix incorrectly defaulting to nip-44 encryption when missing 'encryption' tag"). When the encryption-method tag was missing, the code defaulted to NIP-44, but the real semantic default per the spec varies by message type. Fix: explicit selection per kind, not a global default.

**Lesson:** Don't choose encryption schemes by global default. The choice is per-kind, per-NIP, sometimes per-recipient capability. Make the choice explicit at every call site.

## G18. ExtensionSigner pubkey not cached

**Commit `0867a502`** ("Cache the pubkey on ExtensionSigner"). Every getPublicKey() round-tripped to the extension. For an action that called `getPublicKey()` three times, three prompts. Fix: cache after first success.

**Lesson:** Pubkey is immutable per signer instance. Cache aggressively. Invalidate only on explicit lifecycle (e.g., extension disconnected).

## G19. SignerMismatchError post-conditions

**Reading `accounts/src/account.ts:33-128`** + commit `872115f8` ("Verify ExtensionSigner and NostrConnectSigner return hex pubkey"). The account layer enforces three post-conditions:
- `getPublicKey()` result matches account's stored pubkey.
- `signEvent()` result's `pubkey` matches.
- `signEvent()` result's `id` matches the precomputed hash (i.e., the signer didn't modify the template before signing).

**Lesson:** These are cheap defensive checks against compromised/buggy signers. NMP must implement them. They catch a real attack vector: a malicious extension that signs a different event than the one passed in.

## G20. URL scheme inference: ws ↔ wss, http ↔ https

**Commit `39975fdd`** ("Fix `ensureWebSocketURL` converting `ws:` URLs to `wss:` Fix `ensureHttpURL` converting `http:` URLs into `https:`"). Helper functions were silently upgrading ws:// to wss:// (and http to https), which broke local-development relays.

**Lesson:** Helpers must not silently mutate URL schemes. Local-relay development is a first-class workflow. NMP's URL normalization should default to permissive and offer an explicit "force-secure" mode.

## G21. Event store getByFilters return-type churn

**Commit `1a7a4e1e`** ("Change `EventStore.getByFilters` to return `NostrEvent[]` instead of `Set<NostrEvent>`"). The Set return type meant downstream consumers were doing `Array.from()` everywhere; the API was inconsistent with `getTimeline()`. Breaking change.

**Lesson:** Return types ripple. Pick `Vec<&Event>` (or `impl Iterator`) early in NMP and don't change it. Document the iteration order contract (sorted desc by created_at, by `getTimeline`'s contract).

## G22. Loaders returning duplicates by default

**Commit `871c929d`** ("loaders should not return duplicate events by default"). Default deduplication via `filterDuplicateEvents(EventMemory())` (see `address-loader.ts:182-184`). Without it, multi-relay loaders surfaced N copies to the consumer.

**Lesson:** NMP's loader API must dedupe by default. Opt-out for raw provenance is fine, but the default is "one logical event per pointer."

---

## Pattern summary

The clusters of bugs above sort into five categories:

1. **NIP-01 / replaceable invariants** (G1, G12, G21) — get the spec exactly right or pay forever.
2. **Lifecycle observables and the operations that should emit them** (G5, G13, G14) — colocate emit and mutate.
3. **Caches without invalidation hooks** (G6, G10, G9, G11) — every cache needs an invalidation contract.
4. **Concurrency, races, leaks** (G2, G7, G8, G16) — refcount when shared, serialize when external.
5. **Default behaviours that silently fail** (G3, G15, G17, G18, G20, G22) — defaults must fail loud, not silent.

NMP's M2 design review should explicitly check each new component against these five categories.
