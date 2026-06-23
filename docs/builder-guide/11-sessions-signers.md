# 11 — Sessions, signers, and identity storage

*Status: SHIPS · Audience: both*

> **Scope note.** This section is **M8-multi-account**: the runtime
> account roster + active-account switch + kind:3 rewire. It is *not*
> M8-subs (the subscription-lifecycle / relay-manager split) — that is
> [14 — relay manager](14-relay-manager.md). When you see "M8" in the
> orchestration log it covers both; here we mean only the account side.

## Identity ownership boundary

Per doctrine **D4**, the actor is the single writer for account and signer
facts: active account, signer kind, app-managed signer roster, remote signer
payloads, and the account projection. Per **D7**, native shells only execute
capabilities such as Keychain/Keystore reads and writes; they do not own a
parallel session cache, active-account pointer, signer-kind record, retry
policy, or restore decision.

Local nsecs inevitably cross the FFI boundary when a user imports one, and
local-key accounts live in the actor-local identity runtime. That is allowed
only through explicit secret-bearing doorways such as `nmp_app_signin_nsec`,
`nmp_app_register_agent_nsec`, account-lifecycle import, and the
`KeyringCapability` restore path. Every Rust-side copy must be wrapped in
`Zeroizing` as soon as it is materialized, and normal snapshots, action
history, logs, projection payloads, and debug surfaces must never contain raw
nsecs or bearer tokens.

Remote signers follow the same ownership rule without local key material:
NIP-46/NIP-55 restore stores opaque signer payloads through the keyring
capability, then hands those payloads back to the Rust-owned restore hooks.
Native launches UI/OS affordances and returns raw approval or failure facts;
Rust decides whether the signer is active, usable, limited, or unavailable.

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
| `Nip46Signer` | handshake → `Nip46SignerHandle::complete` (`signers/nip46/mod.rs:133-146`) | `SignerOp::Pending(rx)` — relay round-trip, seconds | Key never leaves remote bunker; local ephemeral key only | Remote-approve prompt per op | `Nip46Transport` injected by kernel (D7 — kernel owns relays) |
| `Nip07Signer` | `from_cached_pubkey` / `from_payload` (`signers/nip07.rs:53-80`) | Browser `window.nostr.*` on wasm; **non-wasm = `Unsupported`** | Key in extension; pubkey cached so `pubkey()` is sync | Extension dialog per op | wasm target + `feature = "wasm"` (`nip07.rs:83-85`) |
| Amber NIP-55 (future) | via `CapabilityModule` (`SignerBackend::Custom`) | Android intent round-trip | Key in Amber app | System intent prompt | `ExternalSigner` capability — see [16 — capabilities](16-capabilities.md) |

Every backend's `pubkey()` is sync because construction is gated: NIP-46
caches the remote user pubkey after the first handshake
(`nip46/mod.rs:23-24`), NIP-07 cannot exist without a cached pubkey
(`nip07.rs:42-45`) — there is **no panic path** (D6).

## Session storage and restore

`crates/nmp-core/src/actor/session_persistence.rs` owns the current session
persistence policy. It writes only the minimum restore facts through
`KeyringCapability`:

- active account id: `nmp.identity.active.id`;
- active signer kind: `nmp.identity.active.kind`;
- local nsec: `nmp.identity.local_nsec.<pubkey>`;
- remote signer payload: `nmp.identity.remote_payload.<pubkey>`;
- app-managed local signer roster and per-signer nsecs.

The startup flow is deliberately ordered:

1. The host registers a keyring capability handler before any start/restore
   path that can read secure storage.
2. `Start` calls `restore_active_session` on the actor thread before the first
   snapshot. This cold-start read chain is synchronous by ADR-0040 because it
   runs once, before UI frames exist, and each read continuation decides the
   next restore step.
3. App-managed local signers restore first. They are signable by explicit
   pubkey but hidden from account projections and never become active.
4. The actor recalls the active signer kind and active account id. A miss
   means no restored active account; stale active pointers are forgotten.
5. A local signer restore recalls the local nsec, wraps it in `Zeroizing`, and
   routes through `AddSigner { LocalNsec, make_active: true }`.
6. A remote signer restore recalls the opaque payload and invokes the installed
   NIP-46 or NIP-55 restore hook. The actor stores signer state as data, not as
   an exception.
7. After a successful restore or active-account change, keyring writes go
   through the serialized capability worker. The actor never lets native retry,
   reorder, or reinterpret keyring outcomes.

For new lifecycle work, ADR-0059 is the target shape: `CreateLocal` and
`ImportLocal` explicitly request `persist: KeyringRequired { account_id }`
when secure storage must gate account activation and bootstrap publish. Until
that ABI lands, builders should use the current sign-in and restore symbols
without introducing an app-side session store.

## `AccountManager` — synchronous active-switch

`crates/nmp-signers/src/identity/manager.rs:68-78` holds the roster:
`accounts: HashMap<IdentityId, Arc<dyn Signer>>` (id = hex pubkey),
insertion-ordered `order`, an `active: Option<IdentityId>`, and
observers.

### Add an account

`add()` simply inserts the signer into the roster, keyed by hex pubkey
(`IdentityId`). It is idempotent per **PD-004**: re-adding a known pubkey
is a no-op that returns the existing id and keeps the originally-installed
signer. There is no add-time signature probe — local signers are
deterministic crypto and NIP-46 bunkers are already authenticated by the
handshake, so a post-condition sign-and-verify would only add latency.

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

App-managed local signer slots use `nmp_app_register_agent_nsec`. The kernel
persists these slots, resolves them for explicit `signer_pubkey` publishes and
uploads, hides them from account projections, and rejects `SwitchActive` for
their pubkeys. `nmp_app_signin_nsec(make_active=0)` remains a visible secondary
account import, not the hidden app-managed path.

## Builder checklist

For each app shell:

1. Register the native keyring capability before `nmp_app_start` or any
   app-specific identity restore wrapper. iOS/TUI/desktop use
   `nmp_app_set_capability_callback`; Android uses
   `nativeSetCapabilityHandler` before `nativeIdentityRestore`.
2. Keep the native handler mechanical: store, retrieve, delete, and report raw
   `KeyringResult` facts. Do not store signer kind, active account, relays,
   retries, onboarding state, or "logged in" policy in native secure storage.
3. Import user keys through `nmp_app_signin_nsec`, `nmp_app_signin_bunker`,
   `nmp_app_signin_nip55`, or the account-lifecycle ABI once available.
   Import app-owned automation keys through `nmp_app_register_agent_nsec`.
4. Let the actor restore sessions on startup. Do not read an nsec in Swift,
   Kotlin, or desktop code and then decide locally whether to show onboarding
   or switch accounts.
5. Render account and signer state from snapshots/projections. A locked,
   missing, or unavailable signer may gate writes, but it must not blank cached
   read content.
6. Test the cold-start path: sign in, terminate the process, relaunch with the
   same keyring handler registered, and assert the account projection and write
   path recover without a native session cache.

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
4. **"Is logged in?" UI guards that withhold cached content.** A
   missing/locked signer must not blank already-cached events (D1).
   Gate *write* actions, never *reads*.
5. **Re-handshaking NIP-46/NIP-07 on every `pubkey()`.** The pubkey is
   cached at construction; `pubkey()` is sync and free. Treating it as
   async is an API misuse.
6. **App-side session restore.** Storing active pubkey, signer kind, or nsec in
   app code and replaying it on launch creates a second writer. Wire the
   keyring capability and let the actor restore.

## See also

- [10 — Outbox routing (NIP-65)](10-outbox-routing.md)
- [12 — Publishing + the publish engine](12-publish-and-ledger.md)
- [16 — Capabilities (D7)](16-capabilities.md)
- [ADR-0059 — Account lifecycle is separate from bootstrap publish](../decisions/0059-account-lifecycle-bootstrap-policy.md)
