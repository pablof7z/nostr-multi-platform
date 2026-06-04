---
title: NIP-42 Auth Gate & REQ Replay
slug: nip-42-auth-gate
summary: Nip42Driver automatically signs and sends the AUTH response when a challenge arrives and a signer is bound
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-03
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:f1b740a8-d601-4b63-8633-072c83a6de22
---

# NIP-42 Auth Gate & REQ Replay

## NIP-42 Authentication Gate

App relays must be pre-authenticated via NIP-42 before subscriptions are opened to prevent REQs being closed due to auth-required. NIP-42 authentication is handled automatically by Nip42Driver, which immediately signs and sends the kind:22242 AUTH response when a challenge arrives and a signer is bound. When a relay requires auth, REQs sent before authentication completes are closed by the relay with auth-required and are not automatically replayed by merely flushing the AuthGate buffer. On transitioning to Authenticated state, the kernel calls lifecycle.handle_reconnect(relay_url) to replay all active plan subscriptions, fixing the race where REQs sent before a NIP-42 AUTH challenge arrive get closed by the relay. This covers both buffered REQs and REQs closed before auth completed. relay.tenex.chat requires NIP-42 authentication before serving any REQ subscriptions, closing unauthenticated subscriptions with auth-required. The delivering_relay_url used in auth paths must match the canonical URL keys in current_plan.per_relay.

<!-- citations: [^f1b74-6] [^f1b74-1] [^f1b74-2] [^f1b74-5] [^f1b74-15] -->
## See Also

NMP version numbers follow a 0.x.y scheme. The NIP-42 auth race fix, open_interest FFI, PublishRaw{kind:1} replacing PublishNote, and unified AddSigner API are released as NMP v0.2.3. [^f1b74-6]

A manual test program must be written to connect to relay.tenex.chat and exercise the auth race by requesting kind 31933 events. The manual test program must be added as an example in the nmp-repl crate. [^f1b74-3]

<!-- citations: [^f1b74-3] [^f1b74-16] -->
