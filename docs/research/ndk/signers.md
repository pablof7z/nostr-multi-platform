# NDK Signers

Lives in `core/src/signers/`. The signer contract is `core/src/signers/index.ts:NDKSigner`.

## Interface

```
core/src/signers/index.ts:18-84
interface NDKSigner {
    get pubkey(): string;                // sync, throws "Not ready" if not loaded
    blockUntilReady(): Promise<NDKUser>; // setup handshake
    user(): Promise<NDKUser>;
    get userSync(): NDKUser;
    sign(event: NostrEvent): Promise<string>;
    relays?(ndk?: NDK): Promise<NDKRelay[]>;
    encryptionEnabled?(scheme?: NDKEncryptionScheme): Promise<NDKEncryptionScheme[]>;
    encrypt(recipient: NDKUser, value: string, scheme?: NDKEncryptionScheme): Promise<string>;
    decrypt(sender: NDKUser, value: string, scheme?: NDKEncryptionScheme): Promise<string>;
    toPayload(): string;
}

interface NDKSignerStatic<T extends NDKSigner> {
    fromPayload(payload: string, ndk?: NDK): Promise<T>;
}
```

Serialization: `toPayload()` returns a JSON string `{ type, payload }`. `deserializeSigner` (`core/src/signers/deserialization.ts`) routes by `type` to the right `fromPayload`. The signer registry (`registry.ts`) is populated by side-effect imports — each signer file calls `registerSigner(type, fromPayload)` on module load.

## Built-in implementations

### `NDKPrivateKeySigner` (`core/src/signers/private-key/index.ts`)

- Backed by `@noble/hashes` + `nostr-tools`.
- Constructor accepts hex private key, `Uint8Array`, or `nsec1...` bech32 (auto-decoded — explicit guardrail in JSDoc against pre-decoding).
- `.generate()` static for fresh keys; `.nsec`/`.npub` getters.
- Supports NIP-04 and NIP-44 encrypt/decrypt.
- NIP-49 (ncryptsec password encryption): instance method `toNCryptSec(password, logn?)` and static `fromNCryptSec(nsec, password)`.

### `NDKNip07Signer` (`core/src/signers/nip07/index.ts`)

- Browser extension bridge (window.nostr). Lazy `waitForExtension()` with configurable `waitTimeout` (default 1000ms).
- Has its own `encryptionQueue` and sequential processor — many extensions don't tolerate concurrent encryption calls.
- `pubkey` getter throws "Not ready" before `blockUntilReady()` resolves.
- `relays(ndk)` returns relays from `window.nostr.getRelays()` if extension supports it.
- No persistence — `toPayload()` returns just `{ type: "nip07" }`; on `fromPayload` the user must re-confirm via the extension.

### `NDKNip46Signer` (`core/src/signers/nip46/index.ts`)

Remote signer over Nostr (NIP-46). Most complex signer.

Three flows:
- `bunker(ndk, bunkerUriOrNip05, localSigner?)` — connect to a bunker URL/connection token. Optional local signer for local-side encryption.
- `nostrconnect(ndk, relayUrls)` — generate a `nostrconnect://` URI for the user's signer app to connect to. Uses `nostrConnectSecret` for handshake.
- Reconstructed via `fromPayload`.

Key methods:
- `blockUntilReady()` — opens RPC subscription, performs `connect`/`get_public_key` handshake.
- `startListening()` — opens the kind 24133 subscription that receives RPC responses.
- `sign/encrypt/decrypt` — proxied as RPC calls; timeout-capable (`signer.timeout = N` in ms, throws `NDKNip46TimeoutError`).

Critical gotcha (`d08415f6` Feb 2026):
> `fromPayload()` previously did NOT call `startListening()`. Restored signers would hang forever on first sign/encrypt because no subscription was listening for the response. Fix adds `await signer.startListening()` to `fromPayload`.

### Mobile signers (`mobile/src/signers/`)

- `nip55.ts` — Android Amber-style external signer over intent broadcast (`NDKNip55Signer`).
- Exposed via `mobile/src/signers/index.ts`.

## Session ↔ signer coupling

Sessions store maintains `signers: Map<Hexpubkey, NDKSigner>` separately from the session data map. On `switchToUser`:

```
sessions/src/store.ts:281-282
if (state.ndk) {
    state.ndk.signer = signer;
    // ... session state changes ...
    set({ activePubkey: pubkey });  // ← AFTER signer set
}
```

The `ndk.signer = ...` setter (in `core/src/ndk/index.ts`) has an **async side effect** that queues a promise to resolve `activeUser`. This was the root of the logout race (`7d25cd76`): subscription handlers re-setting `ndk.signer = undefined` correctly, but a pending promise from a stale set would resolve later and re-set `activeUser`. Fixed by clearing NDK state *before* triggering subscriptions during `removeSession`.

Race fix `a14c7a78` (Oct 2025): always set `ndk.signer` synchronously BEFORE `set({ activePubkey: ... })` so reactive consumers don't observe pubkey-without-signer.

## Encryption scheme negotiation

`encryptionEnabled(scheme?)` returns the list of schemes the signer supports. Pattern: check before calling encrypt, fall back to NIP-04 if NIP-44 unavailable. Most callers use `EncryptionMethod`-typed helpers in `core/src/events/encryption.ts`.

## Implications for NMP

- **Signer interface is generous**: NMP can implement Swift/Kotlin signers that conform to a translated `NDKSigner` protocol. The hard part is faithful NIP-46 if NMP wants remote-signing parity.
- **NIP-46 round-trip** is async and times out — UX needs spinners, not blocking modals.
- **NIP-07 doesn't exist on mobile** — equivalents are NIP-55 (Android intent), Lightning Wallet-style intent flows on iOS, or a custom in-app signer.
- **Serialization** is the cleanest cross-platform contract: store `{ type, payload }` JSON, reconstruct on app restart. Mobile equivalent must produce the same payload schema if cross-app interop is wanted.
- **bunker:// URL parse and connection flow** is a one-action UX in NDK (`NDKNip46Signer.bunker(ndk, uri)`) — NMP framework-magic must give apps the same one-call onboarding.
