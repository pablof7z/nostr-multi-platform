# 11 — Sessions + signers + identity scopes (`nmp-signers`)

*Status: SHIPS · Audience: both*

> **Scope note.** This section is **M8-multi-account**: the runtime
> account roster + active-account switch + kind:3 rewire. It is *not*
> M8-subs (the subscription-lifecycle / relay-manager split) — that is
> [14 — relay manager](14-relay-manager.md). When you see "M8" in the
> orchestration log it covers both; here we mean only the account side.

## Why signers live outside the kernel

Per doctrine **D0**, `nmp-core` carries no identity material. Signers,
account state, and the bunker parser ship in the `nmp-signers` crate
(`crates/nmp-signers/src/lib.rs:1-37`). The kernel only knows the
scope kind via `IdentityScopeKind`
(`crates/nmp-core/src/substrate/identity.rs`); the concrete signer
backends are policy + capability bridges, not substrate. Rationale:
`docs/decisions/0015-m6-signer-design.md`.

`nmp-signers` supplies the signer implementations the kernel composes;
the kernel never imports a secret key, an `nsec`, or a `bunker://` URL.

## The `Signer` trait

`crates/nmp-signers/src/signers/traits.rs:110-133` defines a deliberately
small contract:

- `backend() -> SignerBackend` — `LocalKey` / `Nip46` / `Nip07` / `Custom`.
- `pubkey() -> PublicKey` — **synchronous and infallible after
  construction** (invariant 1). Constructors that need an async
  handshake (NIP-46) complete it *before* returning `Ok`; the
  pre-handshake form is a separate `Nip46SignerHandle` type.
- `sign(unsigned) -> SignerOp<SignedEvent>` — returns a thunk
  (`Ready` for local, `Pending(rx)` for remote). The signed event's
  embedded pubkey must equal `self.pubkey()` and its id must match the
  template (invariant 2 — the applesauce `SignerMismatchError`
  post-condition).
- `nip04()` / `nip44()` — `Option<&dyn ...>`; callers **must** check
  (invariant 3). A `LocalKeySigner` returns `Some`; a non-wasm
  `Nip07Signer` returns `Some` but every op yields `Unsupported`.
- `to_payload() -> SignerPayload` — serialize for persistence;
  round-trips via the kind-specific constructor.

`SignerError` (`traits.rs:33-57`) is **string-typed by design** — per
**D6** it is Rust-internal flow control only and is mapped to
`toast: Option<String>` at the FFI boundary. It never crosses FFI as an
exception. Variants: `NotReady`, `Unsupported`, `Rejected`, `Mismatch`,
`Timeout`, `SignatureVerificationFailed`, `Backend`.

## Signer-kind comparison

| Kind | Construct | Latency | Security | UX | Capability deps |
|---|---|---|---|---|---|
| `LocalKeySigner` | `generate` / `from_nsec` / `from_ncryptsec` / `from_secret_hex` (`signers/local.rs:43-104`) | `SignerOp::Ready` — synchronous, microseconds | Raw key in process; NIP-49-at-rest if constructed from `ncryptsec` (`local.rs:71-83`, `log_n` default 16, never < 14 for real keys) | No prompt; instant | None (pure crypto) |
| `Nip46Signer` | handshake → `Nip46SignerHandle::complete` (`signers/nip46/mod.rs:133-146`) | `SignerOp::Pending(rx)` — relay round-trip, seconds; default post-condition timeout 5s (`identity/manager.rs:104`) | Key never leaves remote bunker; local ephemeral key only | Remote-approve prompt per op | `Nip46Transport` injected by kernel (D7 — kernel owns relays) |
| `Nip07Signer` | `from_cached_pubkey` / `from_payload` (`signers/nip07.rs:53-80`) | Browser `window.nostr.*` on wasm; **non-wasm = `Unsupported`** | Key in extension; pubkey cached so `pubkey()` is sync | Extension dialog per op | wasm target + `feature = "wasm"` (`nip07.rs:83-85`) |
| Amber NIP-55 (future) | via `CapabilityModule` (`SignerBackend::Custom`) | Android intent round-trip | Key in Amber app | System intent prompt | `ExternalSigner` capability — see [16 — capabilities](16-capabilities.md) |

Every backend's `pubkey()` is sync because construction is gated: NIP-46
caches the remote user pubkey after the first handshake
(`nip46/mod.rs:23-24`), NIP-07 cannot exist without a cached pubkey
(`nip07.rs:42-45`) — there is **no panic path** (D6).

## `AccountManager` — synchronous active-switch

`crates/nmp-signers/src/identity/manager.rs:68-78` holds the roster:
`accounts: HashMap<IdentityId, Arc<dyn Signer>>` (id = hex pubkey),
insertion-ordered `order`, an `active: Option<IdentityId>`, and
observers.

### Add-time post-condition

`add()` (`manager.rs:118-128`) runs `verify_signer`
(`manager.rs:236-276`): it pre-computes the canonical event id for a
fixed kind:1 probe template, calls `signer.sign(probe)`, then refuses
the account if the returned pubkey ≠ claimed **or** the returned id ≠
expected. This catches malicious/buggy signers that mutate the event
before signing.
`add_unverified()` (`manager.rs:134-143`) is the restore-path escape for
signers that cannot sign eagerly (NIP-46 with no connected transport) —
callers **must** run their own verification before relying on it.

### Switch-account: action → state

`switch_active(id)` (`manager.rs:150-173`) is the whole story. It is a
*flip*, not a tear-down:

```text
SwitchAccount(id)
   │
   ├─ id absent?            → Err(NotFound)            [no observer]
   ├─ id == active?         → Ok(())  no-op            [no observer]
   │
   └─ valid switch:
        previous = active.take()
        active   = Some(id)            ◄── flip is SYNCHRONOUS, before observers
        ┌────────────────────────────────────────────────┐
        │ for obs in observers (registration order):      │
        │   obs.on_active_change(ActiveChangeEvent {       │
        │     previous, current: Some(id),                 │
        │     current_pubkey: Some(pk) })                  │
        └────────────────────────────────────────────────┘
                          │
                          ▼
        Kind3RewireObserver buffers Kind3RewireEvent
                          │
            (kernel drains on actor tick)
                          ▼
        planner tears down old "your follows" sub,
        rebuilds against new kind:3 + kind:10002
```

`remove(active_id)` (`manager.rs:183-203`) clears `active` *before*
firing observers, then emits one event with `current: None` /
`current_pubkey: None` — the kind:3 / kind:10002 teardown +
`FullState { active_account: None }` signal.

Observers run **on the actor thread** (D4 — single writer per fact) and
must not block (`manager.rs:60-65`).

### kind:3 auto-rewire

`crates/nmp-signers/src/identity/rewire.rs:34-70`: `Kind3RewireObserver`
is registered as an `ActiveChangeObserver`. On every transition it
buffers a `Kind3RewireEvent { previous, current }`; the kernel drains it
each tick. **`nmp-signers` only signals** — the actual subscription
teardown/rebuild is the planner's job because the planner owns the relay
pool (D7 capability-vs-policy split). `current: None` means "tear down
the kind:3 subscription, emit `FullState` with no active account."

This is framework-magic contract bullet C-sessions: the app gets
follow-set rewire for free on every account switch.

## `IdentityScopeKind` decision tree

`crates/nmp-core/src/substrate/identity.rs:26-32` —
`HumanAccount` / `AppLocal` / `ExternalSigner` / `Ephemeral`:

```text
Does a human own this key and expect it to persist + sync kind:3/10002?
├─ yes → is the key held by a separate app/device?
│         ├─ yes (bunker / Amber / extension) → ExternalSigner
│         └─ no  (nsec / ncryptsec in our store) → HumanAccount
└─ no  → is it a per-install key the app generated for itself?
          ├─ yes (app-local agent, device key, app-managed npub) → AppLocal
          └─ no  (one-shot, throwaway, never persisted) → Ephemeral
```

Anti: never give an app-local automation a `HumanAccount` scope — that
makes the kernel sync a follow-list / relay-list for a key that has no
human behind it. App-local agents are `AppLocal`; one-shot signers are
`Ephemeral`.

## `parse_bunker_uri` worked example

`crates/nmp-signers/src/bunker/parser.rs:95-174`. Pure function, fuzz
target, hard 4 KiB cap (`MAX_BUNKER_URI_LEN`, `parser.rs:9`).

Input:

```
bunker://b2c3...64hex?relay=wss%3A%2F%2Frelay.example&relay=wss://r2.example&secret=abc&perms=sign_event:1,nip44_encrypt
```

Parse steps:

1. Empty? no. Length ≤ 4096? yes (`parser.rs:96-101`).
2. Case-insensitive `bunker://` prefix check on the trimmed input
   (`parser.rs:106-113`) — `Bunker://` and leading whitespace are
   rejected/normalised deterministically; `url::Url::parse` is *not*
   used for the scheme step.
3. Split host vs query at first `?` (`parser.rs:117-120`); strip a
   trailing `/` from the pubkey.
4. `normalise_pubkey` (`parser.rs:180-193`): require exactly 64
   ASCII-hex chars, lowercase. Else `InvalidPubkey`.
5. Walk `&`-split pairs, percent-decode each (`parser.rs:130-161`):
   - `relay=` → `validate_relay_url` requires `ws://`/`wss://` +
     `url::Url::parse` (`parser.rs:195-208`); deduplicated.
     `wss%3A%2F%2F...` decodes to `wss://relay.example`.
   - `secret=` → `Some("abc")`.
   - `perms=` (alias `permissions=`) → `Some("sign_event:1,nip44_encrypt")`.
   - unknown keys → preserved in `extra` for round-trip.
6. `relays.is_empty()` → `NoRelay` error. Here two relays survive →
   `Ok(BunkerUri { remote_pubkey_hex, relays, secret, permissions, extra })`.

`BunkerUri` round-trips via its `Display` impl (`parser.rs:61-92`).
`Nip46SignerHandle::from_bunker_uri` (`nip46/mod.rs:99-105`) wraps the
parsed URI + a fresh local ephemeral keypair; the kernel then drives the
`connect` / `get_public_key` RPC and calls `complete(transport, pubkey)`
to upgrade to a fully-connected `Nip46Signer`.

## Anti-patterns

1. **Account switch as tear-down/rebuild.** `switch_active` is a
   synchronous flip; it does *not* drop and re-create the actor or the
   store. Treating a switch as "log out, log in" loses cached content
   and breaks D1.
2. **`HumanAccount` scope for app-local agents.** Forces kernel kind:3 /
   kind:10002 sync for a key with no human. Use `AppLocal`/`Ephemeral`.
3. **Signer calls direct from UI.** The signer is driven by the actor
   (D4). UI dispatches an action; it never holds an `Arc<dyn Signer>`
   or calls `sign()` itself.
4. **Skipping the add-time post-condition.** Using `add_unverified` on
   a path that *can* sign eagerly, then publishing — an ADR-0015
   post-condition violation that lets a mutating signer through.
5. **"Is logged in?" UI guards that withhold cached content.** A
   missing/locked signer must not blank already-cached events (D1).
   Gate *write* actions, never *reads*.
6. **Re-handshaking NIP-46/NIP-07 on every `pubkey()`.** The pubkey is
   cached at construction; `pubkey()` is sync and free. Treating it as
   async is an API misuse.

## See also

- [10 — Outbox routing (NIP-65)](10-outbox-routing.md)
- [12 — Publishing + the publish engine](12-publish-and-ledger.md)
- [16 — Capabilities (D7)](16-capabilities.md)
