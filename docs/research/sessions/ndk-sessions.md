# NDK Sessions + Signers — Deep Dive

> Source: `/Users/pablofernandez/Work/NDK-nhlteu` (master). Read for NMP M6 to learn what the most-deployed JS Nostr library actually does about multi-account, signer plumbing, persistence boundaries, and reactive switching.

## 1. Package layout

NDK's session story lives in **three** packages, not one:

- `sessions/src/*` (1.7k LOC) — pure session/signer state machine with pluggable storage. Built on `zustand/vanilla` (`store.ts:11`).
- `core/src/signers/*` — the `NDKSigner` trait + registry + four built-in signers (PrivateKey, NIP-07, NIP-46 client+backend); `mobile/src/signers/nip55.ts` adds the Android Amber bridge by calling `registerSigner("nip55", …)`.
- `react/src/session/*` — a *parallel* zustand store that pre-dates the new `sessions` package and is still the React integration layer (`react/src/session/store/types.ts:16`). Diverges in field names (`NDKUserSession` vs `NDKSession`) and embeds profile resolution.

**Applies to NMP:** the kernel must do the work of NDK's `sessions` package internally — there will be no separate per-frontend session store. Native shadows derive from `AppState` per D4.

**Diverges from NMP:** zustand-vanilla as the reactivity primitive is a JS-only choice; NMP's equivalent is the actor + `AppUpdate::FullState`/`ViewBatch` pipeline (`docs/product-spec/api-surface.md:170`).

## 2. The `NDKSession` shape

`sessions/src/types.ts:7-51` — what NDK keeps **per active account** (in memory):

```ts
export interface NDKSession {
    pubkey: Hexpubkey;
    followSet?: Set<Hexpubkey>;        // from kind:3
    muteSet?: Map<string, string>;     // from kind:10000 ("p"|"e" → id)
    mutedWords?: Set<string>;          // from kind:10000 "word" tags
    blockedRelays?: Set<string>;       // from kind:10001
    relayList?: Map<string, { read: boolean; write: boolean }>; // kind:10002
    events: Map<NDKKind, NDKEvent | null>;   // replaceable-event cache
    subscriptions: NDKSubscription[];        // live REQs owned by this session
    lastActive: number;
    preferences?: SessionPreferences;        // app-level (only walletEnabled today)
}
```

What gets **persisted** is much smaller (`sessions/src/types.ts:135-140`):

```ts
export interface SerializedSession {
    pubkey: Hexpubkey;
    signerPayload?: string;   // result of NDKSigner.toPayload()
    lastActive: number;
    preferences?: SessionPreferences;
}
```

Comment on the serializer (`sessions/src/persistence-manager.ts:65-67`): *"Only restores identity and signer — data will be fetched from relays/cache."*

**Applies to NMP:** the persistence boundary is principled — persist *identity + signer descriptor + user preferences*; everything else is derived from the event store at next start. This maps cleanly onto NMP's actor-owned domain stores: the kernel persists `AccountId` + `IdentityDescriptor` blob; follow set, mute set, relay list are derived from the verified event store.

## 3. `NDKSessionManager` — the public API

`sessions/src/manager.ts:54-478`. Public surface that NMP's `IdentityModule` API should compare against:

- `login(userOrSigner, { setActive })` (line 147) — accepts a signer OR a bare user (read-only account). Returns the pubkey.
- `createAccount({ profile, relays, wallet, follows }, { publish, signer })` (line 193) — generates an nsec, optionally publishes kind:0/10002/3 + sets up NIP-60 wallet.
- `switchTo(pubkey | null)` (line 280) — null deactivates.
- `logout(pubkey?)` (line 273) — defaults to active.
- `startSession(pubkey, opts: SessionStartOptions)` (line 287) / `stopSession` (line 294) — control what kinds get subscribed for this pubkey.
- `addMonitor([NDKEventConstructor | NDKKind, ...])` (line 307) — incremental kind-monitor extension on the active session.
- `enableWallet(pubkey?)` / `disableWallet` / `isWalletEnabled` (lines 322–394) — *opt-in* fetching for NIP-60 wallet (kind:17375 etc.). Crucially, this **restarts the subscription** so wallet kinds are not over-fetched for accounts that won't use them.
- `restore()` (line 413) — load + start subscriptions for restored sessions; honors per-session `walletEnabled` preference.
- `subscribe(cb)` (line 406) — pass-through to zustand subscribe.

**Applies to NMP:** *opt-in subscription per concern* is right. NMP should keep this discipline (follows, mutes, relay list, wallet are separate fetch toggles). Maps to NMP's `ViewSpec` discipline (D5 snapshots bounded by what's open).

**Applies to NMP:** `createAccount` packages a "generate + publish kind:0/10002/3 + opt-in wallet" flow into one call — matches M6's "Create new nsec flow" deliverable in `docs/plan/m6-signers-write.md:14`.

**Diverges from NMP:** NDK's manager owns NDK-instance side effects directly (`ndk.signer = …`, `ndk.muteFilter = …`). NMP's signer is selected by `ActivateAccount { pubkey }` (`docs/product-spec/api-surface.md:108`) and bound through the actor; no public assignment of a global "current signer" exists.

## 4. The store layer — what reactivity buys you

`sessions/src/store.ts` is the heart. Three patterns worth importing into NMP's mental model:

### 4.1 Switch order matters (`store.ts:254-329`)

```ts
switchToUser: async (pubkey: Hexpubkey | null) => {
    // ...
    // Update NDK signer BEFORE setting activePubkey
    // This prevents race conditions where reactive code runs before signer is ready
    if (state.ndk) {
        state.ndk.signer = signer;
        // ... muteFilter / relayConnectionFilter rewire ...
    }
    set({ activePubkey: pubkey });
    // ...
    if (state.ndk && signer) {
        const user = await signer.user();
        user.ndk = state.ndk;
        state.ndk.activeUser = user;
    }
}
```

The signer is wired *before* `activePubkey` flips so any zustand subscriber waking from the activePubkey change sees a fully-configured NDK. **Applies to NMP:** the actor's `ActivateAccount` handler must publish a single `AppUpdate::FullState` after *all* derived state is rebuilt — partial-update visibility is a class of bug NDK hit (the comment is "//prevents race conditions").

### 4.2 The mute/relay filter is rewired on switch, not stored

`store.ts:286-316` — `ndk.muteFilter` becomes a closure over the *session's* mute data; `ndk.relayConnectionFilter` becomes a closure over `blockedRelays`. Both are cleared on switch-to-null. **Applies to NMP:** the kernel's per-account filter sets must be rebound atomically with `ActivateAccount`; nothing outside the actor should hold a "current mute filter" reference.

### 4.3 kind:3 / kind:10002 / kind:10000 auto-rewire is integral

`store.ts:417-444` builds the subscription kinds list mechanically from `SessionStartOptions`:

```ts
function buildSubscriptionKinds(opts: SessionStartOptions, monitorKinds: NDKKind[]): NDKKind[] {
    const kinds: NDKKind[] = [];
    if (opts.follows) kinds.push(NDKKind.Contacts);
    if (opts.mutes) kinds.push(NDKKind.MuteList);
    if (opts.blockedRelays) kinds.push(NDKKind.BlockRelayList);
    if (opts.relayList) kinds.push(NDKKind.RelayList);
    if (opts.wallet) kinds.push(NDKKind.CashuWallet, NDKKind.CashuMintList);
    kinds.push(...monitorKinds);
    return kinds;
}
```

Then `handleIncomingEvent` (`store.ts:449-487`) dispatches per-kind to `handleContactListEvent` / `handleMuteListEvent` / `handleBlockRelayListEvent` / `handleRelayListEvent`. Each handler:
1. Loads the existing event for that kind (`session.events.get(event.kind)`).
2. Drops events with same id or older `created_at` (`store.ts:498-499`, repeated in every handler).
3. Rebuilds the derived set (`followSet` / `muteSet` / `mutedWords` / `relayList`) **and** stores the raw event for later inspection.

**Applies to NMP:** the kernel's account-data planner should similarly drive a fixed subscription (kind:3 + kind:10002 + kind:10000 + kind:10001 for the active account) and rebuild derived domain rows on every newer-`created_at` arrival. This is mechanically the right way to satisfy D4 (single writer per derived fact).

**Diverges from NMP:** NDK puts both the raw event *and* derived sets on the same session struct. NMP would put the raw event in the verified event store and the derived `FollowSet` / `MuteSet` in their own domain stores per `docs/product-spec/subsystems.md:445`. Same data, different home.

## 5. The signer trait (`core/src/signers/index.ts:18-84`)

```ts
export interface NDKSigner {
    get pubkey(): string;                                     // sync; throws "Not ready"
    blockUntilReady(): Promise<NDKUser>;                      // initializer
    user(): Promise<NDKUser>;
    get userSync(): NDKUser;                                  // throws if not ready
    sign(event: NostrEvent): Promise<string>;                 // returns sig only
    relays?(ndk?: NDK): Promise<NDKRelay[]>;                  // optional NIP-65 hint
    encryptionEnabled?(scheme?: NDKEncryptionScheme): Promise<NDKEncryptionScheme[]>;
    encrypt(recipient: NDKUser, value: string, scheme?): Promise<string>;
    decrypt(sender: NDKUser, value: string, scheme?): Promise<string>;
    toPayload(): string;                                      // type-tagged JSON
}

export interface NDKSignerStatic<T extends NDKSigner> {
    fromPayload(payload: string, ndk?: NDK): Promise<T>;
}
```

Notable choices:
- `pubkey` getter is **sync** but can throw `"Not ready"` for NIP-07/NIP-46 — caller must call `blockUntilReady()` first. Documented at `index.ts:21-23`.
- `sign` returns only the signature string, not a signed event. The caller assembles.
- `encrypt`/`decrypt` are required, but `encryptionEnabled?` is optional capability-probing — implementations that lack any encryption scheme can omit it entirely. The schemes themselves are split by string (`"nip04" | "nip44"`). Total: **8 required + 2 optional** members (`relays?` and `encryptionEnabled?` are the two optionals).
- Serialization is via two methods: `toPayload()` returns `{type, payload}` JSON; the registry (`registry.ts:7-14`) dispatches to `static fromPayload`. Adding a new signer type means calling `registerSigner("name", Class)` at module import (e.g. `private-key/index.ts:241`, `nip07/index.ts:302`, `nip46/index.ts:613`, `mobile/src/signers/nip55.ts:185`).

**Applies to NMP:** the **type-tagged registry** for signer serialization is the right pattern. NMP already has `IdentityModule::Descriptor` (`crates/nmp-core/src/substrate/identity.rs:11`) which is the moral equivalent. Make sure the registry is in `nmp-core` so extension modules (e.g. an NIP-46 module) can register without core knowing the type.

**Applies to NMP:** errors-as-sentinels (`throw new Error("Not ready")`) is exactly what D6 forbids. NMP must return `Result<SignedEvent, SigningError>` (already does — `identity.rs:18-22, 72-76`).

## 6. The four built-in signers

### 6.1 `NDKPrivateKeySigner` (`core/src/signers/private-key/index.ts`, 242 LOC)

Accepts hex *or* nsec via one ctor (line 43-58 — note the `nsec1`/length-64 sniff). Adds NIP-49 helpers `encryptToNcryptsec(password, logn, ksb)` (line 111) and `fromNcryptsec(ncryptsec, password)` (line 140). `toPayload` (line 211) stores the hex key in plaintext — **explicit choice not to wrap in NIP-49 at the persistence layer**; that's the consumer's responsibility.

**Applies to NMP:** the `KeychainCapability` from `docs/product-spec/api-surface.md:198-203` is exactly the layer NDK leaves to the consumer. The kernel should NIP-49-encrypt nsecs before handing them to `KeychainCapability::store`, since `KeychainCapability` only sees opaque bytes. (`docs/plan/m6-signers-write.md:14-15` agrees: "Create new nsec flow: generate, encrypt (NIP-49), and store via `KeychainCapability`.")

### 6.2 `NDKNip07Signer` (`core/src/signers/nip07/index.ts`, 303 LOC)

Wraps `window.nostr`. Three things to copy:
- `waitForExtension()` (line 222-246) polls with `setInterval` + bounded `waitTimeout`. Returns rejection if extension never appears.
- **Encryption queue** (`encryptionQueue` line 34, `processEncryptionQueue` line 169) — serializes encrypt/decrypt calls and retries on `"call already executing"` with backoff. Without this, two concurrent encrypts to a NIP-07 extension race.
- `toPayload` (line 253) stores only `{"type":"nip07","payload":""}` — there's nothing to persist; on restore a fresh `NDKNip07Signer` is constructed and re-prompts.

**Applies to NMP:** on web (post-M15), the NMP wasm shim needs the same queue behavior. The wasm/JS bridge runs in the same single-threaded event loop, so a queue at the JS bridge layer (analogous to applesauce's `BaseAccount.operation()`) handles the same race.

### 6.3 `NDKNip46Signer` (`core/src/signers/nip46/index.ts`, 614 LOC)

The biggest. Three constructor flows distinguished by string-shape (`index.ts:149-156`):

```ts
if (userOrConnectionToken === false) { /* deserialization path */ }
else if (!userOrConnectionToken) { this.nostrconnectFlowInit(nostrConnectOptions); }
else if (userOrConnectionToken.startsWith("bunker://")) { this.bunkerFlowInit(userOrConnectionToken); }
else { this.nip05Init(userOrConnectionToken); }
```

The `bunker://` parser (`index.ts:204-215`):

```ts
private bunkerFlowInit(connectionToken: string) {
    const bunkerUrl = new URL(connectionToken);
    const bunkerPubkey = bunkerUrl.hostname || bunkerUrl.pathname.replace(/^\/\//, "");
    const userPubkey = bunkerUrl.searchParams.get("pubkey");
    const relayUrls = bunkerUrl.searchParams.getAll("relay");
    const secret = bunkerUrl.searchParams.get("secret");
    this.bunkerPubkey = bunkerPubkey; this.userPubkey = userPubkey;
    this.relayUrls = relayUrls; this.secret = secret;
}
```

Note: `hostname || pathname.replace(/^\/\//, "")` to defend against Firefox vs Chrome differences in WHATWG URL parsing (also explicitly called out in applesauce — see `applesauce-sessions.md` §6). The `secret` parameter is forwarded to the bunker's `connect` RPC (`index.ts:347-351`) — it is the bunker's challenge-response handshake.

`blockUntilReady` for the bunker flow (`index.ts:315-368`): subscribes on `localPubkey`'s NIP-46 RPC topic, sends a `connect` request with `[userPubkey, secret?]`, awaits `"ack"`, then issues `get_public_key`, then `switchRelays`. The `authUrl` event (line 342-344) lets the application open a popup for additional auth.

`switchRelays` (`index.ts:375-411`): a NIP-46 protocol extension where the bunker can tell the client which relays to migrate to. Times out at 5s if the bunker doesn't reply — important so old/simple bunkers don't hang the login.

`toPayload` (`index.ts:536-553`) persists `bunkerPubkey + userPubkey + relayUrls + secret + localSignerPayload`. `fromPayload` (`index.ts:562-610`) re-subscribes and is ready to sign — **does not re-issue `connect`** because the bunker already trusts this `localSigner`. This is the "reconnect after app restart" path.

**NDKNip46TimeoutError** (`index.ts:21-33`) — typed timeout error with the operation name. `signer.timeout` is opt-in (default undefined). **Applies to NMP:** the `IdentityError` enum in `identity.rs:50-53` needs a `Timeout(operation, ms)` variant (currently only has `InvalidDescriptor` and `Storage`).

### 6.4 `NDKNip55Signer` (`mobile/src/signers/nip55.ts`, 186 LOC)

Android Amber bridge via `expo-nip55`. Important shape: stores `packageName` + cached `_pubkey` in `toPayload` (line 139) so re-creating the signer doesn't re-prompt the user for app selection. **Applies to NMP:** the Apple Secure Enclave / iOS-passkey signer (post-M6) will have the same shape — store an opaque platform-side handle (`packageName` analog) plus cached pubkey; on restore, the platform-side capability uses that handle to re-establish the link.

## 7. Storage adapters (`sessions/src/storage/*.ts`)

Three implementations behind one interface (`storage/types.ts:8-23`):

```ts
export interface SessionStorage {
    save(sessions: Map<Hexpubkey, SerializedSession>, activePubkey?: Hexpubkey): Promise<void>;
    load(): Promise<SessionData>;
    clear(): Promise<void>;
}
```

`LocalStorage` (`local-storage.ts`, 59 LOC) — `localStorage[key] = JSON.stringify(...)`. Throws `StorageError("localStorage is not available")` outside browsers (line 14-17). `FileStorage` (`file-storage.ts`, 56 LOC) — dynamic-imports `fs/promises` so it tree-shakes out of browser bundles. `MemoryStorage` for tests. **No platform-native (Keychain/Keystore) storage** — those are explicitly the consumer's job, same boundary as NMP's `KeyringCapability`.

`PersistenceManager` (`persistence-manager.ts`) is the orchestration layer. Save (line 43-51) serializes all sessions, writes. Restore (line 23-39) loads, recreates sessions WITHOUT activating, then activates the saved active pubkey. **Important:** `restoreSession` (line 68-95) tolerates a missing signer payload — the session re-appears as read-only if the signer fails to deserialize (e.g. NIP-07 extension uninstalled, NIP-46 bunker pubkey rotated). This is how a "lost signer" downgrades gracefully to npub-only mode rather than crashing the session.

**Applies to NMP:** The kernel must do the same — a startup-time signer-resurrection failure must downgrade to read-only and surface a toast (per D6), not break the session entirely.

## 8. Errors (`sessions/src/utils/errors.ts`)

Six error classes total — base `SessionError extends Error` plus five typed subclasses. The discriminator is `error.name` (set in each constructor). Subclasses: `SignerDeserializationError`, `StorageError`, `SessionNotFoundError`, `NoActiveSessionError`, `NDKNotInitializedError`. **Applies to NMP:** NMP's `IdentityError` enum (`identity.rs:50-53`) currently has only two variants; the equivalent set is needed when M6 lands — at minimum `NotFound(pubkey)`, `NotActive`, `SignerRestoreFailed(reason)`, `KeyringUnavailable`, `Timeout(op, ms)`.

## 9. Auto-save (`manager.ts:467-477`)

```ts
private setupAutoSave(): void {
    const debouncedPersist = debounce(() => {
        this.persistenceManager.persist().catch((error) => {
            console.error("Failed to auto-save sessions:", error);
        });
    }, this.options.saveDebounceMs!);
    this.unsubscribe = this.store.subscribe(() => { debouncedPersist(); });
}
```

Subscribes to zustand store, debounces a save by `saveDebounceMs` (default 500ms, `manager.ts:65`). Save errors are *logged*, not propagated — partial-failure isolation. **Applies to NMP:** session persistence in NMP should be similarly debounced (the actor can use a heartbeat tick rather than a JS debounce). Errors become toast state per D6, not bubbled to the platform.

## 10. The React layer (`react/src/session/store/*`) — divergence note

The react package has its **own** zustand store that pre-dates the `sessions` package. Key differences worth knowing because it's what the iOS app's mental model probably came from:

- `NDKUserSession` (`react/src/session/store/types.ts:16-38`) carries `profile?: NDKUserProfile` directly on the session — the new `sessions/types.ts:7` does NOT (profile is its own concern).
- `switchToUser` in react (`react/src/session/store/switch-to-user.ts:13-70`) couples with a separate `useNDKMutes` store and a `useNDKStore.getState().setSigner(signer)` — i.e., the signer lives in yet another zustand atom. This is the "monolithic Nostr client object quietly accumulating concerns" pattern NMP's lessons doc warns against (`docs/design/ndk-applesauce-lessons.md:31`).

**Diverges from NMP:** NMP must NOT replicate the multi-store split. Identity, mutes, signer are all owned by the actor and projected through `AppState`.

## 11. NDK summary table — what to copy, what to skip

| Pattern | Cite | NMP verdict |
| --- | --- | --- |
| Type-tagged signer registry | `core/src/signers/registry.ts:7-14` | **copy** — fits `IdentityModule::Descriptor` |
| `toPayload`/`fromPayload` per signer | `nip07/index.ts:253-278`, `nip46/index.ts:536-610` | **copy** the principle; use serde+enum dispatch in Rust |
| Persist identity+signer only; derive everything else from events | `persistence-manager.ts:65-67` | **copy** — matches D4 |
| Per-session opt-in fetch toggles (`follows`, `mutes`, `wallet`, …) | `manager.ts:287, 322, 356` | **copy** — fits ViewSpec + D5 |
| Switch order: signer before activePubkey, atomic publish | `store.ts:279-329` | **copy as actor invariant** |
| Storage adapter interface w/ memory/file/local impls | `storage/types.ts:8-23` | **copy** — but NMP's adapter is LMDB + `KeyringCapability` |
| `NDKNip46TimeoutError` typed timeout | `nip46/index.ts:21-33` | **copy** — add to `IdentityError` |
| `restoreSession` tolerates missing signer → read-only | `persistence-manager.ts:75-88` | **copy** as actor behavior |
| zustand-vanilla as primitive | `store.ts:11, 98` | **skip** — NMP uses actor + `AppUpdate` |
| Global `ndk.signer = …` mutation | `store.ts:282` | **skip** — NMP uses `ActivateAccount` intent |
| Direct `ndk.muteFilter = (event) => …` closure | `store.ts:286-302` | **skip** — NMP filters live in domain stores |
| Profile on session struct | `react/src/session/store/types.ts:18` | **skip** — profile is its own domain store |
| React package's parallel store | `react/src/session/store/*` | **anti-pattern** — never repeat |
