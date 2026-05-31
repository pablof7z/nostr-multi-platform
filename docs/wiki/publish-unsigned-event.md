---
title: PublishUnsignedEvent — Kind-Agnostic Kernel Publishing Entrypoint
slug: publish-unsigned-event
summary: "NMP provides ActorCommand::PublishUnsignedEvent(UnsignedEvent) as a kind-agnostic kernel entrypoint that signs with the active identity and routes via NIP-65 ou"
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
  - session:09da8d90-44d5-4038-834b-5393adb0d2b9
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
---

# PublishUnsignedEvent — Kind-Agnostic Kernel Publishing Entrypoint

## PublishUnsignedEvent

NMP provides ActorCommand::PublishUnsignedEvent(UnsignedEvent) as a kind-agnostic kernel entrypoint that signs with the active identity and routes via NIP-65 outbox resolver, with FFI entrypoint nmp_app_publish_unsigned_event(app, json_ptr). PublishUnsignedEvent ignores unsigned.pubkey and derives the pubkey from the active identity keys at sign time, preventing author forgery. NoopSigner is wired as the publish engine signer; only pre-signed events can flow through signing paths. PublishUnsignedEvent is intended as a pragmatic stepping stone that will deprecate gracefully once per-kind ActionModule extraction lands, with typed AppAction dispatches replacing it kind-by-kind. PR-F must address both `nmp_app_publish_signed_event` and `nmp_app_publish_unsigned_event` to close the one-door gap.

Direct WebSocket publish (fetch::send_event) is used alongside kernel fire-and-forget publish for key packages, because the kernel's fire-and-forget silently drops events in the simulator. [^fe79b-13]

Marmot's dependency on `nmp_app_publish_signed_event_to` is resolved by migrating it to an internal kernel API (`Kernel::publish_signed_explicit(event, relays)`), eliminating the FFI-across-crates pattern.

A defensive guard inside `publish_signed_event` makes NIP-17 kind:1059 outbox leaks structurally impossible, regardless of caller.

<!-- citations: [^fe79b-13] [^590ca-10] [^09da8-7] [^1c093-23] -->
## See Also

ActorCommand::PushInterest(LogicalInterest) allows protocol crates to register relay subscriptions without touching Swift code.

<!-- citations: [^fe79b-14] -->
