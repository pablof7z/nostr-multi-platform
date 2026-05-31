---
title: Bootstrap Relay Fallback & Zero-Discovery Sign-In
slug: bootstrap-relay-fallback
summary: Bootstrap relay fallback is promoted from test-only to production to fix sign-in with zero discovery relays.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-22
updated: 2026-05-29
verified: 2026-05-22
compiled-from: conversation
sources:
  - session:2c4adc99-0b1b-430c-8594-834da3ab4cef
  - session:53838558-81bd-433d-a46d-d117ecebb361
  - session:6e4c3a3a-9515-4437-a4bf-b4228a10ae57
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# Bootstrap Relay Fallback & Zero-Discovery Sign-In

## Production Status

Bootstrap relay fallback is promoted from test-only to production to fix sign-in with zero discovery relays. Bootstrap relay URLs fall back to hardcoded constants: `wss://relay.primal.net` for content relays and `wss://purplepag.es` for indexer relays. FALLBACK_CONTENT_RELAY and FALLBACK_INDEXER_RELAY activate silently when relay rows are empty, causing the user to publish to an unconsented relay. The NMP kernel fetches kind:0 and kind:10002 for any claimed pubkey even without a logged-in user, connecting to bootstrap/indexer relays automatically. The hardcoded relay.damus.io URL is moved from nmp-core to a host bootstrap capability slot with a composition-root default in nmp-app-template.

<!-- citations: [^2c4ad-1] [^53838-2] [^6e4c3-1] [^cd2b6-4] [^42908-2] -->
## See Also

