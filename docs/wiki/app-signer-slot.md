---
title: AppSignerSlot & Agent Key Independence
slug: app-signer-slot
summary: An AppSignerSlot lets an action module sign agent events with a keyring-populated key independent of the active user identity, registered via AddSigner(make_active false) and selected per-publish via signer_pubkey.
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

# AppSignerSlot & Agent Key Independence

## Overview

An AppSignerSlot lets an action module sign agent events using a keyring-populated key that is independent of the active user identity — preserving D4 (single writer per fact) without polluting the account list. The agent key is a real, fully functional signer in the kernel roster; it simply has no UI presence and no account-switcher entry. A signer can be registered after app launch, not only during startup, by sending `AddSigner` on the actor channel at any time. Because the actor channel is FIFO, when a signer registration and a publish command are enqueued together, the registration is guaranteed to be processed before the publish. [^d8869-2]

<!-- citations: [^d8869-2] [^d8869-11] -->

## Registering an agent key — `AddSigner(make_active: false)`

Agent keys (non-user signers) enter the roster through the same primitive every signer uses:

```rust
ActorCommand::AddSigner { source: SignerSource, make_active: bool }
```

For an agent key the call is `AddSigner { source, make_active: false }`. `SignerSource` is `LocalNsec(Zeroizing<String>)` for a keyring-held nsec, `BunkerUri(String)` for a NIP-46 bunker, or `RemoteHandle(Box<dyn RemoteSignerHandle>)` for a broker-supplied remote handle. The `make_active: false` flag is what makes it an agent rather than a user account: the signer is added to the roster but is **not** set active, **not** published as kind:0 / kind:10002, and **not** shown in the account-switcher projection. There is no separate "agent signer" type — the difference from a user sign-in is purely this one flag on the same command.

## Publishing with an agent key — `signer_pubkey: Some(pubkey)`

Publish actions carry an optional `signer_pubkey: Option<String>` selector. `None` signs with the active user account (the default). `Some(pubkey)` signs with the registered signer whose pubkey matches — that is how an action module emits an event under the agent key without ever making it the active account. The kernel resolves the selector against the roster and signs through `sign_with_account_nonblocking`.

Whether the selected signer's key is local (nsec) or remote (NIP-46 bunker) is invisible to the action module: the kernel's async `PendingSign` path handles both transparently, parking the operation on the actor loop until the signature resolves. The module dispatches one action and gets the same nonblocking sign-then-publish pipeline regardless of backend.

## CreateAccount Command

`CreateAccount` is the sole command that publishes kind:0 and kind:10002. It generates a keypair, publishes the metadata events, and internally composes `AddSigner { source: LocalNsec(..), make_active: true }`. Agent keys never take that path — they are added with `make_active: false` and produce no metadata events. [^d8869-13]

## See Also

- [Kernel signing pipeline](kernel-signing-pipeline.md)
- [11 — Sessions + signers + identity scopes](../builder-guide/11-sessions-signers.md)
- [12 — Publishing + the publish engine](../builder-guide/12-publish-and-ledger.md)
