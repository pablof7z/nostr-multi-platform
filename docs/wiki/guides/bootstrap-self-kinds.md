---
title: Bootstrap Self-Kinds
slug: bootstrap-self-kinds
topic: crate-architecture
summary: NMP subscribes at login to the user's self-kinds with a Tailing (persistent) subscription for kinds 0, 3, 10002, 10000, and 10006, so that updates to any of the
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-06-19
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:64f3e239-c4c1-4c32-82de-458516b28418
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# Bootstrap Self-Kinds

## Bootstrap Self-Kinds

NMP subscribes at login to the user's self-kinds with a Tailing (persistent) subscription for kinds 0, 3, 10002, 10000, and 10006, so that updates to any of these kinds arrive reactively from the relay without polling. Kind 10050 is excluded from the default self-kinds (it is owned by the NIP-17 runtime) and remains a OneShot discovery interest. Bootstrap self-kind subscriptions must not use limit:1 — relays automatically send the correct replaceable event. <!-- [^64f3e-1] -->

## App Overrides

Apps can override the default bootstrap self-kinds set before calling nmp_app_start. <!-- [^64f3e-2] -->

## Slot Persistence and Account Safety

NmpApp has pre-start slots for bootstrap_self_kinds and blocked_relay_lookup that survive Reset via the dispatch-context re-binding path. Account-switch safety for bootstrap interests uses drop_owner + set_sub pattern so that switching accounts replaces the slot's author in-place rather than silently keeping the old pubkey in the interest filter.

The kernel persists host-declared follow_feed_kinds unconditionally (even without an active account), fixing a latent both-platforms no-feed-after-sign-in bug; Android removes the imperative openTimeline post-identity call.

<!-- citations: [^64f3e-3] [^11850-49] [^11850-113] [^11850-236] -->
