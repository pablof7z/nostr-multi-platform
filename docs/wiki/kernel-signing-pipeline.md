---
title: Kernel Signing Pipeline & Nonblocking Sign Flow
slug: kernel-signing-pipeline
summary: All event signing goes through the kernel's sign_with_account_nonblocking; apps never access keys or sign events directly.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-03
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:d8869714-0ee5-4fe3-94db-1efd068b1c58
---

# Kernel Signing Pipeline & Nonblocking Sign Flow

## Signing Pipeline

All event signing goes through the kernel's `sign_with_account_nonblocking`, which takes a specific account ID to sign with; apps never access keys or sign events directly. `sign_active_nonblocking` is a one-line wrapper that calls `sign_with_account_nonblocking` with the active account. Apps never hold, use, or access raw secret key bytes; all signing goes through the kernel's `IdentityRuntime`. Whether a signer's key is local (nsec) or remote (NIP-46 bunker) is invisible to the action module; the kernel's async `PendingSign` path handles both. Apps do not build, sign, or publish events themselves; they dispatch actions and the kernel runs the sign-then-publish pipeline internally. [^d8869-3]

<!-- citations: [^d8869-3] [^d8869-8] [^d8869-17] -->

## PendingSign and Timeout Handling

A `PendingSign` operation parked on the actor loop polls via `try_recv` each idle tick until the remote signer resolves or times out after 5 seconds. On signing timeout, a toast is surfaced and the operation is dropped without wedging the actor thread.

Signer registration is a single primitive:

```rust
ActorCommand::AddSigner { source: SignerSource, make_active: bool }
```

`SignerSource` is `LocalNsec(Zeroizing<String>)` (hex or bech32 nsec), `BunkerUri(String)` (a `bunker://` URI that drives the async NIP-46 handshake), or `RemoteHandle(Box<dyn RemoteSignerHandle>)` (the internal arm the broker uses to hand a connected remote signer back after the handshake). The command adds the signer to the roster and, when `make_active` is `true`, makes it the active identity. With `make_active: false` the signer is added but not set active, does not publish kind:0 / kind:10002, and does not appear in the account-switcher projection.

`CreateAccount` is the sole command that generates a keypair and publishes kind:0 + kind:10002 metadata; it composes `AddSigner { source: LocalNsec(..), make_active: true }` after those side effects. Publish commands accept an optional `signer_pubkey` field; `None` signs with the active account, `Some(pubkey)` signs with the specified registered signer. [^d8869-4]

<!-- citations: [^d8869-4] [^d8869-9] [^d8869-18] -->

## Prohibited Patterns

Apps must not call `active_local_keys()` directly in DM or zap paths, as this is banned by the D13 lint (ADR-0026). Apps must not block the actor thread waiting on a signer; they must use `sign_active_nonblocking` / `sign_with_account_nonblocking` and `PendingSign` to comply with D8. Building, signing, or publishing an event in the app duplicates kernel state and breaks D4/D7. App-side timestamps using `Utc::now().timestamp()` are invalid; `created_at` must come from the kernel via `kernel.now_secs()` (D9). [^d8869-5]

<!-- citations: [^d8869-5] [^d8869-10] [^d8869-19] -->
## See Also
