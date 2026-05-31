---
title: Contact List Subscription and Interest Resolution
slug: contact-list-subscription-interest
summary: "ActorCommand::OpenTimeline is replaced by OpenContactListSubscription { kinds: BTreeSet<u32> }, allowing the app to declare which event kinds to subscribe to"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-26
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:6e4c3a3a-9515-4437-a4bf-b4228a10ae57
  - session:64f3e239-c4c1-4c32-82de-458516b28418
---

# Contact List Subscription and Interest Resolution

## Contact List Subscription Interest

ActorCommand::OpenTimeline is replaced by OpenContactListSubscription { kinds: BTreeSet<u32> }, allowing the app to declare which event kinds to subscribe to. A new LogicalInterestSource enum variant ContactListAuthors { viewer: Pubkey, kinds: BTreeSet<u32> } expands into per-followed-author LogicalInterest instances resolved by the kernel. When ContactListAuthors is registered and a kind:3 event arrives where pubkey == viewer, the kernel re-resolves seed_contacts[viewer] and diffs the fan-out; a missing kind:3 is treated as an empty set (CLEAR), not a no-op. When a new kind:3 arrives via the Tailing subscription (e.g. published from another client), ingest_contacts fires, calls sync_follow_feed_interests, and enqueues FollowListChanged, causing the planner to open new follow subscriptions without any app-level code involvement. Auto-include-viewer is app policy, not a kernel contract; apps that want the viewer's own notes must register a separate Direct interest. The InterestId for ContactListAuthors interests derives from (tag, viewer, kinds_hash, author) to prevent collisions when different apps register different kinds for the same viewer. The nmp_app_open_timeline C ABI symbol remains unchanged; Swift, Kotlin, and TUI call sites internally declare {1, 6} rather than receiving it from the kernel.

<!-- citations: [^6e4c3-1] [^64f3e-3] -->
## See Also

