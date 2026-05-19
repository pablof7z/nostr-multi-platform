# Applesauce — Signers

> Source: `/private/tmp/nostr-research/applesauce` @ `da5ec22b`.

## 1. The `ISigner` contract

`packages/signers/src/interop.ts:5-16`:

```ts
export type ISigner = {
  getPublicKey: () => Promise<string>;
  signEvent: (template: EventTemplate) => Promise<NostrEvent>;
  nip04?: { encrypt(pk, pt): Promise<string>; decrypt(pk, ct): Promise<string> };
  nip44?: { encrypt(pk, pt): Promise<string>; decrypt(pk, ct): Promise<string> };
};
```

The four methods are the entire contract. `nip04` and `nip44` are optional and signer-dependent — readonly omits everything beyond `getPublicKey`, ExtensionSigner proxies to whatever the extension implements.

## 2. The eight signer implementations

| Signer | LOC | File | Notes |
| --- | ---: | --- | --- |
| `PrivateKeySigner` | 38 | `signers/private-key-signer.ts` | Plain in-memory key. NIP-04 + NIP-44 via nostr-tools. |
| `PasswordSigner` | 126 | `signers/password-signer.ts` | NIP-49 ncryptsec, locked-by-default; emits a deferred unlock promise. |
| `ReadonlySigner` | 56 | `signers/readonly-signer.ts` | Pubkey-only. All sign/encrypt throw. |
| `ExtensionSigner` | 36 | `signers/extension-signer.ts` | NIP-07 `window.nostr` proxy. Caches pubkey on first call. |
| `NostrConnectSigner` | 464 | `signers/nostr-connect-signer.ts` | NIP-46. Most complex by far. |
| `NostrConnectProvider` | 571 | `signers/nostr-connect-provider.ts` | The remote (signer-side) of NIP-46. |
| `AmberClipboardSigner` | 174 | `signers/amber-clipboard-signer.ts` | Android Amber via intent URIs + clipboard. |
| `AndroidNativeSigner` | 121 | `signers/android-native-signer.ts` | Capacitor plugin. |
| `SerialPortSigner` | 294 | `signers/serial-port-signer.ts` | Hardware signer over WebSerial. |
| `SimpleSigner` | 6 | `signers/simple-signer.ts` | Deprecated alias for PrivateKeySigner. |

For NMP, the relevant analogues are PrivateKey (in-memory dev), Password (encrypted at rest on disk/keychain), Readonly (npub login), Extension (web only — wasm), NostrConnect (NIP-46, all platforms), and platform-native (iOS Secure Enclave, Android Keystore — Applesauce doesn't have direct analogues but the contract shape is right).

## 3. `PrivateKeySigner` — the reference (`private-key-signer.ts`)

38 lines. Holds `Uint8Array` key, derives pubkey on demand, signs via nostr-tools `finalizeEvent`, encrypts via `nip04`/`nip44.v2`. `fromKey(privateKey: Uint8Array | string)` normalizes hex / nsec via `normalizeToSecretKey` (line 33-37). Nothing surprising — this is the shape every other signer should reduce to.

## 4. `PasswordSigner` — locked-by-default with deferred unlock (`password-signer.ts`)

Key idea: the signer can be created without a key (just an ncryptsec + lock state). Any call to a privileged method (`getPublicKey`, `signEvent`, encrypt/decrypt) routes through `requestUnlock()`:

```ts
// :39-45
protected requestUnlock() {
  if (this.key) return;
  if (this.unlockPromise) return this.unlockPromise;
  const p = createDefer<void>();
  this.unlockPromise = p;
  return p;
}
```

`requestUnlock` returns the **same** deferred promise across all callers — so 50 simultaneous signing attempts block on one unlock prompt. The application UI subscribes (somehow — not in this file) to know "I need to ask the user for a password." The unlock side calls `unlock(password)` (lines 63-74) which decrypts and assigns `this.key`. There's no explicit resolution of `unlockPromise` in the code path — it's broken by the fact that `key` is now set, so the next time the queued operation re-checks it can proceed. **This is subtle.** See gotchas §G7.

`lock()` sets `key = null` but does not clear `unlockPromise`. Lock-then-unlock-then-sign should work but is not explicitly tested in the file.

`testPassword(password)` (lines 55-60) is the "verify without committing" path — useful for UX where you want to confirm the password before mutating state.

## 5. `ExtensionSigner` — pubkey caching (`extension-signer.ts`)

36 lines. Two important details:

- Pubkey is **cached after first successful `getPublicKey()`** (lines 17, 22-28). Commit `0867a502` added this; without it, every action triggered a window.nostr.getPublicKey() round-trip.
- `nip04`/`nip44` are getters that return `window.nostr?.nip04` directly (lines 10-15). If the extension lacks the namespace, `signer.nip04` is undefined — callers must check. **No proxy-with-error.** This means `account.nip04?.encrypt(...)` is the safe shape.
- `signEvent` verifies the returned event (line 32-33) — defensive against buggy extensions. NMP's signer wrapper should do the same.

The `ExtensionMissingError` class (line 6-7) is a discrete error type so UI can render "install nostr extension" CTAs without string-matching.

## 6. `NostrConnectSigner` — NIP-46 client (`nostr-connect-signer.ts`)

The complex one. 464 lines. Architecture:

- Holds a **local** `PrivateKeySigner` for client-side encryption to the remote (line 39, 122).
- Owns a `req: Subscription` (line 135) that opens a long-lived `subscriptionMethod(relays, [{kinds:[24133], '#p':[clientPubkey]}])` (lines 145-161). Uses `repeat()` + `retry()` to survive disconnects — see commit `e6d5613b` ("Fix nostr connect signers not reconnecting to relays").
- `requests: Map<id, Deferred>` (line 186) and `auths: Set<id>` (line 187) track pending RPCs.
- `handleEvent` (lines 190-239):
  - Verifies signature.
  - Drops events not from the configured remote (line 194) — this is how `connect()` is symmetric: the first event with `result === "ack"` (or the supplied secret) sets `remote = event.pubkey` (lines 207-214).
  - Decrypts content using whichever scheme the content looks like (`isNIP04` heuristic, line 199-201). Tries hidden-content cache first (line 198) for already-decrypted gift-wrap-style content.
  - Routes `auth_url` responses to `onAuth(url)` (lines 220-232) — the application supplies a popup-opener; the signer dedups via `auths` set so the popup opens once per request.
- `makeRequest(method, params, kind?)` (lines 252-278) generates a nanoid, encrypts with NIP-44, signs, publishes, awaits the deferred. Handles both `Promise` and `Observable` return shapes from `publishMethod` (lines 271-273).
- `waitForSigner(abort?)` (lines 306-322) — returns a promise that resolves on first ack from a remote. Supports AbortSignal (commit `c290264d`). `close()` cancels it (commit `29d53501`).
- `switchRelays()` (lines 413-430) — NIP-46 extension to ask the remote what relays to migrate to; if non-null, tears down the subscription and rebuilds with the new relays. Commit `0cdd0edd`.
- `fromBunkerURI(uri, opts)` (lines 453-463) — parses `bunker://`, constructs the client, connects with the supplied secret/permissions.

Notable design choices:

- The signer **owns the relay connection**, configured via `subscriptionMethod` / `publishMethod` / `pool` (interop.ts:32-81). Static fallbacks on the class (lines 52-56) let an app set globally. NMP should mirror this: signer-owned connection but with explicit dependency injection (no globals).
- Static `buildSigningPermissions(kinds)` (line 448) and `parseBunkerURI` (line 443) make discoverability easy.

## 7. `AmberClipboardSigner` — the gnarliest UX path (`amber-clipboard-signer.ts`)

This is the **what-not-to-do reference** as much as a reference implementation. The clipboard is the IPC channel:

- `intentRequest(intent)` (lines 60-71): opens the intent URI in a new window, then awaits a `pendingRequest: Deferred<string>` that gets resolved by a `visibilitychange` listener reading the clipboard 200ms after the page becomes visible again (lines 46-58).
- Race: the `visibilitychange` fires immediately on `window.open`, before the user has actually responded. Guarded with a 500ms `setTimeout` before installing the pending request (lines 65-68). See gotchas §G8.
- `rejectPending()` cancels any in-flight request — needed because the user could initiate a second signing before completing the first.
- All signing operations construct an `intent:...#Intent;scheme=nostrsigner;...end` URI with payload JSON-URL-encoded (lines 145-173).

For NMP iOS/macOS, the analogous path is the URL-scheme deeplink to nsec.app or a hardware signer; the Amber implementation is the closest mental model.

## 8. `AndroidNativeSigner` — Capacitor plugin proxy (`android-native-signer.ts`)

121 lines. Holds a `packageName` (the installed signer app id), a permissions list, cached pubkey. All methods delegate to `NostrSignerPlugin.<method>(packageName, payload, nanoid(), ...)` (lines 99-119). `setup()` (lines 49-61) calls `setPackageName` and `getPublicKey` once.

`signEvent` (lines 67-96) is the one with substance: it pre-computes the event hash via `getEventHash`, passes `id` to the plugin (which the plugin uses for request correlation), then verifies the returned signature. The pubkey is set on the template before hashing (line 71-74) — critical, since the plugin doesn't add it.

## 9. The account layer adds three things on top

`packages/accounts/src/account.ts` (187 lines) wraps `ISigner` with:

1. **Per-account request queue** (lines 137-187). `operation()` routes every signer call through `waitForQueue()` which chains promises. Reasons:
   - Some signers (Extension, NostrConnect) cannot handle concurrent requests reliably.
   - User-facing prompts (Password unlock, Amber intent) need serialization.
   - The queue is opt-out via `disableQueue` (lines 49, 150).
   - `abortQueue(reason)` cancels everything in-flight (lines 131-133, 138).

2. **`SignerMismatchError` post-conditions** (lines 33, 109, 123-124). After `getPublicKey`, asserts result matches account's stored pubkey. After `signEvent`, asserts `result.pubkey === this.pubkey` AND `result.id === precomputed id`. This **catches a signer modifying the event before signing**, e.g., a malicious extension changing kind. NMP **must** copy this.

3. **`metadata$` BehaviorSubject** (lines 51-57) for app-level per-account state (name, color, etc.) that survives serialization.

`packages/accounts/src/proxy-signer.ts` (36 lines, not read but inferred) provides a stable `ISigner` that proxies to whichever account is currently active in `AccountManager.active$` (`manager.ts:21-28`). This is what makes `new EventFactory({ signer: manager.signer })` work without re-binding when the active account changes.

## 10. `AccountManager` — serialization, swapping, queue (`packages/accounts/src/manager.ts`)

193 lines. Three concerns:

- **Account-type registry** (lines 33-42): `registerType(AccountClass)` keyed by `static type`. `fromJSON`/`toJSON` per class, manager handles the dispatch in `fromJSON([])` (lines 161-178).
- **Active account** (lines 99-115): `BehaviorSubject<IAccount | undefined>`, with set/clear/get. `removeAccount` clears active if the removed account was active (lines 82-84).
- **Replace** (lines 87-95): add new, swap active if old was active, remove old. This is the account-swap path that powered the bug in commit `96120548` ("Fix ActionRunner cache causing actions to have wrong `user` when accounts change") — see gotchas §G9.

## 11. NMP design implications

- The `ISigner` shape is right. Don't expand it. Optional `nip04`/`nip44` is the correct shape — different signers genuinely have different capability sets.
- Bake `SignerMismatchError`'s post-conditions into NMP's signer wrapper. They are cheap and catch a real class of attacks.
- The per-account queue is **load-bearing**, not an optimization. Without it, two concurrent React renders that both ask for `getPublicKey()` from a password-locked signer will trigger two unlock prompts.
- For NIP-46, copy the `repeat() + retry()` subscription pattern (commit `e6d5613b`). A bunker that drops a websocket should not silently kill the signer.
- For the Apple ecosystem specifically: model the platform-native signer (Secure Enclave / hardware key) after `AndroidNativeSigner` — proxy, cache pubkey, pre-compute id, verify signature.
- `NostrConnectSigner.switchRelays` is a real NIP-46 capability that NMP should support to allow bunkers to redirect clients to better relays without re-pairing.
- **The `bunker://` URI parser + connector is one static method (`parseBunkerURI` + `fromBunkerURI`)** — NMP framework-magic must expose this same one-call onboarding as `Signer.fromBunkerUri(url) -> ImpersonatedSigner`.
