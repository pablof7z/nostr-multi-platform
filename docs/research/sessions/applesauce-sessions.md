# Applesauce Accounts + Signers — Deep Dive

> Source: `/private/tmp/nostr-research/applesauce`. Applesauce uses the term **"accounts"** for what NDK calls "sessions"; the multi-account container is `AccountManager`. The wire-level signer abstraction is `ISigner`. See `docs/research/applesauce/signers.md` (already in repo) for the signer-only deep-dive; this doc covers the account/session layer + delta against the existing signer doc.

## 1. Package layout

- `packages/signers/src/interop.ts` (82 LOC) — the `ISigner` trait + connection-method plumbing.
- `packages/signers/src/signers/*` — 10 signer implementations (8 production + 2 deprecated/aliases). Per-signer LOC table in `docs/research/applesauce/signers.md:24-33`.
- `packages/accounts/src/account.ts` (188 LOC) — `BaseAccount` with the load-bearing per-account queue and signer-mismatch guards.
- `packages/accounts/src/manager.ts` (193 LOC) — `AccountManager` with rxjs `BehaviorSubject` for reactive active-account.
- `packages/accounts/src/proxy-signer.ts` (37 LOC) — the rxjs proxy that makes `manager.signer` stable across active-account swaps.
- `packages/accounts/src/accounts/*` — 9 account adapters (Extension, Password, Readonly, PrivateKey, NostrConnect, AmberClipboard, AndroidNative, SerialPort, Simple).

**No "session" concept** in applesauce. The library deliberately does NOT track per-account derived state (followSet, muteSet, etc.) — those live in the consumer's `EventStore` and are queried per-render. This is the cleanest separation in the JS ecosystem.

## 2. `ISigner` (`packages/signers/src/interop.ts:5-16`)

```ts
export type ISigner = {
  getPublicKey: () => Promise<string>;
  signEvent: (template: EventTemplate) => Promise<NostrEvent>;
  nip04?: { encrypt(pubkey, plaintext): Promise<string>; decrypt(pubkey, ciphertext): Promise<string> };
  nip44?: { encrypt(pubkey, plaintext): Promise<string>; decrypt(pubkey, ciphertext): Promise<string> };
};
```

Four methods total. Differences from NDK's `NDKSigner`:
- `getPublicKey` is async (NDK has both sync `pubkey` and async `user()`).
- `signEvent` returns the **full signed `NostrEvent`** (NDK returns only the sig).
- `nip04`/`nip44` are **optional namespaces**, not required methods — extension/readonly signers can lack them entirely without any "Not supported" stubs.
- **No `toPayload` on the signer.** Serialization is the account's job (`account.toJSON()`), so signers stay storage-agnostic.

**Applies to NMP:** the smaller surface is right. NMP's `IdentityModule::sign` (`crates/nmp-core/src/substrate/identity.rs:18-22`) currently takes `&UnsignedEvent` and returns `Result<SignedEvent, SigningError>` — already closer to applesauce's "return the full event" than NDK's "return sig only". **Keep this.**

**Applies to NMP:** the optional-namespace pattern for encryption (returning `Option<NipXxEncryptor>`) is what NMP's `IdentityModule` should grow into — different account types have genuinely different capability surfaces.

## 3. `IAccount` (`packages/accounts/src/types.ts:26-42`)

```ts
export interface IAccount<Signer extends ISigner = ISigner, SignerData = any, Metadata = any> extends ISigner {
  id: string;
  name?: string;
  pubkey: string;
  metadata?: Metadata;
  signer: Signer;
  type: string;
  disableQueue?: boolean;
  toJSON(): SerializedAccount<SignerData, Metadata>;
}
```

Three things this contract pins:
- **`IAccount extends ISigner`**: the account IS the signer the rest of the app uses. There's no separate "give me the signer for this account" step — the account is the surface, and it wraps signer calls with queue+mismatch guards.
- **`metadata: Metadata` (parameterized)**: per-account app-level state (display name, color, custom relays). The library doesn't touch it; the app does.
- **`SerializedAccount`** (`types.ts:12-23`) is `{id, type, pubkey, signer, metadata?}` — explicitly typed by the account type's `SignerData` and `Metadata` generics.

**Applies to NMP:** the "account wraps signer" pattern is the right shape for NMP's `IdentityModule::HumanAccount` etc. `crates/nmp-core/src/substrate/identity.rs:11` has `Descriptor: Clone + Serialize + DeserializeOwned` — that's the analog of `IAccountConstructor.fromJSON` (line 51-53).

## 4. `BaseAccount` — three load-bearing details (`packages/accounts/src/account.ts`)

### 4.1 Per-account request queue (lines 137-187)

```ts
protected waitForQueue<T>(operation: () => Promise<T>): Promise<T> {
    if (this.disableQueue) return operation();
    if (this.lock && this.abort) {
        const p = wrapInSignal(
            this.lock.then(() => { this.abort?.signal.throwIfAborted(); return operation(); }),
            this.abort.signal,
        );
        this.lock = p.catch(() => {}).finally(this.reduceQueue.bind(this));
        this.queueLength++;
        return p;
    } else {
        // ... first-in-queue path ...
    }
}
```

Every privileged call (`getPublicKey`, `signEvent`, `nip04.*`, `nip44.*`) routes through `operation() → waitForQueue` (lines 86-89, 60-73, 67-73, 107-128). Reasons captured in `docs/research/applesauce/signers.md:114-119`:
- Extension and NIP-46 signers cannot handle concurrent requests reliably.
- User-facing prompts (password unlock, amber intent) need serialization.
- `abortQueue(reason)` cancels everything in-flight on logout / signer swap.

**Applies to NMP:** the actor model gives NMP serialization-for-free per account (one inbox per account-domain handler). The lesson is that **two concurrent SignerCapability calls for the same account must not race**; the actor must serialize them per-account, not globally.

### 4.2 `SignerMismatchError` (lines 33, 109, 122-124)

```ts
signEvent(template: EventTemplate): Promise<NostrEvent> {
    if (!Reflect.has(template, "pubkey")) Reflect.set(template, "pubkey", this.pubkey);
    return this.operation(async () => {
      const id = getEventHash(template as UnsignedEvent);
      const result = await this.signer.signEvent(template);
      if (result.pubkey !== this.pubkey) throw new SignerMismatchError("Signer signed with wrong pubkey");
      if (result.id !== id) throw new SignerMismatchError("Signer modified event");
      return result;
    });
}
```

**Three integrity checks**:
1. Pre-compute the event hash; if the signer returns an event with a different `id`, the signer modified the template.
2. The returned `pubkey` must match the account's stored pubkey.
3. `getPublicKey` (line 106-112) similarly asserts the signer's pubkey matches the account's stored pubkey.

**Applies to NMP:** **MUST copy.** A malicious or buggy signer (compromised extension, broken bunker, hardware glitch) could otherwise sign events as a *different* user without detection. NMP's `IdentityModule::sign` wrapper must enforce both checks before returning `Ok(SignedEvent)` — currently no such assertion exists in `crates/nmp-core/src/substrate/identity.rs`.

### 4.3 `metadata$` BehaviorSubject (lines 51-57)

Per-account app-level state with rxjs reactivity. Not serialized by default — the app's `toJSON` override decides whether to persist metadata. **Applies to NMP:** NMP can support "account custom name / color / muted relays" the same way — per-account metadata is a domain-store concern, not a kernel concern. Keep it out of the `IdentityModule` trait surface.

## 5. `AccountManager` (`packages/accounts/src/manager.ts`)

### 5.1 Reactive active account (lines 10-15, 99-115)

```ts
active$ = new BehaviorSubject<IAccount<any, any, Metadata> | undefined>(undefined);
accounts$ = new BehaviorSubject<IAccount<any, any, Metadata>[]>([]);
// ...
setActive(id: string | IAccount<any, any, Metadata>) {
    const account = this.getAccount(id);
    if (!account) throw new Error("Cant find account with that ID");
    if (this.active$.value?.id !== account.id) {
        this.active$.next(account);
    }
}
```

`active$` and `accounts$` are rxjs `BehaviorSubject`s — views subscribe and re-render on `.next(...)`. The de-duplication check (`!== account.id`) avoids redundant re-renders.

**Applies to NMP:** rxjs maps mechanically to NMP's `AppUpdate` stream. The de-dup discipline matters: the actor should not emit a `FullState` if `active_pubkey` didn't actually change (`AppUpdate` deltas already cover this if implemented carefully).

### 5.2 The `ProxySigner` pattern (`packages/accounts/src/proxy-signer.ts`)

```ts
export class ProxySigner<T extends ISigner> implements ISigner {
    private _signer: T | undefined;
    protected get signer(): T {
        if (!this._signer) throw new Error(this.error || "Missing signer");
        return this._signer;
    }
    constructor(protected upstream: Observable<T | undefined>, protected error?: string) {
        this.upstream.subscribe((signer) => (this._signer = signer));
    }
    async signEvent(template: EventTemplate): Promise<NostrEvent> { return this.signer.signEvent(template); }
    async getPublicKey(): Promise<string> { return this.signer.getPublicKey(); }
    // ...
}
```

`AccountManager` exposes `signer: ISigner` (line 21-28) which is a `ProxySigner` over `active$`. **What this enables:** consumers do `new EventFactory({ signer: manager.signer })` *once*, and the factory continues to "just work" when the user switches accounts — the proxy always re-dispatches to whatever is in `active$`. **No re-binding required.**

**Applies to NMP:** this is exactly the problem NMP avoids by having the actor own the signer binding internally. Native code in NMP never holds a signer reference — it issues `AppAction::SendNote { … }` and the actor uses the currently-active account. **Confirms NMP's existing design.**

### 5.3 Account-type registry (lines 33-42, 161-178)

```ts
registerType<S extends ISigner>(accountType: IAccountConstructor<S, any, Metadata>) {
    if (!accountType.type) throw new Error(`Account class missing static "type" field`);
    if (this.types.has(accountType.type)) throw new Error(`An account type of ${accountType.type} already exists`);
    this.types.set(accountType.type, accountType);
}
// ...
fromJSON(accounts: SerializedAccount<any, Metadata>[], quite = false) {
    for (const json of accounts) {
        const AccountType = this.types.get(json.type);
        if (!AccountType) { if (!quite) throw new Error(`Missing account type ${json.type}`); else continue; }
        const account = AccountType.fromJSON(json);
        this.addAccount(account);
    }
}
```

Plus `registerCommonAccountTypes(manager)` (`accounts/common.ts:9-15`) which bulk-registers the 5 most-common types in one call.

**Applies to NMP:** type-tagged dispatch on a static `type` string is the same pattern NDK uses (`core/src/signers/registry.ts:7-14`). NMP's existing `IdentityModule::NAMESPACE` const (`identity.rs:9`) is the analog — confirm that the orchestrator code path uses this for serde dispatch.

### 5.4 `replaceAccount` (lines 87-95)

```ts
replaceAccount(old: string | IAccount, account: IAccount) {
    this.addAccount(account);
    const id = typeof account === "string" ? account : account.id;
    if (this.active$.value?.id === id) this.setActive(account);
    this.removeAccount(old);
}
```

Caused the bug in commit `96120548` (cited in `docs/research/applesauce/signers.md:132`) — "ActionRunner cache causing actions to have wrong `user` when accounts change." **Applies to NMP:** signer-rotation (e.g., user reconnects a bunker with a new local key) must invalidate any in-flight actions bound to the old account. The actor must abort and re-route.

## 6. Signer adapters — what's interesting

### 6.1 `NostrConnectSigner` + `parseBunkerURI` (`packages/signers/src/signers/nostr-connect-signer.ts` + `helpers/nostr-connect.ts`)

`helpers/nostr-connect.ts:86-98` shows the canonical bunker URI parser:

```ts
export function parseBunkerURI(uri: string): BunkerURI {
  const url = new URL(uri);
  // firefox puts pubkey part in host, chrome puts pubkey in pathname
  const remote = url.host || url.pathname.replace("//", "");
  if (!isHexKey(remote)) throw new Error("Invalid bunker URI: remote is not a valid hex key");
  const relays = url.searchParams.getAll("relay");
  if (relays.length === 0) throw new Error("Invalid bunker URI: missing relays");
  const secret = url.searchParams.get("secret") ?? undefined;
  return { remote, relays, secret };
}
```

Three validations: `isHexKey(remote)`, at-least-one relay, secret is optional. NDK's parser (`core/src/signers/nip46/index.ts:204-215`) has the same hostname/pathname dance but does NOT validate isHex — **applesauce's version is stricter**.

`NostrConnectSigner.fromBunkerURI(uri, opts)` (`nostr-connect-signer.ts:453-463`):

```ts
static async fromBunkerURI(uri, options?) {
    const { remote, relays, secret } = NostrConnectSigner.parseBunkerURI(uri);
    const client = new NostrConnectSigner({ relays, remote, ...options });
    await client.connect(secret, options?.permissions);
    return client;
}
```

One static method, parse + construct + connect. **Applies to NMP:** the NIP-46 module should expose exactly this shape — `IdentityModule::ExternalSigner::create(IdentityDescriptor::BunkerUri(uri))` parses and dials, returning a fully-connected signer (or `IdentityError::InvalidDescriptor(...)`).

### 6.2 Reconnect resilience (lines 145-161)

```ts
this.req = from(this.subscriptionMethod(this.relays, [{ kinds: [kinds.NostrConnect], "#p": [pubkey] }]))
    .pipe(repeat(), retry(), filter((event) => typeof event !== "string"))
    .subscribe(this.handleEvent.bind(this));
```

`repeat() + retry()` rxjs operators make the bunker subscription self-heal on websocket churn (commit `e6d5613b` per `docs/research/applesauce/signers.md:77`). **Applies to NMP:** NMP's NIP-46 client must reconnect transparently. The relay-pool subscription planner already does this for normal REQs; the NIP-46 client likely needs to ride that same machinery rather than open its own raw connection.

### 6.3 `switchRelays` (lines 413-430) — protocol extension

```ts
async switchRelays(): Promise<string[] | null> {
    await this.requireConnection();
    const result = await this.makeRequest(NostrConnectMethod.SwitchRelays, []);
    if (result !== null && Array.isArray(result) && result.length > 0) {
        this.log("Switching relays from", this.relays, "to", result);
        this.relays = result;
        if (this.listening) { await this.close(); await this.open(); }
    }
    return result;
}
```

Mirror of NDK's `switch_relays` (`core/src/signers/nip46/index.ts:375-411`). Both libraries support this NIP-46 extension; both tear down + rebuild the subscription. **Applies to NMP:** support it from day one — bunkers actively use it (nsec.app, AlbyHub).

### 6.4 `nostrconnect://` URI (client-initiated) (lines 432-440)

The reverse flow — the client publishes a `nostrconnect://` URI containing its local pubkey + secret + relays + metadata, and waits for any bunker to connect to it. Implementation in `helpers/nostr-connect.ts:156-167`. **Applies to NMP M6:** the spec mentions only `bunker://` (`docs/plan/m6-signers-write.md:13`); `nostrconnect://` is an addition worth tracking but can be M7+ since the bunker-paste flow covers 95% of M6's exit criterion.

### 6.5 Encryption queue *not* needed at signer level

Unlike NDK, applesauce does NOT have a per-signer encryption queue in `nostr-connect-signer.ts`. Instead, the **account-level queue in `BaseAccount.operation()`** (account.ts:86-89, 60-73) covers it. One queue, not two. **Applies to NMP:** preserve the single-queue principle. The actor serializes per-account; signer impls can be lock-free.

### 6.6 `AmberClipboardSigner` — clipboard IPC (`amber-clipboard-signer.ts`)

Already covered in `docs/research/applesauce/signers.md:94-103`. Key takeaways: a `visibilitychange` listener with a 500ms guard against the immediate-fire race; `rejectPending` for canceling in-flight requests; the entire signer is unusable on non-Android (`SUPPORTED` check, line 12-17). **Applies to NMP:** any iOS-side Secure-Enclave / external-app signer must have the same "visible again → read response → settle pending promise" shape, with explicit cancellation on logout/switch.

### 6.7 `AndroidNativeSigner` — Capacitor plugin (121 LOC)

Covered in `docs/research/applesauce/signers.md:105-109`. Important detail (lines 67-96): pre-computes the event id before sending to the plugin (`getEventHash(withPubkey)`), so the plugin returns a correlation id that the signer can verify. **Applies to NMP:** NMP's signer wrapper should likewise pre-compute id so the post-condition check (§4.2) is meaningful.

### 6.8 `ExtensionSigner` — pubkey caching (36 LOC)

Lines 17, 22-28: pubkey cached after first `getPublicKey()` call (commit `0867a502`). The cache is **per-instance**, not per-pubkey — a fresh `new ExtensionSigner()` re-prompts. **Applies to NMP:** the wasm/web bridge (post-M15) should similarly cache; pubkey is `O(1)` to compute server-side but `O(user-attention)` if the extension prompts for confirmation.

## 7. Persistence — applesauce vs NDK

| Aspect | applesauce | NDK |
| --- | --- | --- |
| What's persisted | `{id, type, pubkey, signer: SignerData, metadata}` per account | `{pubkey, signerPayload, lastActive, preferences}` per session |
| Active marker | NOT persisted by `AccountManager.toJSON()` (lines 143-155) | Persisted as `activePubkey` (`storage/local-storage.ts:19`) |
| Derived state (followSet, mutes) | NOT persisted, NOT tracked by account layer | NOT persisted, but tracked in-memory by session (`types.ts:9-30`) |
| Storage adapter abstraction | None — `toJSON()`/`fromJSON()`, caller writes | `SessionStorage` interface (`storage/types.ts:8-23`) |
| Auto-save | None — explicit `manager.toJSON()` calls | Debounced 500ms auto-save (`manager.ts:467-477`) |

**Applesauce's choice (no active marker)** is opinionated: the app re-prompts the user on every cold start. **NDK's choice (persist active)** is friendlier but means "active" can be stale across signer-rotation events. **Applies to NMP:** persist active pubkey per NDK; let the app config decide whether to re-prompt anyway (UX-level decision, not framework).

## 8. NMP design implications (applesauce-specific)

1. **`IAccount extends ISigner`** is the cleanest contract. NMP's `IdentityModule::sign` already takes `(ctx, id, unsigned)` — the account is implicit in `id`. Keep it.
2. **Per-account queue is load-bearing.** Even if NMP's actor model serializes per-account naturally, document that the framework MUST never call a signer concurrently for the same account.
3. **Signer-mismatch post-conditions** are mandatory. Add to `IdentityModule::sign` wrapper: assert returned `pubkey == account.pubkey` AND `id == precomputed_id`.
4. **Type-tagged registry** — `IdentityModule::NAMESPACE` is already in place; ensure serde dispatch uses it.
5. **`bunker://` parser**: 13 LOC, fully testable. NMP's NIP-46 module should ship this as `IdentityDescriptor::parse_bunker_uri(s: &str) -> Result<BunkerUri, IdentityError>`.
6. **No "active marker" in serialization is an option**, but NDK's choice (persist activePubkey) better fits NMP's "session resumption" UX. Persist it.
7. **`metadata$` per-account state** belongs in NMP's domain stores, not the IdentityModule. Don't pollute `Descriptor`.
8. **NostrConnect's `repeat() + retry()` for reconnect** maps to NMP's relay pool — the NIP-46 client must be a *consumer* of the pool, not a peer of it.
9. **`switchRelays` NIP-46 extension** — implement from M6. 5s timeout is the right default.
