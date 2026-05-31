---
title: Pending Event Claim Queue & Drain Path
slug: pending-event-claim-queue
summary: "When `claim_event` fires with `!can_send` (relays not yet connected), the URI and consumer_id are parked in a `pending_event_claims: Vec<(String, String)>` queu"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-27
updated: 2026-05-29
verified: 2026-05-27
compiled-from: conversation
sources:
  - session:7e56b660-13cc-42c9-915c-f8f97ef826d9
  - session:16ca6097-5734-4d49-8b9a-0a20d42a324b
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
  - session:38935d82-0cbf-4e85-98d3-a0f056fd450c
---

# Pending Event Claim Queue & Drain Path

## Pending Event Claim Queue

When `claim_event` fires with `!can_send` (relays not yet connected), the URI and consumer_id are parked in a `pending_event_claims: Vec<(String, String)>` queue on the Kernel. The kernel `claim_send_gate` must gate on `any_relay_connected` (not `all_relays_connected`) so claims with relay hints can proceed even when an unrelated bootstrap relay role is down. Parked claims with URI relay hints must dial those hint relays directly rather than waiting indefinitely for all bootstrap relays. The queue is drained by `pending_event_claim_requests()`, which is called from `pending_view_requests()`, mirroring the existing profile claim pattern. EmbedHost reads `claimedEvents` from the pushed snapshot to resolve envelopes; EventClaimSinkProtocol is defined and KernelModel conforms to it to drive EmbedHost. When a claim is terminated due to early exhaustion in advance_to_phase2, it must be routed through terminate_claim to properly clean up claim_sub_index, rather than setting the phase to Terminal inline.

<!-- citations: [^7e56b-1] [^2073] [^2074] [^16ca6-9] [^6a951-16] [^38935-8] -->
## See Also

