---
title: NIP-65 Relay List Cache
slug: nip65-relay-list-cache
summary: EmptyMailboxCache and InMemoryMailboxCache are deliberate Phase 1 stubs; the real nmp-nip65 relay-list cache crate does not yet exist.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-25
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:09da8d90-44d5-4038-834b-5393adb0d2b9
  - session:fd8095ba-6ff1-4552-9ee1-5b6e79f1bb53
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:53838558-81bd-433d-a46d-d117ecebb361
---

# NIP-65 Relay List Cache

## Phase 1 Stubs

EmptyMailboxCache and InMemoryMailboxCache are deliberate Phase 1 stubs; the real nmp-nip65 relay-list cache crate does not yet exist. nmp-ffi alone uses EmptyOutboxRouter; nmp-app-template::register_defaults must be called to install GenericOutboxRouter + InMemoryMailboxCache for claim_profile to produce relay REQs.

<!-- citations: [^09da8-6] [^fd809-5] [^1670f-11] [^53838-6] -->
## Outbox Resolution

The kernel uses NIP-65 outbox resolution with a cold-start fallback to decide which relay to ask for metadata. [^fd809-6]
## See Also

