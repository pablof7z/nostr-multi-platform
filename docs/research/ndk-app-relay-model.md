# NDK — app/default/indexer relay handling

> Source repo: https://github.com/nostr-dev-kit/ndk (master branch).
> All line numbers are against `master` HEAD at time of research (2026-05-18).
> Verbatim quotes are marked. Items marked "(paraphrased)" are from WebFetch
> summaries and were not retrieved character-for-character.

## Summary

NDK exposes **three relay-config inputs** (`explicitRelayUrls`, `outboxRelayUrls`,
`devWriteRelayUrls`) plus a **fourth implicit source** (signer-supplied
NIP-65 list via `autoConnectUserRelays`). None of these map cleanly onto
NMP's five-role model:

- NDK has **no concept of an "indexer relay"** — `outboxRelayUrls` is *only* a
  metadata-discovery pool used to fetch kind:3 and kind:10002 for the
  `OutboxTracker`. Despite its name, `outboxRelayUrls` is **not** a routing
  destination for kind:0 / kind:3 / kind:1xxxx data.
- Routing is **kind-agnostic**: profiles, follow lists, and replaceable lists
  go through the same outbox calculation as kind:1 notes. There is no
  special-case for universal-data kinds.
- The cold-start fallback is implicit: when an author has no outbox data, the
  subscription falls back to "permanent and connected relays" (the top-5 of
  the main `ndk.pool`), i.e. the `explicitRelayUrls`.
- **No NIP-51 specialized routing** for kind:10007 (search), kind:10050 (DM
  inbox), or kind:10013 (drafts). NDK *knows the kind numbers* as enum entries
  but does not route by them. Kind:10013 isn't even in the enum.
- Known footguns: relay list TTL is 2 minutes, zaps stuff every connected
  relay URL into a header causing HTTP 431, blacklist not respected once
  outbox enabled.

## 1. Concepts and naming

| NDK type / field | Purpose | File |
|---|---|---|
| `NDKPool` | A bag of `NDKRelay`s with subscribe/publish. Two instances live on NDK: `pool` (main) and `outboxPool` (metadata discovery). | `core/src/relay/pool/index.ts` |
| `NDKRelay` | One websocket connection. | `core/src/relay/index.ts` |
| `NDKRelaySet` | An immutable subset of `NDKPool` used for a single sub/publish. | `core/src/relay/sets/` |
| `NDKRelayList` | The kind:10002 NIP-65 event wrapper (read/write URL split). | `core/src/events/kinds/relay-list.ts` |
| `OutboxTracker` | LRU keyed by `Hexpubkey` → `{readRelays, writeRelays}` derived from kind:10002 (with kind:3 fallback). | `core/src/outbox/tracker.ts` |
| `OutboxItem` | The tracker's value type. Holds `readRelays`, `writeRelays`, `relayUrlScores`. | `core/src/outbox/tracker.ts:19-39` |
| `temporaryRelayTimers` | Pool-internal map; a relay is "permanent" iff it is **absent** from this map. | `core/src/relay/pool/index.ts` |
| `DEFAULT_OUTBOX_RELAYS` | The bootstrap fallback for `outboxPool` when no `outboxRelayUrls` are configured. | `core/src/ndk/index.ts:82` |

Verbatim, from `core/src/ndk/index.ts` line 82:

```ts
export const DEFAULT_OUTBOX_RELAYS = ["wss://purplepag.es/", "wss://nos.lol/"];
```

Verbatim, from `core/src/outbox/tracker.ts` lines 60-66:

```ts
this.data = new LRUCache({
    maxSize: 100000,
    entryExpirationTimeInMS: 2 * 60 * 1000,
});
```

That 2-minute TTL on kind:10002 results is a footgun — see §6.

## 2. Configuration surface

Verbatim, from `core/src/ndk/index.ts` lines 84-211 (excerpted to the relevant fields):

```ts
export interface NDKConstructorParams {
    /**
     * Relays we should explicitly connect to
     */
    explicitRelayUrls?: string[];

    /**
     * When this is set, we always write only to this relays.
     */
    devWriteRelayUrls?: string[];

    /**
     * Outbox relay URLs.
     */
    outboxRelayUrls?: string[];

    /**
     * Enable outbox model (defaults to true)
     */
    enableOutboxModel?: boolean;

    /**
     * Auto-connect to main user's relays. The "main" user is determined
     * by the presence of a signer. Upon connection to the explicit relays,
     * the user's relays will be fetched and connected to if this is set to true.
     * @default true
     */
    autoConnectUserRelays?: boolean;

    /**
     * Custom filter function to determine if a relay connection should be allowed.
     */
    relayConnectionFilter?: (relayUrl: string) => boolean;
    ...
}
```

Verbatim, from `core/src/ndk/index.ts` constructor lines 327-398 (relay portion only):

```ts
public constructor(opts: NDKConstructorParams = {}) {
    super();
    ...
    this._explicitRelayUrls = opts.explicitRelayUrls || [];
    this.subManager = new NDKSubscriptionManager();
    this.pool = new NDKPool(opts.explicitRelayUrls || [], this);
    this.pool.name = "Main";
    ...
    this.autoConnectUserRelays = opts.autoConnectUserRelays ?? true;
    ...
    if (!(opts.enableOutboxModel === false)) {
        this.outboxPool = new NDKPool(opts.outboxRelayUrls || DEFAULT_OUTBOX_RELAYS, this, {
            debug: this.debug.extend("outbox-pool"),
            name: "Outbox Pool",
        });

        this.outboxTracker = new OutboxTracker(this);
        ...
    }
    ...
    if (opts.devWriteRelayUrls) {
        this.devWriteRelaySet = NDKRelaySet.fromRelayUrls(opts.devWriteRelayUrls, this);
    }
    ...
}
```

Three things to note:

1. **Outbox is on by default** (`!(enableOutboxModel === false)`).
2. `outboxRelayUrls` falls back to `DEFAULT_OUTBOX_RELAYS` (`purplepag.es`, `nos.lol`) — i.e. an opinionated, app-developer-invisible default.
3. `explicitRelayUrls` is what feeds the **main** pool — it acts as both "bootstrap" and "fallback when outbox tracker has no data".

Typical app-dev call site (from search results, [npm 2.3.3 docs](https://www.npmjs.com/package/@nostr-dev-kit/ndk/v/2.3.3)):

```ts
const ndk = new NDK({
  explicitRelayUrls: ['wss://relay.damus.io', 'wss://nos.lol'],
  outboxRelayUrls: ['wss://purplepag.es'],
  enableOutboxModel: true,
});
```

## 3. Routing decisions by kind

**TL;DR: NDK does not special-case any kind for routing.**

`calculateRelaySetFromEvent` in `core/src/relay/sets/calculate.ts` (paraphrased
from WebFetch summary — not verbatim) applies the same rules to every event
regardless of kind:

1. Add the author's NIP-65 write relays (from `OutboxTracker`).
2. Add up to 5 relay hints from `"a"` / `"e"` tags.
3. For `"p"` tags (≤5), call `chooseRelayCombinationForPubkeys`.
4. Add `ndk.pool?.permanentAndConnectedRelays()` (everything currently sticky).
5. Top up from `ndk.explicitRelayUrls` if still short.

There are no `if (event.kind === 0)` / `if (kind === 3)` / `kind in [10000..19999]` branches in this function.

For subscriptions, `calculateRelaySetsFromFilter` in the same file is also
kind-agnostic (paraphrased): it groups authors by their outbox relays, then
fans out filters per relay; if a filter has no authors, it goes to *all*
identified relays; if no relays emerge, it falls back to up to 5
`permanentAndConnectedRelays`.

Verbatim, the permanent-vs-connected predicate from `core/src/relay/pool/index.ts` lines 443-447:

```ts
public permanentAndConnectedRelays(): NDKRelay[] {
    return Array.from(this.relays.values()).filter(
        (relay) => relay.status >= NDKRelayStatus.CONNECTED && !this.temporaryRelayTimers.has(relay.url),
    );
}
```

A relay is "permanent" iff it lacks a temporary-timer entry; there's no
semantic flag like `isIndexer` or `isAlwaysOn`.

NDK *does* recognize NIP-51 list kinds as enum entries (from
`core/src/events/kinds/index.ts`):

```
MuteList = 10000
PinList = 10001
RelayList = 10002
BookmarkList = 10003
...
SearchRelayList = 10007
SimpleGroupList = 10009
RelayFeedList = 10012
InterestList = 10015
...
DirectMessageReceiveRelayList = 10050
BlossomList = 10063
```

But these are **labels**, not routes. Notably absent: kind:10013 (drafts
relays) and kind:10102 (good-wiki relays). `core/src/events/kinds/drafts.ts`
is a NIP-37 event wrapper with no relay-routing logic; its only relay
parameter is a caller-supplied `relaySet` on `publishReplaceable(relaySet)`.

## 4. Cold-start / no-NIP-65 fallback path

When a user has no kind:10002, the path is:

1. **`new NDK({...})`** instantiates two pools:
   - `pool` (main) seeded with `explicitRelayUrls`.
   - `outboxPool` seeded with `outboxRelayUrls || DEFAULT_OUTBOX_RELAYS`.
2. **`ndk.connect()`** opens connections to both pools.
3. If `autoConnectUserRelays === true` (default) **and** the signer
   implements `relays()`, that list is added to `pool` too — a **third
   implicit relay source** the app dev never specified at construction time.
4. On any subscription that names this user as an author, NDK calls
   `OutboxTracker.trackUsers(...)`. Inside, `getRelayListForUsers` issues
   `{kinds: [3, 10002], authors: [...]}` against `ndk.outboxPool || ndk.pool`
   (verified, `core/src/utils/get-users-relay-list.ts` line ~37).
5. If kind:10002 is returned, `OutboxItem.{readRelays,writeRelays}` is filled.
6. If only kind:3 is returned, NDK derives a relay list from the legacy
   contact-list JSON content (`relayListFromKind3`).
7. If **neither** is returned, the author goes into `authorsMissingRelays`.
   In `chooseRelayCombinationForPubkeys` (`core/src/outbox/index.ts:118-125`):

   ```ts
   for (const author of authorsMissingRelays) {
       pool.permanentAndConnectedRelays().forEach((relay: NDKRelay) => {
           const authorsInRelay = relayToAuthorsMap.get(relay.url) || [];
           authorsInRelay.push(author);
           relayToAuthorsMap.set(relay.url, authorsInRelay);
       });
   }
   ```

   Every connected permanent relay (i.e. `explicitRelayUrls` + auto-connected
   signer relays) gets that author's filter. No de-duplication for
   universal-data kinds; if you have 8 explicit relays, kind:0 fetches fan
   out to all 8.

So NDK's "no NIP-65" answer is essentially: **explicit relays catch
everything**, on the implicit assumption that the app dev has put a
reasonable bootstrap list there.

## 5. Outbox model interaction

The three relay sources interact this way:

| Source | Lives on | Used for |
|---|---|---|
| `explicitRelayUrls` | `ndk.pool` (main) | Default subscription target; cold-start fallback; included in every "permanent" set; injected into publishes as a last-resort top-up. |
| `outboxRelayUrls` (defaults to `purplepag.es`, `nos.lol`) | `ndk.outboxPool` | **Only** queried by `getRelayListForUsers` to fetch kind:3 / kind:10002 for the tracker. Never used as a routing destination for any other kind. |
| Signer's `relays()` | `ndk.pool` (main, via `addRelay` in `connect()`) | Same role as `explicitRelayUrls` — augments the main pool. |
| `devWriteRelayUrls` | `ndk.devWriteRelaySet` | If set, all publishes go here exclusively (a dev-time override). |

**Relationship to outbox tracker:**

- The tracker fills `readRelays` / `writeRelays` strictly from kind:10002 (or kind:3 fallback).
- For subscriptions: outbox results are **substitutive** for authors that have a NIP-65 list (NDK queries the author's own write relays, NOT the explicit relays), and **the explicit relays become additive only as a fallback** when an author lacks data.
- For publishes (`calculateRelaySetFromEvent`): outbox is **additive** — author's write relays *plus* permanent/connected relays *plus* a top-up from explicit URLs. This is why issue #175 happens.

The constructor wires a refresh hook (`core/src/ndk/index.ts` lines 354-369, verbatim):

```ts
this.outboxTracker.on("user:relay-list-updated", (pubkey, _outboxItem) => {
    this.debug(`Outbox relay list updated for ${pubkey}`);

    // Find all active subscriptions that include this author
    for (const subscription of this.subManager.subscriptions.values()) {
        const isRelevant = subscription.filters.some((filter) => filter.authors?.includes(pubkey));

        if (isRelevant && typeof subscription.refreshRelayConnections === "function") {
            this.debug(`Refreshing relay connections for subscription ${subscription.internalId}`);
            subscription.refreshRelayConnections();
        }
    }
});
```

— meaning a fresh kind:10002 mid-subscription causes a re-fan-out. Combined
with the 2-minute LRU TTL, an active feed for N authors can re-issue
`{kinds:[3,10002], authors:[…]}` against the outbox pool every ~2 minutes.

## 6. Known failure modes / open issues

- **[#175 — "Failed to zap an event if outbox is enabled"](https://github.com/nostr-dev-kit/ndk/issues/175).** Verbatim from the issue: zap request creation does `const relays = Array.from(this.ndk.pool.relays.keys());` — and with outbox enabled, that set becomes large enough to trigger HTTP 431 (Request Header Fields Too Large). Root cause: NDK indiscriminately serializes every connected relay URL into the zap request header, with no per-purpose filtering.
- **[#149 — "Blacklist relay will be connected if outbox enabled"](https://github.com/nostr-dev-kit/ndk/issues/149)** (closed). Outbox tracker pulls a kind:10002 that names a relay on the blacklist and connects to it anyway. The post-fix is the `relayConnectionFilter` callback (visible in current constructor, line 376) — but it requires the app dev to set it. Default behavior is to honor whatever the network says.
- **[#268 — "can't publish event on 2.10.0"](https://github.com/nostr-dev-kit/ndk/issues/268).** Publish regression around the outbox / relay-set boundary.
- **[#145 — "Outbox: NOTICE: ERROR: bad req: subscription id too long"](https://github.com/nostr-dev-kit/ndk/issues/145).** Outbox-related sub-id overflow.
- **[#141 — "error on login: TypeError: relays[url] is undefined"](https://github.com/nostr-dev-kit/ndk/issues/141).** Cold-start crash when the signer or relay map is mid-flight.
- **[#387 (PR, in-progress) — "outbox: add Thompson+CG3 (+4.9pp recall), NIP-66 filtering (~45% faster), connection cap"](https://github.com/nostr-dev-kit/ndk/pull/387)** and **[#385 — "add nip-66 relay liveness check to outbox"](https://github.com/nostr-dev-kit/ndk/pull/385)**. Active iteration on outbox quality. The "connection cap" hint here strongly implies #175-class problems persist.
- **The 2-minute LRU TTL** (`tracker.ts:65`) is itself a latent footgun. For long-running feeds, every author's kind:10002 expires every 2 minutes, prompting re-fetch from the outbox pool. There's no signed-event-creation-time check; it expires on wall-clock entry age.
- **Three additive relay sources at cold-start** (`explicitRelayUrls` + signer's `relays()` + `DEFAULT_OUTBOX_RELAYS`) with no operator visibility. An app dev who writes `explicitRelayUrls: ['wss://a']` may end up connected to `wss://a`, `wss://purplepag.es/`, `wss://nos.lol/`, plus whatever the signer returned — without ever opting in to the latter three.

## 7. What NMP can learn — concrete recommendations

Mapped to NMP's five-role model:

1. **NIP-65 outbox (per-author write).** NDK conflates "author write relays" with "publishes also fan out to everything else". NMP's outbox routing should be **substitutive, not additive** for kind:1-class content. Don't union explicit/indexer relays into the publish set for events where the author's outbox is authoritative — that's exactly what causes NDK #175 (HTTP 431 on zaps).

2. **NIP-65 inbox (per-author read).** NDK's tracker pulls inbox + outbox from one event, fine. But NDK has **no notion of "I am being mentioned" inbox subscription** that re-uses the inbox half distinct from outbox. NMP should keep `readRelays` / `writeRelays` semantically distinct in subscription dispatch — NDK has the data fields but never seems to consult only one side for a directional role.

3. **Indexer relays (always-on for kind:0/3/10000-19999).** **NDK has no equivalent.** The misleadingly named `outboxRelayUrls` is *not* an indexer — it's a metadata-discovery pool whose only consumer is `getRelayListForUsers`. NMP should:
   - Codify "indexer" as a routing role that is consulted **for specific kinds** regardless of outbox state, not as a separate pool with a different consumer.
   - Treat the role as both read **and** write, unlike NDK's `outboxPool` which is effectively read-only.
   - Default-list-shipped indexers should be visible config, not invisible like NDK's `DEFAULT_OUTBOX_RELAYS = ["wss://purplepag.es/", "wss://nos.lol/"]` hard-coded constant.

4. **App relays (operator config, user-mutable fallback).** NDK's `explicitRelayUrls` is doing four jobs at once: bootstrap target, cold-start fallback, publish top-up, kind-0 broadcast destination. NMP should split:
   - "Bootstrap" (one-shot, to fetch kind:10002 then drop) — distinct from
   - "Fallback" (used only when author has no NIP-65) — distinct from
   - "Always-on for universal data" (= indexer, NMP role 3).
   NDK's failure mode is that disabling outbox or changing `explicitRelayUrls` mid-session has surprising effects across all four jobs.

5. **NIP-51 specialized routing (kind:10007 search, kind:10013 drafts, kind:10102 wiki).** NDK has **zero**. It knows `SearchRelayList = 10007` as an enum entry but no code consults a user's published 10007 to route search queries. `drafts.ts` does not reference 10013 at all (NDK's draft kind:31234 just takes a caller-supplied relay set). NMP should make this routing first-class: the kernel should consult the user's 10007 before issuing search filters, the user's 10013 before publishing drafts (with the 10013-implies-nip44 encryption invariant), and the user's 10102 for wiki queries. Don't repeat NDK's pattern of "we have the type but no router".

**Cross-cutting recommendations from NDK pain points:**

- **Per-purpose relay sets, not one giant union.** NDK's `pool.relays.keys()` zap bug (#175) is a direct consequence of treating "all connected relays" as one set. NMP's dispatch should know *why* each relay is connected (outbox / inbox / indexer / app / specialized) and use only the relevant subset for any given operation.
- **TTL on NIP-65 cache should be much longer than 2 minutes,** or driven by event-creation timestamp comparison rather than wall-clock entry age. NDK's `tracker.ts:65` expires authors every 2 minutes regardless of whether the kind:10002 changed.
- **Operator-visible relay sources.** NDK has three implicit sources (explicit, signer, default-outbox) that the app dev didn't ask for. NMP should require explicit operator declaration of each role's relay set and surface every source in introspection.
- **Honor blacklist universally.** NDK #149 showed that outbox subverts blacklist by default. NMP's `relayConnectionFilter` equivalent must be enforced at the routing layer, not optional.
