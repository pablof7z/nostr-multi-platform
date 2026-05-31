---
title: SubscriptionLifecycle and InterestRegistry
slug: subscription-lifecycle-and-interest-registry
summary: SubscriptionLifecycle, InterestRegistry, and AuthGate machinery is compiled into the Kernel but dormant; the M1 hand-rolled req() path remains authoritative unt
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-28
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:09da8d90-44d5-4038-834b-5393adb0d2b9
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:64f3e239-c4c1-4c32-82de-458516b28418
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
---

# SubscriptionLifecycle and InterestRegistry

## Kernel Compilation & Dormancy

Two subscription systems coexist: InterestRegistry and M1 hand-rolled req(). SubscriptionLifecycle, InterestRegistry, and AuthGate machinery is compiled into the Kernel but dormant; the M1 hand-rolled req() path remains authoritative until view modules migrate at M11. The M1 hand-rolled req() and M2 planner coexist on the hot path, creating a divergence risk as both evolve. D3 is enforced for the publish path but not demonstrably wired for the live follow-feed read path, where M1 hand-rolled req() remains authoritative. Apps declare event subscriptions via NmpApp::push_interest(LogicalInterest) with a stable InterestId for deduplication, InterestScope for mailbox routing, InterestShape mirroring a Nostr filter, and InterestLifecycle for one-shot vs tailing behavior. InterestRegistry manages active logical subscriptions with deduplication across owners using (owner, key, scope) triples; ensure_sub is idempotent register-if-absent and drop_owner performs refcount GC when the last owner leaves. InterestId is stable — re-pushing the same id is idempotent and the registry de-dupes it. InterestScope controls mailbox routing: ActiveAccount re-routes when the user switches accounts, Global is account-agnostic. On login, NMP subscribes to self-kinds 0, 3, 10002, 10000, and 10006 with Tailing lifecycle (no limit), keeping the subscriptions open for reactive updates. Apps can override the default set of bootstrap self-kinds before nmp_app_start. Bootstrap subscriptions for self-kinds do not need limit:1 because relays automatically send the correct replaceable event. The LogicalInterest struct includes an is_indexer_discovery sentinel field so that Tailing interests can be routed to bootstrap indexer relays, replacing the previous OneShot+Global gate.

<!-- citations: [^09da8-8] [^57528-24] [^95d02-19] [^1670f-19] [^64f3e-9] [^54ae9-15] -->
## Drain Tick Integration

The drain_tick() method on SubscriptionLifecycle must be called from the kernel tick path to drain CompileTrigger::FollowListChanged events. [^57528-25]
## See Also

