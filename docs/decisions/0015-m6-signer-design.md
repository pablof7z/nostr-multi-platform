# ADR-0015 — M6 Signer trait, IdentityModule, and crate boundary

**Date:** 2026-05-18
**Status:** Accepted (M6 task #43, written before sessions-research synthesis landed; subject to re-evaluation when `docs/research/sessions/synthesis.md` is published).
**Doctrines invoked:** D0 (no app nouns in `nmp-core`), D3 (outbox automatic), D4 (single writer per fact), D6 (errors never cross FFI), D7 (capabilities report; never decide), D8 (reactivity contract).

## Context

M6 (per `docs/plan/m6-signers-write.md`) requires three signer kinds: local nsec, NIP-46 bunker, and NIP-07 browser extension. Task #43 additionally requires a multi-account `AccountManager` and kind:3 rewire observer — work that strictly belongs to M8 but is folded into M6 here because the kernel reactivity for active-account changes needs to be in place before the M5 NIP-42 auth path can route challenges to the *right* signer (the same active account).

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

### kind:3 auto-rewire

When the active account flips:
1. Kernel closes all subscriptions tagged with the prior active account.
2. Kernel re-derives interest set against the new active account's `follows` (kind:3) and `relayList` (kind:10002).
3. New subscriptions open via the existing planner.

Implementation: `AccountManager` exposes an `ActiveChangeObserver` callback hook. This commit adds `Kind3RewireObserver`, which stages active-account rewire requests for a future kernel integration to drain and translate into kind:3 / kind:10002 subscription rebuilds.

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

## Synthesis reconciliation (post-`docs/research/sessions/synthesis.md`)

The synthesis landed mid-implementation (commit `de9e7b4`).  Where the
synthesis recommendations align with the design above, no change is needed.
Where they diverge, this section names the divergence + rationale.

### Adopted from synthesis

1. **Id-precompute post-condition** (§1.1c) — `AccountManager::verify_signer`
   now pre-computes the canonical event id via `nostr::EventBuilder` and
   compares to `signed.id`.  Catches *any* signer-side mutation
   (content, tags, kind, created_at, tag ordering).  Test `t3b` exercises a
   tag-adding mutating signer and confirms the check fires.

2. **Bunker pubkey strict-hex validation** (§1.7) — `parse_bunker_uri`
   rejects non-hex / wrong-length pubkeys (matches applesauce, stricter than
   NDK).  Fuzz harness `bunker_uri_fuzz.rs` enforces this on 1000+ inputs.

3. **`bunker://` secret persisted** (§1.7) — `Nip46Payload.secret: Option<String>`.

4. **Cached remote-user pubkey on `Nip46Payload`** (§1.7) — enables
   synchronous `pubkey()` on restore without a re-handshake (also matches
   applesauce `0867a502`).

5. **Sync `pubkey()` despite synthesis pushback** — synthesis §1.1c argues
   "async only" for D6.  D6 is about *errors not crossing FFI as exceptions*,
   not sync-vs-async.  A synchronous, infallible `pubkey()` (only callable
   after construction completes the handshake) carries no error to cross FFI
   and is the only design that satisfies D8's hot-path constraint
   (per-event allocation MUST be linear in active-view count, not in
   per-active-account async-state-machine size).  Applesauce caches the
   pubkey for exactly this reason (`extension-signer.ts` commit `0867a502`).
   See "Divergence from synthesis" below.

### Adopted with adjustment

6. **`AccountId` == pubkey (one account per pubkey)** — PD-004 (resolved)
   made `IdentityId = pubkey_hex` **permanent**; the ULID rekey is
   **cancelled** and the applesauce dual-account-per-pubkey model is
   rejected.  Same nsec = same account: `AccountManager::add` is an
   idempotent no-op for a known pubkey (at most a future relay-policy
   merge), never a second slot.

7. **Extend `IdentityError`** (§1.1) — `nmp-core::substrate::identity::IdentityError`
   currently has 2 variants.  Our `SignerError` (in `nmp-signers`) already
   covers `Timeout`, `NotReady`, `Unsupported`, `Mismatch` — the additional
   names the synthesis wants (`SignerPubkeyMismatch`, `SignerModifiedEvent`)
   are captured by `SignerError::Mismatch` with descriptive messages.  The
   `IdentityError` extension in `nmp-core` is a separate task because it
   touches the existing `IdentityModule` trait and would expand the change
   surface beyond this commit.  **Filed as M6 follow-up**: ext-identityerror.

### Divergence from synthesis (with rationale)

- **Async trait surface** — synthesis §1.1c mandates "all methods async,
  return BoxFuture."  We use sync `pubkey()` + `SignerOp<T>` thunks for
  fallible ops.  Rationale: the kernel actor is `std::sync::mpsc` based; we
  do not have Tokio in the workspace, and adding it for the signer path
  would change the runtime model of every actor message.  `SignerOp<T>`
  satisfies the synthesis intent (no blocking the actor on a remote
  round-trip) without the runtime cost.  If a future signer kind genuinely
  needs `async fn` ergonomics, we add an `AsyncSignerAdapter` rather than
  retrofit the whole trait.

- **Bunker parser lives in `nmp-signers`, not `nmp-core`** — synthesis §1.7
  recommends `nmp-core`.  Per D0, signer-onboarding is policy/capability —
  the parser belongs with the implementation it serves.  Both placements
  work; the parser is a pure function and re-locating it is a 5-minute
  mechanical move if a reviewer disagrees.  Flagging this for the codex
  post-merge review.

- **No `IAccount` shape adoption beyond AccountManager** — synthesis §1.2
  proposes a full `AccountRecord { id, pubkey, namespace, descriptor,
  display_name, last_active, created_at }` for persistence.  The current
  `AccountManager` keeps signers + active in memory only; persistence is
  intentionally deferred to a separate commit because it touches
  `KeyringCapability` (which doesn't have a real impl yet) and the LMDB
  schema (M3 phase 2).  The current minimal model + `SignerPayload`
  serialization is enough for the M6 demo (paste nsec + paste bunker +
  generate-new flow on iOS with in-memory + ephemeral keychain stub) and
  forward-compatible with the `AccountRecord` shape — adding ULID id +
  metadata fields is additive.

- **NIP-07 wasm-stub** — synthesis §1.8 explicitly defers web to M15.
  Our impl agrees and provides a compile-correct stub today so the
  identifying type + payload shape are already stable.

### Codex synthesis correction (post-`de9e7b4`, addressed here)

The coordinator forwarded a codex correction on `synthesis.md`:

> **AccountRecord MUST split into AccountPublic (LMDB) + AccountSecret (Keyring).**  Putting display_name + last_active in Keyring while ALSO writing them to LMDB violates D4.

This is forward-looking guidance for the *persistence layer*.  The current
landing has **no persistence yet** — `AccountManager` is purely in-memory.
Keyring + LMDB persistence is a separate task (depends on
`KeychainCapability` real impl + M3 LMDB schema extension, neither in
scope here).

When persistence lands the schema will be:

- **`AccountPublic`** in LMDB (`pubkey`, `display_name`, `last_active`,
  `created_at`, `signer_kind`, `signer_descriptor`).  LMDB is the sole
  writer of all mutable public metadata per D4.
- **`AccountSecret`** in Keyring (NIP-49-encrypted nsec OR bunker
  `local_signer.private_key` + bunker URI metadata).  Opaque to the
  capability; nmp-signers serialises the blob.
- Linked by `AccountId` (== `pubkey_hex`, permanent per PD-004).

The current `SignerPayload` is already shaped to slot into `AccountSecret`
as the opaque blob — it carries only secret-bearing fields, no display
metadata, no timestamps.  When the persistence task lands the split is
additive; no rewrite needed.

Other synthesis corrections from the same coordinator message
(account-bootstrap uses NIP-77 from day one; drop `ListAccounts`; defer
`RenameAccount` to M8; bunker-secret divergence noted in NDK direction)
are forward-looking and consistent with the current landing — no
current-commit change required.

### Open follow-ups (for orchestrator)

- ~~**PD-004 candidate**: switch `IdentityId` from `pubkey_hex` to ULID
  before M8.~~  **Resolved (PD-004): `pubkey_hex` is permanent; ULID rekey
  cancelled — one account per pubkey, applesauce dual-account model
  rejected.**
- **`IdentityError` extension** in `nmp-core` to add per-synthesis-§1.1
  variants — one-file diff, deferred to keep this commit focused.
- **NIP-46 `switch_relays` extension** — synthesis §1.7 wants it day-one.
  Current `Nip46Transport::reconnect_hint()` is the placeholder; the kernel
  side will own the actual relay-pool re-bind in the same commit that wires
  the live RPC subscription.
- **Keyring persistence** — `LocalPayload::Raw(hex)` and
  `LocalPayload::Ncryptsec(s)` are storage-form-ready; the real iOS
  Keychain backend lands with `KeychainCapability` (M6 plan §1).

## Related

- ADR-0007 (diagnostics and non-Nostr domain data) — toast-style error surfacing.
- ADR-0009 (app-extension-kernel boundary) — `IdentityModule` is one of the canonical extension trait shapes.
- `docs/plan/m5-nip42.md` — auth challenge routing needs the active signer bridge.
- `docs/plan/m7-interaction-loop.md` — write-path actions consume the active signer to sign.
- `docs/plan/m8-multi-account.md` — multi-account UX builds on this manager.
