# NMP M6 Sessions + Signers — Synthesis & Recommendation

> Reads: `ndk-sessions.md`, `applesauce-sessions.md`, existing `docs/research/applesauce/signers.md`, NMP doctrine D0–D8 (`docs/product-spec/overview-and-dx.md` §1.5), `docs/plan/m6-signers-write.md`, current Rust shape in `crates/nmp-core/src/substrate/identity.rs`.

## 1. The nine questions, answered

### 1.1 Signer trait shape

**Recommendation:** keep current `IdentityModule::sign(ctx, id, &UnsignedEvent) -> BoxFuture<Result<SignedEvent, SigningError>>` (`identity.rs:18-22`). Add three things:

a. **Encryption namespace** — mirror applesauce's optional capability pattern (`interop.ts:8-15`):

```rust
fn nip04(ctx: &IdentityContext, id: &IdentityId) -> Option<Box<dyn Nip04Capability>>;
fn nip44(ctx: &IdentityContext, id: &IdentityId) -> Option<Box<dyn Nip44Capability>>;
```

Returning `Option` means readonly / extension-without-nip04 accounts don't have to throw at call time.

b. **Mandatory post-conditions in the wrapper**, NOT in each implementation. Per applesauce's `account.ts:115-128`:

```rust
async fn sign_with_guards(...) -> Result<SignedEvent, SigningError> {
    let precomputed_id = compute_event_id(&unsigned);
    let signed = T::sign(ctx, id, &unsigned).await?;
    if signed.id != precomputed_id { return Err(SigningError::SignerModifiedEvent); }
    if signed.unsigned.pubkey != account_pubkey { return Err(SigningError::SignerPubkeyMismatch); }
    Ok(signed)
}
```

c. **Sync vs async:** all methods async (return `BoxFuture`). NDK's sync `pubkey` getter that can `throw "Not ready"` violates D6 — never replicate.

**Errors:** extend `IdentityError` (currently 2 variants, `identity.rs:50-53`) with `Timeout(operation, ms)`, `SignerPubkeyMismatch`, `SignerModifiedEvent`, `NotReady`, `Unsupported(method)`. Per D6, these become toast state at the FFI; never `panic!`.

### 1.2 Account/Session type — fields, mutable vs immutable

NDK's `NDKSession` (`sessions/src/types.ts:7-51`) mixes identity, derived state, and live subscription handles. Applesauce's `IAccount` (`accounts/src/types.ts:26-42`) is just identity + signer + metadata.

**Recommendation:** **adopt applesauce's narrow account shape; derived state lives in domain stores.**

```rust
// Persistent / serializable:
pub struct AccountRecord<D: IdentityDescriptor> {
    pub id: AccountId,           // ULID; distinct from pubkey (supports multiple accounts for one pubkey)
    pub pubkey: Hexpubkey,
    pub namespace: &'static str, // IdentityModule::NAMESPACE
    pub descriptor: D,           // module-specific
    pub display_name: Option<String>,
    pub last_active: u64,
    pub created_at: u64,
}

// Session-only (in-memory, projected into AppState.session):
pub struct ActiveAccount {
    pub id: AccountId,
    pub pubkey: Hexpubkey,
    pub capabilities: SignerCapabilities, // {can_sign, can_nip04, can_nip44, is_readonly}
    pub display_name: Option<String>,
}
```

`AccountId` (ULID) distinct from `pubkey` — required because applesauce allows multiple accounts for one pubkey (`manager.ts:60-63`), and NMP must support "same nsec, different relay policy" or "same bunker user from two devices."

**Mutable vs immutable:**
- `AccountRecord.pubkey`, `id`, `namespace` are immutable post-creation.
- `descriptor`, `display_name`, `last_active` are mutable via dedicated actor messages.
- Follow set, mute list, relay list **are not on the account** — query the domain stores by `pubkey`.

### 1.3 AccountManager API

NDK's `NDKSessionManager` (manager.ts) and applesauce's `AccountManager` (manager.ts) converge on the same surface. NMP's equivalent is the actor; the public API is the `AppAction` variants in `docs/product-spec/api-surface.md:104-110`:

```rust
AddAccountPrivateKey { nsec_or_ncryptsec: String, passphrase: Option<String> },
AddAccountBunker { connect_uri: String },
AddAccountExternal { kind: ExternalSignerKind },
ActivateAccount { pubkey: String },
RemoveAccount { pubkey: String, wipe: bool },
```

**Recommendation:** these five intents are the right primitives. Add three more for M6 completeness:

```rust
GenerateNewAccount { display_name: Option<String>, passphrase: String }, // create + nip-49 encrypt + publish kind:0/10002 if requested
ListAccounts,                                                            // returns Vec<AccountRecord> in AppState
RenameAccount { pubkey: String, display_name: String },                  // touches metadata only
```

`ListAccounts` is implicit (always in `AppState.sessions.accounts`); `RenameAccount` is metadata-only and writes through `KeyringCapability` (since the keyring already holds the account record).

**Enumeration**: read-only via `AppState.session.accounts: Vec<AccountRecord>`. Reactive: the actor emits `AppUpdate::FullState` (or a `SessionDelta` variant) on add/remove/activate.

**Switch order invariant** (from NDK `store.ts:279-329`): on `ActivateAccount`, the actor must (1) bind the signer, (2) rebuild mute/relay filters, (3) update active_pubkey, (4) emit ONE `AppUpdate::FullState`. Never partial updates.

### 1.4 Reactivity model

NDK uses zustand subscribe (`manager.ts:406`); applesauce uses rxjs `BehaviorSubject` (`manager.ts:10-15`). Both expose a reactive primitive directly to consumers.

**Recommendation:** **neither.** NMP's reactivity is the `AppUpdate` stream over UniFFI (`api-surface.md:170-183`). Views subscribe to the stream; the active account is a field in `AppState.session`. There is no separate `manager.active$` observable. This matches D8's "snapshots by default."

Per-view reactivity (e.g., a SessionSwitcher view) reads `AppState.session.accounts` and `AppState.session.active_pubkey`; re-renders when those change. The framework handles the diff; the view code is declarative.

### 1.5 Persistence boundary

NDK: persists `{pubkey, signerPayload, lastActive, preferences}` per session + global `activePubkey` (`types.ts:135-140` + `local-storage.ts:13-26`). Everything else derives from the cache.

Applesauce: persists `{id, type, pubkey, signer, metadata}` per account (`types.ts:12-23`). No active marker. No derived state.

**Recommendation for NMP:**

| Lives in LMDB (`nmp-core` event store / domain stores) | Lives in `KeyringCapability` | In-memory only |
| --- | --- | --- |
| All received events (kind:0, 3, 10000, 10001, 10002, etc.) | Encrypted `AccountRecord` per account (one keyring entry per `AccountId`) | `ActiveAccount` (derived on activate) |
| `FollowSet`, `MuteSet`, `RelayList` per pubkey (derived domain stores) | Encrypted bunker `local_signer` private keys (with the bunker descriptor) | Subscription handles per active account |
| Last-active timestamp per account (so resume picks "most recent") | NIP-49-encrypted nsec bytes | Action ledger entries (already specified in M6) |

Two things go in the keyring, NOT LMDB: (a) encrypted nsec for human accounts, (b) the NIP-46 `local_signer` private key per bunker connection. The keyring blob is opaque to `KeyringCapability` per `api-surface.md:198-203`; NMP serializes the `AccountRecord` (including the bunker `local_signer.private_key`) into bytes, then asks the keyring to store them.

LMDB stores the *index* — `AccountId → keyring_key` — plus the public account record (display name, namespace, pubkey, last_active, created_at). The keyring stores the secret material.

**Why split:** the keyring's threat model (passcode-gated, hardware-backed where available) is different from LMDB's. LMDB is on-disk-encrypted at the OS level only.

### 1.6 kind:3 + kind:10002 auto-rewire

**NDK's mechanism** (`sessions/src/store.ts:417-444` + `:449-487`): on session start, subscribe to a fixed kind set for the authoring pubkey; per-kind handlers dispatch on `event.kind`, drop older-`created_at` events, rebuild derived sets, store the raw event.

**Applesauce's mechanism**: not implemented in the accounts package — the consumer subscribes via the EventStore and gets reactive results via rxjs query results.

**NMP's preferred mechanism**: a kernel-owned **account-bootstrap planner** that, on `ActivateAccount`, opens a fixed subscription `{kinds: [0, 3, 10000, 10001, 10002], authors: [pubkey]}` against the user's outbox relays (or fallback). Per-kind event handlers update the corresponding domain store (`ProfileStore`, `FollowSetStore`, `MuteSetStore`, `RelayListStore`). The subscription is owned by the account session and torn down on `RemoveAccount`/`ActivateAccount(other)`.

The per-kind handler logic is mechanical (NDK got it right at `store.ts:492-611`); port the algorithm but write the data into domain stores instead of session fields. **D4 invariant:** the `FollowSetStore` is the sole writer of the `FollowSet`; views read from it.

### 1.7 bunker:// flow

NDK's parser (`core/src/signers/nip46/index.ts:204-215`) and applesauce's parser (`helpers/nostr-connect.ts:86-98`) agree on the URL shape. Applesauce additionally validates `isHexKey(remote)`.

**Recommendation:** implement in `nmp-core` (not in a separate module) since the URL format is part of the kernel-facing `IdentityDescriptor`:

```rust
pub struct BunkerUri {
    pub remote_pubkey: [u8; 32],  // validated hex
    pub relays: Vec<String>,       // ≥1
    pub secret: Option<String>,
}

impl BunkerUri {
    pub fn parse(uri: &str) -> Result<Self, IdentityError> { /* ... */ }
}
```

RPC setup mirrors `nip46/index.ts:315-368`: open a long-lived subscription on `{kinds: [24133], "#p": [local_signer.pubkey]}`, send `connect [user_pubkey, secret?]` request, await `"ack"`, issue `get_public_key`, then optional `switch_relays` with 5s timeout (`index.ts:375-411`).

**Reconnect strategy:** on app restart, re-deserialize from keyring (which includes `local_signer.private_key` + `relay_urls` + `bunker_pubkey` + `user_pubkey`), re-open the subscription, but **do NOT re-issue `connect`** — the bunker already trusts this local signer (`index.ts:592-607`). This is the key insight: bunker pairing is per-local-signer-key, not per-session.

**On websocket churn:** the subscription must auto-reconnect (NDK uses NDK's normal pool retry; applesauce uses `repeat() + retry()`, `nostr-connect-signer.ts:153-161`). NMP's relay pool already retries; the NIP-46 client should ride that machinery.

**Secret parameter handling:** the secret is **only** valid for the initial `connect` call; it is NOT a long-term token. After ack, the bunker authorizes by `local_signer.pubkey`. The secret can be persisted in `BunkerUri.secret` (some bunkers re-validate on every reconnect; AlbyHub does this) — NDK persists it (`nip46/index.ts:547`), applesauce does too (`nostr-connect-account.ts:24-29`). **NMP: persist it.**

### 1.8 NIP-07 wasm wrapping

NDK (`core/src/signers/nip07/index.ts`): polls for `window.nostr` with bounded timeout (`waitForExtension`, line 222-246); queues encryption calls and retries on "call already executing" (line 169-220); `toPayload` is `{"type":"nip07","payload":""}` (line 253-258).

Applesauce (`packages/signers/src/signers/extension-signer.ts`, 37 LOC): caches pubkey after first call (line 17); no queue at the signer level (uses account-level queue).

**Recommendation for NMP** (deferred to M15 cross-platform, not M6 critical path):

- Wasm-side bridge exposes a `WebSignerCapability` (analog to `ExternalSignerCapability` in `api-surface.md:212-215`) that calls `window.nostr.*` from a JS shim.
- The Rust side treats this exactly like `IdentityScopeKind::ExternalSigner` — no special case.
- Pubkey caching happens at the wasm bridge layer (one round-trip per session).
- Encryption serialization happens at the per-account queue (actor inbox), not in the bridge.
- Timeout: 5s for any single call; surface `IdentityError::Timeout("extension call", 5000)`.

**Not in M6 scope.** Web is M15. But mention the parallel here so the M6 design doesn't accidentally close the door.

### 1.9 Multi-account UX surface for M6 vs deferred

M6 deliverables per `docs/plan/m6-signers-write.md:11-19`:

| In scope for M6 | Deferred (M7+) |
| --- | --- |
| Paste nsec (raw + ncryptsec) | `nostrconnect://` (client-initiated reverse handshake) |
| Generate new nsec + NIP-49 encrypt + keyring store | Multi-account switcher UI (M8 per `docs/plan/m8-multi-account.md`) |
| Paste `bunker://` URL → live signer | Hardware signers (Coldcard, Krux, etc.) — analogous to applesauce's SerialPortSigner |
| `KeychainCapability` real impl on iOS | NIP-07 web extension (M15) |
| Active account binding (one at a time) | Account metadata (color, display name editing) |
| Action ledger w/ atomic publish + store | Account-rotation / signer migration UX |
| Compose flow on iOS | NIP-55 / Amber-style iOS-app deeplink signers |

**Critical M6 invariant:** one active account at a time is fine. Multi-account *visibility* (showing other accounts in a switcher) is M8. **But** the data model must already support multiple accounts in the keyring + LMDB so M8 is purely a UI/UX milestone — don't bake "single account" into any schema or actor message.

## 2. Side-by-side scorecard

| Dimension | NDK | applesauce | NMP recommendation |
| --- | --- | --- | --- |
| Signer trait size | 9 required + 1 optional method | 2 required + 2 optional namespaces | 3 required + 2 optional (matches applesauce) |
| Return shape for sign | sig string only | full signed event | full signed event (already in `identity.rs:65-69`) |
| Sync `pubkey` accessor | yes, throws if not ready | no, async only | async only |
| Type-tagged signer registry | yes (`registry.ts:7-14`) | yes (`manager.ts:33-42`) | `IdentityModule::NAMESPACE` (already in `identity.rs:9`) |
| Per-account queue | no (NIP-07 has its own encrypt queue) | yes, mandatory (`account.ts:137-187`) | yes, per-account actor inbox serializes |
| Signer-mismatch post-conditions | no | yes (`account.ts:115-128`) | **yes, mandatory** — add to NMP wrapper |
| Bunker URL parser | regex hostname/pathname (no hex check) | URL + isHexKey validation | strict hex validation |
| `switch_relays` NIP-46 ext | yes, 5s timeout (`nip46/index.ts:375-411`) | yes, async retry (`nostr-connect-signer.ts:413-430`) | yes, 5s timeout |
| Reactive active-account | zustand store subscription | rxjs BehaviorSubject | `AppUpdate::FullState`/`SessionDelta` only |
| Persist active marker | yes | no | yes (UX-friendly) |
| Auto-save | debounced 500ms | none (explicit `toJSON`) | debounced via actor tick |
| Derived state location | on session struct | not in account package | own domain stores |
| Storage adapter trait | yes (`SessionStorage`) | no | `KeyringCapability` + LMDB are NMP's "adapters" |
| `replaceAccount` (signer rotation) | logout+login | explicit method | dedicated `RotateAccountSigner` action; aborts in-flight |
| Read-only fallback on signer-restore-fail | yes (`persistence-manager.ts:75-88`) | implicit via ReadonlySigner | yes, with toast (D6) |

## 3. Doctrine sanity-check

| Doctrine | Risk vector | Mitigation |
| --- | --- | --- |
| D0 (no app nouns in kernel) | "HumanAccount" / "Bunker" naming risks app-coupling | These are `IdentityModule` namespaces, not core enum variants. The kernel only knows `IdentityScopeKind::{HumanAccount, AppLocal, ExternalSigner, Ephemeral}` (`identity.rs:27-32`) — that's the right abstraction level. |
| D1 (best-effort rendering) | Account display-name takes a moment to load on switch | `ActiveAccount.display_name: Option<String>` is a placeholder field; view payload renders pubkey-truncation until loaded. |
| D2 (negentropy first) | Account-bootstrap subscriptions are REQ-based | First fetch of kind:0/3/10002 is REQ; subsequent reconciliation can use NIP-77 (M7+). |
| D3 (outbox auto) | Bootstrap subscription needs to know the user's outbox before having read kind:10002 | Bootstrap goes to default-relay list on first activation; rewires to outbox once kind:10002 lands. **Same idempotent re-compile pattern as views.** |
| D4 (single writer per fact) | NDK's session struct holds derived state | NMP splits: domain stores own derived state; account record owns identity only. |
| D5 (snapshots bounded by what's open) | Per-account `walletEnabled` opt-in is the right shape | NDK already does this — copy verbatim (`sessions/src/manager.ts:322-394`). |
| D6 (errors never cross FFI) | `IdentityError` is too narrow today | Extend per §1.1. Every error becomes a toast field. |
| D7 (capabilities report) | `KeychainCapability::store` is one of the most "decision-shaped" capabilities | The capability stores opaque bytes; NMP decides what to encrypt and what key derivation to use. Per `api-surface.md:198-203` this is already correct. |
| D8 (reactivity contract) | Account-bootstrap subscriptions emit derived-store deltas | Per-domain-store reverse-index participation is the existing pattern — account bootstrap is just another producer. ≤60Hz cap applies uniformly. |

## 4. Files-to-touch checklist for #43

If acting on this synthesis, the M6 impl agent's files-to-touch is approximately:

- `crates/nmp-core/src/substrate/identity.rs` — extend `IdentityError`; add `Nip04Capability`/`Nip44Capability` optional traits; add signer-wrapper with post-conditions.
- `crates/nmp-core/src/identity/{registry,bunker_uri,nip49}.rs` (new) — type-tagged dispatch, bunker URL parser, NIP-49 encrypt/decrypt helpers.
- `crates/nmp-modules/nmp-identity-human/` (new) — `HumanAccount` module (nsec + ncryptsec).
- `crates/nmp-modules/nmp-identity-bunker/` (new) — NIP-46 client implementation; rides relay pool.
- `crates/nmp-core/src/keyring/` — `KeyringCapability` integration with account-record serde + opaque blob.
- `crates/nmp-core/src/app/state.rs` — add `session: SessionState { accounts: Vec<AccountRecord>, active_pubkey: Option<Hexpubkey> }`.
- `crates/nmp-core/src/actions/account.rs` (new) — handlers for `AddAccount*`, `ActivateAccount`, `RemoveAccount`, `GenerateNewAccount`.
- iOS side: implement `KeyringCapability` for real (Security framework, app-private access group); login screen with three flows.

## 5. Single biggest risk to avoid

**The biggest mistake NMP can make is treating "session" and "account" as separate concepts.** Both NDK (`NDKSession`) and the React-side store create a per-pubkey blob that holds *both* identity and derived state, then bolt on a separate signer map. This makes activation atomicity hard (NDK explicitly comments on race conditions at `store.ts:279-281`).

NMP's actor model gives this for free: one `AccountId` is the unit of everything. The "session" is just "this account is currently active" — a single `Option<AccountId>` in `AppState`. Derived state lives in domain stores keyed by pubkey, queryable for any account whether active or not.

Adopt applesauce's `IAccount` as the mental model (`packages/accounts/src/types.ts:26-42`), not NDK's `NDKSession`. The fewer fields on the account record, the smaller the surface for race conditions.
