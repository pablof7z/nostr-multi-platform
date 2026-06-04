---
title: NMP Publish Engine & PublishTarget Routing
slug: nmp-publish-engine
summary: "NMP publish requests must use `dispatch_action(\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\"nmp.publish\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\", ...)` with `PublishTarget::Auto` as the default path, allowing the NMP publish engine to handle re"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-03
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:13382c6f-c5ac-4856-99fd-3cbfdd0b06a5
  - session:d8869714-0ee5-4fe3-94db-1efd068b1c58
  - session:7f143c67-6e46-424a-90a8-5bf844947fee
---

# NMP Publish Engine & PublishTarget Routing

## NMP Publish Engine

NMP publish requests must use `dispatch_action("nmp.publish", ...)` with `PublishTarget::Auto` as the default path, allowing the NMP publish engine to handle relay selection via the NIP-65 outbox model, retry logic, offline intent tracking, and per-relay outcome projection. The publish surface consists of `PublishRaw { kind, tags, content, target }` for unsigned events, `Publish { event, target }` for pre-signed events, and the `nmp_app_retry_publish`/`nmp_app_cancel_publish` control plane; the `PublishNote` action variant, `ActorCommand::PublishNote`, and `publish_note` must be deleted from the publish surface. `PublishProfile` must be retained as a distinct action variant for kind:0 events; `PublishRaw` must continue to reject kind:0 events to enforce the use of `PublishProfile`. `PublishRaw` takes unsigned event fields (kind, tags, content) and the kernel stamps `created_at` and signs it with the active account. `PublishTarget::Auto` resolves relays via NIP-65 outbox; `PublishTarget::Explicit` bypasses the resolver and sends to a specified relay set, and must only be used for protocol-specific relay routing, not as a generic app-provided write relay list; the app always uses `Auto` for `PublishTarget` or omits the target entirely, while `Explicit` is reserved for protocol-crate territory. Protocol-specific relay routing for kind:445 (NIP-29) events and NIP-17 gift wraps must be owned by the specific crate implementing that protocol, not specified by the app; gift-wrap relay routing is owned by the `nmp-nip17` crate, which supplies `PublishTarget::Explicit` with the recipient's inbox relay. Every failure (null app, bad JSON, failed sign) surfaces as an error JSON or a terminal stage, never a crash. Passing a pre-signed event to NMP is invalid; `publish_via_nmp` must dispatch unsigned event parameters (kind, tags, content) to an action module so NMP builds, signs, timestamps, and publishes internally.

<!-- citations: [^13382-1] [^d8869-6] [^7f143-3] [^7f143-13] -->
## Bypass Restrictions

Apps must not bypass the NMP publish engine by manually reading relay URLs, hardcoding fallback relays (e.g., `wss://relay.primal.net`), and dispatching raw capability publish calls. Apps must not treat Nostr relay access as a raw capability (like HTTP) outside of NMP's managed `RelayDispatcher`, `OutboxResolver`, and `PublishEngine`. [^13382-2]
## See Also

