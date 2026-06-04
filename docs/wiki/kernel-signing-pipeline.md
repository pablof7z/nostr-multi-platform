---
title: Kernel Signing Pipeline & Nonblocking Sign Flow
slug: kernel-signing-pipeline
summary: All event signing goes through the kernel's sign_with_account_nonblocking; apps never access keys or sign events directly.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-04
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:d8869714-0ee5-4fe3-94db-1efd068b1c58
  - session:f1b740a8-d601-4b63-8633-072c83a6de22
  - session:83b5dae5-d3f4-4f4d-b12f-9d04d17c9139
---

# Kernel Signing Pipeline & Nonblocking Sign Flow

## Signing Pipeline

All event signing goes through the kernel's `sign_with_account_nonblocking`, which takes a specific account ID to sign with; apps never access keys or sign events directly. `sign_active_nonblocking` is a one-line wrapper that calls `sign_with_account_nonblocking` with the active account. Apps never hold, use, or access raw secret key bytes; all signing goes through the kernel's `IdentityRuntime`. Whether a signer's key is local (nsec) or remote (NIP-46 bunker) is invisible to the action module; the kernel's async `PendingSign` path handles both. `ActorCommand::SignEventForAccount` provides a generic sign port with `{unsigned, signer_pubkey, continuation}`, generalizing `PendingSignReturn` so local signs inline and bunker parks and resolves async through the same path with identical worker code. Signer transparency is binding: the `ProtocolCommand` worker signs through one uniform sign-account port (local resolves inline, bunker parks async), making local versus bunker key resolution invisible to the worker; any divergence between local and bunker paths constitutes a signer-interface defect to be fixed, not documented as a special case. Apps do not build, sign, or publish events themselves; they dispatch actions and the kernel runs the sign-then-publish pipeline internally. The net end-state is that app Rust signs nothing, including kind:24242 events—no private-key signing remains in app Rust, eliminating D13 violations. The boundary is: NMP owns key custody and signing; apps own HTTP transport and must not hold private keys.

<!-- citations: [^d8869-3] [^f1b74-21] [^d8869-8] [^d8869-17] [^f1b74-27] [^83b5d-2] [^83b5d-6] [^83b5d-11] [^83b5d-17] [^83b5d-24] -->
## PendingSign and Timeout Handling

A `PendingSign` operation parked on the actor loop polls via `try_recv` each idle tick until the remote signer resolves or times out after 5 seconds. On signing timeout, a toast is surfaced and the operation is dropped without wedging the actor thread.

Signer registration is a single primitive:

```rust
ActorCommand::AddSigner { source: SignerSource, make_active: bool }
```

`SignerSource` is `LocalNsec(Zeroizing<String>)` (hex or bech32 nsec), `BunkerUri(String)` (a `bunker://` URI that drives the async NIP-46 handshake), or `RemoteHandle(Box<dyn RemoteSignerHandle>)` (the internal arm the broker uses to hand a connected remote signer back after the handshake). The command adds the signer to the roster and, when `make_active` is `true`, makes it the active identity. With `make_active: false` the signer is added but not set active, does not publish kind:0 / kind:10002, and does not appear in the account-switcher projection.

`CreateAccount` is the sole command that generates a keypair and publishes kind:0 + kind:10002 metadata; it composes `AddSigner { source: LocalNsec(..), make_active: true }` after those side effects. Publish commands accept an optional `signer_pubkey` field; `None` signs with the active account, `Some(pubkey)` signs with the specified registered signer.

`SignContinuation` is defined in always-compiled `actor/mod.rs` rather than the native-only `pending_sign` module, ensuring `ActorCommand::SignEventForAccount` remains compilable in wasm configurations.

The `SignEventForReturn` seam provides a synchronous-sign-and-return interface for both local keys (resolving immediately) and NIP-46 bunkers (async with 5s timeout), surfacing results via a `signed_events` projection keyed by `correlation_id`. The FFI function `nmp_app_sign_event_for_return` takes `(app, account_pubkey_hex, unsigned_json)` and returns a `correlation_id` string. This provides D13-safe sign-and-return for events such as Blossom auth and ShakeFeedback. `nmp_app_sign_event_for_return` signs kind:24242 Blossom auth events for any custodied key (active or per-podcast NIP-F4) and returns flat NIP-01 JSON via the `signed_events` projection. The `signed_events` projection operates on a drain-once contract: each correlation_id appears on exactly one snapshot frame, then is cleared; continuation registration must occur before the sign call.

Swift avatar, artwork, and feedback uploads use a `KernelSigner` conforming to `NostrSigner` that calls `nmp_app_sign_event_for_return` and awaits the `signed_events` projection (with a 60-second timeout), replacing `LocalKeySigner` to eliminate the D13 violation.

The `UploadBlob` `ActorAction` changes `upload_to_blossom` to accept `sign_kind_24242` (a closure) instead of `secret_bytes`, backed by `sign_with_account_nonblocking` via the `SignEventForReturn` seam, with results surfaced via a `blob_uploads` snapshot projection. This change eliminates the raw-key D13 violation. ChangePhotoSheet and LiveAgentOwnedPodcastManager blossom uploads use these kernel-signed kind:24242 events instead of LocalKeySigner on the Swift side.

ShakeFeedback signing routes through the kernel via `SignEventForReturn` and the `signed_events` projection instead of `identity.signer.sign()`.

The V-78 nip57 bunker-zap bug is fixed by migrating from `sign_active_nonblocking` to the unified `sign_for_account(signer_pubkey)` port, using one seam for both nip57 and nmp-blossom consumers.

<!-- citations: [^d8869-4] [^f1b74-22] [^d8869-9] [^d8869-18] [^f1b74-28] [^83b5d-18] [^83b5d-25] -->
## Prohibited Patterns

The app's Rust code must not sign events with private keys; all signing goes through NMP. Apps must not call `active_local_keys()` directly in DM or zap paths, as this is banned by the D13 lint (ADR-0026). Apps must not block the actor thread waiting on a signer; they must use `sign_active_nonblocking` / `sign_with_account_nonblocking` and `PendingSign` to comply with D8. Building, signing, or publishing an event in the app duplicates kernel state and breaks D4/D7. App-side timestamps using `Utc::now().timestamp()` are invalid; `created_at` must come from the kernel via `kernel.now_secs()` (D9).

<!-- citations: [^d8869-5] [^d8869-10] [^d8869-19] [^83b5d-12] -->
## See Also
- [[app-signer-slot|AppSignerSlot & Agent Key Independence]] — related guide
