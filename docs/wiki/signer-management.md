---
title: Signer Management and Multi-Account Signing
slug: signer-management
topic: signer-management
summary: "`SignInNsec`, `SignInBunker`, and `AddRemoteSigner` are replaced by a single primitive: `AddSigner { source: SignerSource, make_active: bool }`"
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
  - session:f1b740a8-d601-4b63-8633-072c83a6de22
---

# Signer Management and Multi-Account Signing

## Signer Primitives

`SignInNsec`, `SignInBunker`, and `AddRemoteSigner` are replaced by a single primitive: `AddSigner { source: SignerSource, make_active: bool }`. No back-compat aliases are kept for the deleted commands; the change is a clean break. `nmp_app_add_signer_nsec` was rejected as a redundant FFI symbol because it duplicates `nmp_app_signin_nsec`; non-active signer registration goes through the existing signin function with `make_active=0` instead. `CreateAccount` is the sole command that publishes kind:0 and kind:10002; `AddSigner` never does regardless of `make_active`. Signer registration via `AddSigner` can happen at any time after the actor is started, not only at app launch; commands are processed in FIFO order so registration before publish is guaranteed. A signer registered with `make_active: false` does not appear in the account-switcher UI, does not publish kind:0/10002, and does not affect the active account. The `make_active: u8` parameter was added to `nmp_app_signin_nsec`, `nmp_app_signin_bunker`, and `nmp_app_create_new_account`; existing callers must pass 1, non-active signer registration passes 0.

<!-- citations: [^d8869-3] [^d8869-4] [^d8869-5] [^f1b74-8] -->
## App-Facing Signer Surface

The app-facing handle for a non-active signer is a plain public key string (`agent_pubkey_hex`), not a `dyn` trait object; the secret or remote handle lives inside the actor's `IdentityRuntime`. No new `AppSigner` trait is introduced on the app-facing surface; signer agnosticism (local nsec vs NIP-46 vs NIP-07) is a property of the internal resolver in `IdentityRuntime`, not an app-held object. <!-- [^d8869-6] -->

## Signing API

`sign_active_nonblocking` is generalized to `sign_with_account_nonblocking(identity, account_id, unsigned)`, with `sign_active_nonblocking` becoming a thin wrapper that calls it with the active pubkey. The kernel provides `nmp_app_sign_event_for_return(app, account_pubkey_hex, unsigned_json)` returning a `correlation_id`, enabling synchronous-sign-and-return semantics for local keys and async resolution (5s timeout) for NIP-46 bunkers. The `LocalSignerAccess` trait has a `sign_active_nonblocking` method with a default implementation falling back to `active_local_keys`, so test stubs that don't need nonblocking sign still compile.

<!-- citations: [^d8869-7] [^f1b74-9] -->
## Account Manager Changes

`verify_signer` and `add_unverified` are removed from `AccountManager`; `add()` is now a plain idempotent insert with no probe round-trip. <!-- [^d8869-8] -->

## Migration

Migration for apps upgrading to this version requires: (1) replacing `sign_in_nsec`/`SignInBsec` calls with `add_signer(SignerSource::..., make_active)`, (2) adding `signer_pubkey: None` to all `PublishUnsignedEvent`/`PublishUnsignedEventToRelays` construction sites, (3) removing `AccountError::SignerMismatch`/`SignerError` match arms, (4) replacing `add_unverified(signer)` with `add(signer)`, (5) passing `make_active: 1` to `nmp_app_signin_nsec`, `nmp_app_signin_bunker`, and `nmp_app_create_new_account` (or `0` for non-active signer registration), (6) using `nmp_app_signin_nsec` with `make_active=0` instead of any removed `nmp_app_add_signer_nsec` calls.

<!-- citations: [^d8869-9] [^f1b74-10] -->
