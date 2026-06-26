# ADR-0015 — M6 Signer trait, IdentityModule, and crate boundary

**Date:** 2026-05-18
**Status:** Accepted (M6 task #43). The durable decisions below are live. The
provisional synthesis-reconciliation log and the kind:3 auto-rewire scaffolding
that originally accompanied this ADR have been removed — both were overtaken by
later work: signer persistence is now ADR-0050's scope, and active-account
rewire is handled by the `SwitchActive` actor command, not an observer.
`SignerOp` has since moved into the `nmp-signer-iface` crate so `nmp-core` can
name it without depending on `nmp-signers`.
**Doctrines invoked:** D0 (no app nouns in `nmp-core`), D3 (outbox automatic), D4 (single writer per fact), D6 (errors never cross FFI), D7 (capabilities report; never decide), D8 (reactivity contract).

## Context

M6 requires three signer kinds: local nsec, NIP-46 bunker, and NIP-07 browser extension. Task #43 additionally requires a multi-account `AccountManager` and kind:3 rewire observer — work that strictly belongs to M8 but is folded into M6 here because the kernel reactivity for active-account changes needs to be in place before the M5 NIP-42 auth path can route challenges to the *right* signer (the same active account).

Two upstream research families inform the design:

- **NDK** (`docs/research/ndk/signers.md`, `wot-and-sessions.md`): `NDKSigner` is an async interface with NIP-04/44 encrypt/decrypt and a serializable payload. Sessions are a separate store keyed by pubkey; switching sessions sets the signer **synchronously** before flipping `activePubkey` (race fix `a14c7a78`).
- **Applesauce** (`docs/research/applesauce/signers.md`): `ISigner` is a 4-method contract with `nip04`/`nip44` as **optional** namespaces. Accounts wrap signers with a per-account serial queue and `SignerMismatchError` post-conditions.

`rust-nostr` 0.44 provides a `NostrSigner` trait of its own with mandatory NIP-04/44 methods (returning `BoxedFuture`). It is acceptable to adapt to but its mandatory NIP-04/44 surface conflicts with applesauce's correct optional-namespace shape, and its `BoxedFuture` choice forces async-runtime adoption inside the kernel — which today runs `std::sync::mpsc` actors with no Tokio.

## Decision

### Crate boundary (D0)

Signers live in a **new sibling crate `nmp-signers`**, not in `nmp-core`. Per D0, `nmp-core` must not grow app nouns; identity/signer materials are policy + capability bridges, and the kernel should see them through the existing `IdentityModule` boundary when the integration lands.

`nmp-signers` depends on `nmp-core` (for `UnsignedEvent` / `SignedEvent`) and on `nostr = 0.44` (for NIP-49, NIP-04, NIP-44 primitives only — we do not adopt `nostr::NostrSigner` as our public trait).

### Signer trait

```rust
pub trait Signer: Send + Sync + Debug {
    fn backend(&self) -> SignerBackend;
    fn pubkey(&self) -> PublicKey;     // sync after construction/restore
    fn sign(&self, unsigned: UnsignedEvent) -> SignerOp<SignedEvent>;
    fn nip04(&self) -> Option<&dyn Nip04>;
    fn nip44(&self) -> Option<&dyn Nip44>;
    fn to_payload(&self) -> SignerPayload;
}
```

Rationale:

1. **Synchronous `pubkey()`** — applesauce-style cached pubkey. NDK explicitly caches the result of the first `getPublicKey()` call (commit `0867a502`). Sync access satisfies D1 (no spinners on already-known facts) and D8 (the hot path never awaits to know who is active).
2. **`SignerOp<T>`** is our own non-`BoxedFuture` thunk type: a value the actor can poll, cancel, and serialize for retry. Maps to a `oneshot::Receiver`-style channel; the kernel pumps it on its own thread (D7 — capability reports back to policy). Avoids forcing Tokio into the kernel.
3. **Optional `nip04`/`nip44`** namespaces (applesauce shape) — Local signer implements both; Nip07 returns whatever the extension exposes; Nip46 negotiates and may return both, one, or neither.
4. **`SignerPayload`** is a stable, serializable type (`{kind: "local"|"nip46"|"nip07", body: ...}`); intended to be stored by the account persistence layer.

### Three impls (M6)

- `LocalKeySigner` (in-memory + NIP-49 encryption at rest via `nostr::nips::nip49`). Generates fresh keys for the create-new-nsec flow. `to_payload()` returns NIP-49 ncryptsec when a password is set, raw hex otherwise (guardrail: warn if exporting raw).
- `Nip46Signer` (bunker:// remote signer). Owns its own remote-pubkey, local ephemeral signing key, relay set, and pending RPC map. `sign()` produces a `SignerOp` that resolves when the remote responds; timeout configurable. Reconnect uses repeat-on-failure semantics per applesauce commit `e6d5613b`.
- `Nip07Signer` (browser extension). Stub trait impl + compile-error wall behind `feature = "wasm"`. The wasm target is not yet wired in this workspace; the trait shape and serialization (`{kind: "nip07"}`) are in place so the wasm-target follow-up can drop in `window.nostr.*` bindings without an API change.

### IdentityModule (registration trait) vs `AccountManager` (runtime state)

`nmp-core::substrate::identity::IdentityModule` already exists as a **module-registration** trait — it declares a `NAMESPACE` and the per-module factory + sign hooks. It is the kernel-extension hook for "an app contributes an identity kind."

Runtime multi-account state — adding accounts, switching active, dispatching to the correct signer — is a separate concern that doesn't live in the kernel. It lives in `nmp-signers::identity::AccountManager`:

```rust
pub struct AccountManager { /* signers keyed by pubkey + active */ }

impl AccountManager {
    pub fn active(&self) -> Option<IdentityId>;
    pub fn accounts(&self) -> Vec<IdentityId>;
    pub fn signer_for(&self, id: &IdentityId) -> Option<Arc<dyn Signer>>;
    pub fn signer_active(&self) -> Option<Arc<dyn Signer>>;
    pub fn add(&mut self, signer: Arc<dyn Signer>) -> Result<IdentityId, AccountError>;
    pub fn switch_active(&mut self, id: &IdentityId) -> Result<(), AccountError>;
    pub fn remove(&mut self, id: &IdentityId) -> Result<(), AccountError>;
    pub fn observe(&mut self, observer: Arc<dyn ActiveChangeObserver>);
}
```

`switch_active` invariants (NDK race fixes):
1. New signer is installed **synchronously before** active flips, observable to any consumer that reads pubkey + signer in the same critical section.
2. Notifying observers is the last step; observers run on the actor thread, not the caller's.

`add_account` runs the applesauce `SignerMismatchError` post-condition: `sign(test_template).pubkey == signer.pubkey()` before the account is accepted. Catches malicious / buggy signers that mutate the event.

### Active-account rewire

When the active account flips, the kernel closes subscriptions tagged with the
prior account, re-derives the interest set against the new account's `follows`
(kind:3) and `relayList` (kind:10002), and opens new subscriptions via the
planner. `AccountManager` exposes an `ActiveChangeObserver` callback hook for
this. (As built, the rewire is driven directly by the `SwitchActive` actor
command in `nmp-core`; the original provisional `Kind3RewireObserver` staging
scaffold was deleted as dead production code.)

### bunker:// URL parsing

`parse_bunker_uri(&str) -> Result<BunkerUri, BunkerParseError>` is the canonical parser. Format per NIP-46:

```
bunker://<remote-pubkey-hex>?relay=<wss-url>&relay=<wss-url>&secret=<optional>
```

Validation rules (all checked):
- scheme must be `bunker`
- pubkey must be 64-hex
- at least one relay (URL-decoded, must parse as a ws/wss URL)
- optional `secret` carried through; `permissions` carried through; unknown query params preserved for round-trip
- empty / malformed / oversized URIs (>4 KiB) rejected fast

The parser is **the** target of the 1000-URI fuzz suite (`fuzz/bunker_uri.rs`), exercising both well-formed and malformed inputs.

### Reactivity (D8)

`AccountManager::switch_active` is intended to be invoked from one actor message. The kernel integration must batch the resulting subscription close/open into a single delta. View payloads scoped to the active account (e.g. "your follows timeline") should flip in one snapshot — no transient empty state, no double subscription, no per-event allocation beyond what the existing planner already amortizes.

### FFI (D6)

`Signer` and `IdentityModule` are NOT directly FFI-exposed. The FFI surface adds three opaque action variants only — `AddLocalAccount(nsec)`, `AddBunkerAccount(uri)`, `SwitchActive(id)`. All errors surface as toast strings on the next `AppState` emit. The signer trait is Rust-internal.

## Divergence from upstream research

- **No async trait** — we use a sync `pubkey()` + `SignerOp` for `sign/encrypt/decrypt`. Rationale: the kernel actor loop is not Tokio-based today, and the M6 demo doesn't justify pulling in an executor. If a future signer kind genuinely needs `async fn` ergonomics, we add an `AsyncSignerAdapter` rather than retrofit the whole trait.
- **No per-account queue inside `Signer`** — the actor is already the single serializer (D4 — single writer per fact). The queue applesauce builds is for browsers; in Rust the actor model gives us the same property for free.
- **No proxy-signer indirection** — the active signer is queried via `AccountManager::signer_active()` at the point of need. Less indirection; same correctness when the manager is owned by the actor.

## Trade-offs accepted

- **NIP-07 wasm impl is a stub** — full wasm bindings deferred. The trait + payload shape are stable; the wasm target lift can land later as a pure additive change.
- **NIP-46 reconnect/relay-switching** is not implemented in this commit. The 2026-05-18 scope adjustments call out NIP-46 reconnect as M6 work; the bunker:// **parsing** (the M6 first-class onboarding path) lands here, but the kernel-relay-pool integration for the long-lived 24133 subscription will follow when the live NIP-46 demo is wired.
- **`KeychainCapability` is not wired here** — M6 plan calls for real iOS Keychain via `keyring-rs`; this commit only lands storage-shaped signer payloads.

## Durable outcomes

- **`IdentityId == pubkey_hex`, permanent (PD-004).** Same nsec = same account:
  `AccountManager::add` is an idempotent no-op for a known pubkey, never a
  second slot. The ULID rekey and the applesauce dual-account-per-pubkey model
  are rejected.
- **Signer persistence is out of scope for this ADR.** When this ADR landed
  `AccountManager` was in-memory only; the durable signer-secret / public-record
  split and the keyring/LMDB storage schema are owned by ADR-0050 and the
  capability-port work, not here. `SignerPayload` is shaped to slot into an
  opaque secret blob (secret-bearing fields only, no display metadata).
- **Sync `pubkey()` + `SignerOp` thunk, not an async trait.** The kernel actor
  is `std::sync::mpsc` based with no Tokio; `SignerOp` lets remote signers
  resolve without blocking the actor and without pulling an executor into the
  kernel. `SignerOp` now lives in `nmp-signer-iface` so `nmp-core` can name it
  across the D0 boundary.

## Related

- ADR-0007 (diagnostics and non-Nostr domain data) — toast-style error surfacing.
- ADR-0009 (app-extension-kernel boundary) — `IdentityModule` is one of the canonical extension trait shapes.
- `nmp-nip42` crate — NIP-42 auth challenge routing depends on the active signer bridge from this ADR.
- M7 interaction-loop — write-path actions (e.g. SendNote) consume the active signer to sign events.
- Multi-account UX (M8) — builds on `AccountManager` from this ADR.
