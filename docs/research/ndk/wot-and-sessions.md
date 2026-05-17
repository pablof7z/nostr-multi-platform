# WoT and Sessions

Two related-but-distinct subsystems: `@nostr-dev-kit/sessions` manages identity + the active user's metadata; `@nostr-dev-kit/wot` builds a follow graph for spam filtering and ranking.

## Sessions

Package: `sessions/src/`. Lines: ~1500 across `manager.ts` (478), `store.ts` (643), `auth-manager.ts` (51), `persistence-manager.ts` (112), `types.ts` (160).

### Architecture

Three classes by SRP:
- `NDKSessionManager` (public API) — composes auth + persistence + store.
- `AuthManager` — login/logout/switch orchestration.
- `PersistenceManager` — serialize/restore from `SessionStorage` adapter.

Backed by a zustand `vanilla` store (`store.ts:createSessionStore`). Multi-account: `sessions: Map<Hexpubkey, NDKSession>`, `signers: Map<Hexpubkey, NDKSigner>`, `activePubkey?: Hexpubkey`.

### Per-session data

```
sessions/src/types.ts:7-51
interface NDKSession {
    pubkey: Hexpubkey;
    followSet?: Set<Hexpubkey>;
    muteSet?: Map<string, string>;   // id → "p"|"e"
    mutedWords?: Set<string>;
    blockedRelays?: Set<string>;
    relayList?: Map<string, { read: boolean; write: boolean }>;
    events: Map<NDKKind, NDKEvent | null>;
    subscriptions: NDKSubscription[];
    lastActive: number;
    preferences?: SessionPreferences;
}
```

`events` holds the latest replaceable event per kind (with `from`-wrapping if a `MonitorItem` constructor is registered). Always wrapped: kind 3 (Contacts), 10000 (MuteList), 10001 (BlockRelayList), 10002 (RelayList), and any monitored kind.

### Start options

```
sessions/src/types.ts:69-119
interface SessionStartOptions {
    follows?: boolean;          // kind 3
    mutes?: boolean;            // kind 10000
    blockedRelays?: boolean;    // kind 10001
    relayList?: boolean;        // kind 10002
    wallet?: boolean;           // 17375, 10019
    monitor?: MonitorItem[];    // arbitrary kinds + classes
    setMuteFilter?: boolean;       // default true
    setRelayConnectionFilter?: boolean; // default true
}
```

`monitor` accepts either raw `NDKKind` numbers or event constructors with `static kinds: NDKKind[]` and `static from(event)`. Constructors auto-wrap incoming events.

### Side effects of switchToUser

```
sessions/src/store.ts:254-329
```

On switch:
1. Set `ndk.signer = signer` (synchronously, before activePubkey flips — race fix `a14c7a78`).
2. Set `ndk.muteFilter` based on session's `muteSet` + `mutedWords` (single filter that checks pubkey, event id, and lowercase-substring word match).
3. Set `ndk.relayConnectionFilter` from `blockedRelays`.
4. `set({ activePubkey: pubkey })` → reactive consumers fire.
5. Async resolve `user`, set `ndk.activeUser`.

On `removeSession` of the active pubkey: clear NDK state BEFORE `set()` (race fix `7d25cd76`), then stop subscriptions, then attempt switch to next session.

### Persistence

`SerializedSession` (`types.ts:135-140`):
```
{ pubkey, signerPayload?, lastActive, preferences? }
```

Only identity is persisted; follows/mutes/etc come back from NDK cache on restore. Storage adapters in `sessions/src/storage/`:
- `MemoryStorage` (test)
- `LocalStorageStorage` (browser)
- `FileStorageStorage` (Node)
- `mobile`'s `NDKSessionExpoSecureStore` (`mobile/src/session-storage-adapter.ts`).

Auto-save: enabled by default (`autoSave: true`), debounced via `saveDebounceMs` (default 500ms) — `manager.ts:setupAutoSave`.

### Subscription topology

Per active session there's **one** subscription with `subId: "session"` and `closeOnEose: false`. `addMonitor` adds additional subscriptions if the caller wants to monitor more kinds post-login.

## WoT

Package: `wot/src/wot.ts` (~330 LOC).

### Build flow

```ts
const wot = new NDKWoT(ndk, rootPubkey);
await wot.load({ depth: 2, maxFollows: 1000, useNegentropy: true, negentropyMinAuthors: 5 });
```

Algorithm (`wot.ts:59-136`):

1. Start with `nodes = {root: depth 0}`.
2. For each `currentDepth` from 0 to `depth-1`:
   a. Collect pubkeys at this depth.
   b. Fetch their kind:3 contact lists via `fetchContactLists()`.
   c. For each event, extract `p` tags, slice to `maxFollows`, add nodes at `currentDepth + 1` (or update existing if shorter path found).
3. Honors `timeout` per-iteration check.

### Fetch strategies

`fetchContactLists` (`wot.ts:141-187`):
- If `useNegentropy && authors.length >= negentropyMinAuthors`: `NDKSync.sync(ndk, { kinds: [3], authors }, { autoFetch, subId: "wot-sync", relayUrls? })`.
- Otherwise (or on negentropy failure): `fetchViaSubscription(authors)` — `ndk.subscribe({ kinds: [3], authors }, { closeOnEose: true, addSinceFromCache: true })` with 30s timeout.

Negentropy lets WoT efficiently sync large author sets — for `depth: 2` on a heavy account this can be tens of thousands of kind:3s.

### Scoring

```ts
getScore(pubkey): number     // 1/(depth+1) — depth 0 = 1.0, depth 1 = 0.5
getDistance(pubkey): number | null
includes(pubkey, {maxDepth?}): boolean
getAllPubkeys({maxDepth?}): string[]
getScores(pubkeys[]): Map<string, number>
```

No edge weighting; pure BFS distance scoring. Multi-source path-shortening is in (`wot.ts:124-129`).

### Svelte / store integration

`svelte/src/lib/builders/wot.svelte.ts` + `svelte/src/lib/stores/wot.svelte.ts` integrate WoT post-filtering and WoT-ranked sorting into reactive subscriptions just by passing `wot: {...}` or `wotRank: ...`.

### Costs

- WoT graph is in-memory only. No persistence to cache adapter. Lost on app restart unless caller serializes.
- Depth 2 on a 500-follow account = 500 × ~500 = 250k pubkeys (bounded by `maxFollows`).
- Negentropy reduces network bytes dramatically vs raw REQ but still has reconciliation CPU cost.

## Implications for NMP

- Session manager logic is straightforward to port — pure state + one long-lived subscription per active session.
- WoT-as-spam-filter is a big lever for mobile UX (mute is reactive, WoT is proactive).
- WoT build is expensive; defer to background, persist graph to local DB.
- **The session "one long-lived sub per active user, closeOnEose: false, kinds: [3, 10000, 10001, 10002]" pattern IS the framework-magic core for kind:3 auto-tracking.** NMP's session module owns this subscription; UI components never see it; the kernel publishes "follows changed" deltas to any view that declared interest in "current account's follows".
