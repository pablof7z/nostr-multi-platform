# NDK Gotchas (git-archaeology)

Selected fixes from `git -C NDK-nhlteu log --grep="race|leak|deadlock|regression|fix"` (1,253 total commits, filtered to the load-bearing ones).

Format: `commit | one-line summary | implication`.

## Race conditions (sessions/signer)

`a14c7a78` — **fix(sessions): prevent race condition between activePubkey and signer**
Old code set `activePubkey` via store update *before* `ndk.signer = signer`. Reactive consumers ran with no-signer or stale-signer. Fix: set NDK signer synchronously first; svelte wrapper does same before its reactive activePubkey write. Touches `sessions/src/store.ts` and `svelte/src/lib/stores/sessions.svelte.ts`. **Implication for NMP**: when implementing session switching, ALWAYS bind the signer before emitting a "user changed" signal to UI.

`7d25cd76` — **fix: properly clear $currentUser and $currentPubkey on logout**
`ndk.signer` setter has an async side effect that queues a promise resolving `activeUser`. Logout cleared store, but a stale pending promise re-set `activeUser` afterward. Fix: clear `ndk.signer`/`activeUser`/`activePubkey` directly *before* `set()` triggers subscriptions, so subscription handlers don't observe stale `activePubkey` and re-set signer. **Implication**: any signer-setter that triggers async work needs cancel-on-stale logic.

`e5df84eb` — **fix(sessions): correctly remove sessions on logout instead of cycling**
`removeSession` was calling `switchToUser` and returning early, so `set(updates)` never ran — session stayed in store. Fix: apply removal first, *then* switch. Multiple sequential logouts now work. **Implication**: state-update ordering matters even more than usual when zustand selectors are driving subscriptions.

`a912a2c2` — **fix: resolve race condition in relay list fetching**
`getRelayListForUsers` timeout fired before EOSE arrived from relays → empty relay lists → subscriptions never connecting to user relays. Fix: added `resolved` flag (prevent double-resolve), conditional timeout that extends +3s when relays are still connecting. **Implication**: outbox bootstrap is timing-sensitive; consider exposing a "wait until relay lists known" gate in NMP UX before showing follow feed.

`d701898c` — **Fix TDZ race condition in fetchEvent timeout handler**
Timeout callback could fire before subscription variable was assigned. Malformed bech32 entities triggered the 10s timeout reliably. Fix: `s?.stop()`. **Implication**: NMP entity parsers must reject malformed npub/nevent/etc. *before* hitting fetchEvent.

`ad7936b6` — **fix(relay): prevent empty REQ when subscriptions close before execution**
Grouped subscriptions (default 10ms delay) that closed after fast cache hits but before scheduled REQ would still send empty REQs. Fix: `removeItem()` cancels timer when all members removed from INITIAL/PENDING.

## Memory leaks

`e5901c98` — **fix: prevent memory leak in seenEvents with LRU cache**
`seenEvents` was an unbounded `Map`. Replaced with LRU `{ maxSize: 10000, expirationTimeInMS: 5 * 60 * 1000 }` in `subscription/manager.ts:14-19`. **Implication**: long-lived NMP sessions don't pay unbounded RAM for event-relay tracking, but the 5-min TTL means a re-arriving event after that window will look "new" again.

`d788dc24` — **fix(svelte): prevent Avatar component from creating duplicate subscriptions**
Avatar didn't tear down its profile subscription on user-change; piled up duplicates. **Implication**: any per-author UI cell (avatar, profile name) needs explicit teardown on prop change.

## Subscription / relay correctness

`8b9a37cb` (PR #375) — **fix(relay): don't skip events in seenEvents for new subscriptions**
Without a cache: sub A saw event X; sub B opened later requesting X; relay sent X; was dropped as "already seen" globally. Fix: removed global early-return; per-sub `eventFirstSeen` is the dedup. **Implication**: cache is no longer required for sub-overlap correctness.

`33e75950` — **fix(relay): fix reconnection after sleep/wake and stale connections**
`handleStaleConnection()` set DISCONNECTED *before* calling `onDisconnect()`, so the reconnect check (`if status === CONNECTED`) failed. Also removed MAX_RECONNECT_ATTEMPTS=5 limit; relays now retry forever with capped 30s backoff. **Implication for mobile**: sleep/wake cycles are common on iOS/Android; this fix is essential for background-resume scenarios.

`5afbd245` — **fix(cache): skip cache queries for subscriptions with only ephemeral kinds**
Ephemeral 20000-29999 are never cached; querying was a waste. Adds `filterForCache` utilities for adapters.

`3215e51e` — **perf: remove seenEvent call for cached events**
~0.24-0.64ms per call × 5700 events = 1.4s. Fix: skip `seenEvent` for cache-sourced events (only used for relay-source dedup). **Implication**: bulk cache replay is now fast enough that NMP can show full history on launch.

`cfee9ea7` / `267ee52b` — **perf: optimize cache performance and fix cache write-back bug**
Combined: cache write-back was incorrectly skipping events under some conditions.

`ac9a972e` — **perf(core): skip JSON.parse for duplicate events**
Hash-first dedup before parse.

## NIP-46 (remote signer)

`d08415f6` (Feb 2026) — **fix(nip46): initialize RPC subscription in fromPayload()**
Restored bunker signer hung indefinitely on first `sign()` because no subscription was open to receive responses. **Implication**: persisting signers must always test restored flow end-to-end.

`6e296027` / `01ba3c8f` — **fix(nip46): use getPublicKey() in nostrconnect flow to get actual user pubkey**
nostrconnect connection token's pubkey was being used as user pubkey; corrected to fetch via RPC.

`e7b29d84` — **fix(nip46): address code review findings for timeout implementation**
NIP-46 `timeout` field follow-up.

`deb7f93d` — **fix(nip46): fix async error handling in backend event handler**

## Sync / wallet

`3407126e` — **fix(sync): add fallback to fetchEvents for relays without negentropy support**
Async callback race where cache updates weren't awaited; DRY refactor; auto-fallback per relay; events cached even on fallback path.

`7287bf5e` — **fix(wallet): use NDKSync class for proper relay capability caching**
Old `ndkSync` function didn't check capability cache; wallet kept retrying relays that didn't support NEG (e.g., `relay.primal.net`).

## Cache adapters

`81effa63` — **fix(cache-sqlite-wasm): add degradedMode guards to prevent 'Worker not initialized' errors**
Race where worker init failed → subsequent ops crashed. Fix: guard rails to fall back to degraded mode.

`4645d590` — **fix(cache-sqlite-wasm): avoid schema_version errors on fresh db**

`50a23f57` — **fix(cache-sqlite-wasm): apply matchFilter and limits in worker**
Filter post-processing was happening on main thread; correctness/perf fix.

`2867c213` (PR #377) — **fix(cache-dexie): fix primary key of eventTags store**

## Misc reactive / build

`9679d461` — **fix: break infinite reactive loop in profile fetcher**
SvelteMap was tracked by `$effect`, causing infinite loop when fetchProfile failed (NDK not set on user). Switch to plain Map + ensure `NDKUser.ndk` set.

`f7d82bf6` — **fix(relay-info): use regular Map instead of SvelteMap for relay info cache**
Similar pattern — Svelte's reactive Maps cause loops in upstream-shared state.

`b55092b5` — **fix(registry): split EditProps effects for better reactivity**

`83e1884e` — **fix(registry): use derived state for loaded tracking in EditProps**

## Encryption / signature

`62be4021` — **fix: support @noble/curves v2 API in signature verification**
Library upgrade compat.

`85c7eb92` — **fix(blossom): handle UTF-8 characters in filenames during upload auth**
NIP-98 auth header was non-UTF-8-safe for filenames.

## Architectural

`53768a22` (PR #354) — **NDK v3.0: Major Architecture Overhaul & Performance Improvements**
The big monorepo restructure (packages moved out of `ndk-core`; new SRP splits). This is the inflection point — anything pre-3.0 docs may be outdated.

`21e02b36` — **refactor: eliminate subscription race condition with onEvent/onEose/onClose options**
Moved event delivery from EventEmitter pattern to explicit option callbacks, eliminating a class of order-of-attachment races.

## Top 8 for NMP to internalize

1. **Signer-before-pubkey ordering** (`a14c7a78`) — protocol invariant in any session swap.
2. **Logout race with async signer-setter** (`7d25cd76`) — clear state pre-emit.
3. **Outbox tracker bootstrap timing** (`a912a2c2`) — feed UI should wait for first relay-list batch.
4. **Stale connections on sleep/wake** (`33e75950`) — mobile-critical.
5. **Per-subscription dedup, not global** (`8b9a37cb`) — cache-less correctness.
6. **NIP-46 restore must `startListening`** (`d08415f6`) — persisted bunker invariant.
7. **Reactive Map traps** (`9679d461`) — don't put reactive collections in shared in-flight tracking.
8. **LRU TTL on seenEvents = 5min** (`e5901c98`) — long-tail event re-deliveries look "new".
