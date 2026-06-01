---
title: Kernel Claim Lifecycle and Bugs
slug: kernel-claim-lifecycle
summary: The kernel claim-race bug (#843) caused the first relay's EOSE-no-match to tear down a claim before a slower sibling relay could deliver the matching EVENT; the
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-31
updated: 2026-05-31
verified: 2026-05-31
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# Kernel Claim Lifecycle and Bugs

## Kernel Claim Lifecycle

The kernel claim-race bug (#843) caused the first relay's EOSE-no-match to tear down a claim before a slower sibling relay could deliver the matching EVENT; the fix moves teardown to the single `terminate_claim` site gated on `Exhausted|Budget`. The universal `claim_send_gate` bug (#852) gated all claim sending on `all_relays_connected` instead of `any_relay_connected`; when the indexer relay was unreachable (e.g. on the Android emulator), every claim parked permanently without even dialing the nevent's own working relay hint. iOS and TUI have no bypass for the `all_relays_connected` send-gate; they passed only because their environments connected all bootstrap relays, making `all_relays_connected` trivially true — the bug was universal and latent, not Android-specific. [^6a951-21]

## See Also

