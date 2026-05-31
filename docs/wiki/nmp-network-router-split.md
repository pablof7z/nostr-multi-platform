---
title: NMP Network & Router Crate Split
slug: nmp-network-router-split
summary: "Relay transport and routing are two separate crates: `nmp-network` (Layer 1, sockets + pool lifecycle + reconnection) and `nmp-router` (Layer 2, relay selection"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-23
updated: 2026-05-29
verified: 2026-05-23
compiled-from: conversation
sources:
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# NMP Network & Router Crate Split

## Architecture

Relay transport and routing are two separate crates: `nmp-network` (Layer 1, sockets + pool lifecycle + reconnection) and `nmp-router` (Layer 2, relay selection algorithm + MailboxCache, including per-kind routing). V-50 (relay routing) is resolved; per-kind routing shipped as the `nmp-router` crate. The `v58_set_backoff_hint_does_not_break_reconnect` test in `nmp-network` is a known flaky relay-reconnect timing test; lone CI failures on it are re-run rather than treated as regressions.

<!-- citations: [^1670f-10] [^d0690-4] [^4edd4-24] -->
## See Also

