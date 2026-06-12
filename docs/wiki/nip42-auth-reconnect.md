---
title: NIP-42 Auth Reconnect and Subscription Replay
slug: nip42-auth-reconnect
topic: relay-connection
summary: On Authenticated transition, the kernel calls handle_reconnect for the relay instead of just flushing the AuthGate buffer, so all active plan subscriptions are
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

# NIP-42 Auth Reconnect and Subscription Replay

## NIP-42 Auth Reconnect

On Authenticated transition, the kernel calls handle_reconnect for the relay instead of just flushing the AuthGate buffer, so all active plan subscriptions are re-issued with current watermarks. relay.tenex.chat sends an AUTH challenge immediately on connect, closes REQs sent before auth completes with auth-required, and serves events after re-subscribing post-auth. The NIP-42 auth race bug: REQs sent before an AUTH challenge arrives get CLOSED by the relay, and when auth completes the AuthGate buffer is empty so nothing replays. <!-- [^f1b74-6] -->
