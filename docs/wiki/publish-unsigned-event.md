---
title: PublishUnsignedEvent Actor Command
slug: publish-unsigned-event
summary: The `PublishUnsignedEvent` actor command accepts a generic `UnsignedEvent`, signs it with the active identity (ignoring `unsigned.pubkey` to prevent author forg
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-21
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:590ca0cd-3665-42f5-96ab-3ea035a79d67
  - session:d27a4f61-511b-4086-845d-335493f9b464
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:47203d35-d7c9-4c12-bc47-a40773d7acc2
---

# PublishUnsignedEvent Actor Command

## Overview

The `PublishUnsignedEvent` actor command accepts a generic `UnsignedEvent`, signs it with the active identity (ignoring `unsigned.pubkey` to prevent author forgery), and routes it via the NIP-65 outbox resolver. It is kind-agnostic and D0-clean. Apps create events (e.g., a NIP-23 article) by using the per-kind builder to produce an `UnsignedEvent`, then dispatching `ActorCommand::PublishUnsignedEvent(unsigned)` to sign and publish it. [^590ca-10]


## Architecture and Evolution

Refactoring `publish.rs` to consume the new protocol-crate builders inverts D0 (the kernel cannot depend on protocol crates). The doctrine-correct path is extracting publish handlers out into per-crate `ActionModule` impls (Phase 1 per kind-wrappers.md §8). `PublishUnsignedEvent` serves as a stepping stone that can be deprecated kind-by-kind as ActionModule extraction lands. Action modules emit unsigned event shapes via a PublishPlan carrier; the actor's signer-bridge signs, and modules never hold keys. [^590ca-11]

<!-- citations: [^590ca-11] [^d27a4-15] -->

## FFI

The `nmp_app_publish_signed_event` FFI publishes pre-signed events verbatim without re-signing. Publish-handle FFI operations (`publish_signed_event*`, `retry_publish`, `cancel_publish`) reside in `ffi/publish.rs`, not `ffi/identity.rs`. The publish_key_package operation uses dual-path publishing: the kernel fire-and-forget path plus a direct WebSocket send_event path via fetch.rs.

<!-- citations: [^590ca-229] [^590ca-253] [^d27a4-16] [^fe79b-13] [^47203-7] -->
## See Also

