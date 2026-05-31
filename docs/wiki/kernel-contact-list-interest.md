---
title: Kernel Contact List Interest & OpenTimeline Replacement
slug: kernel-contact-list-interest
summary: "Use `ActorCommand::OpenContactListSubscription { kinds: BTreeSet<u32> }` to open a subscription"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-29
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:6e4c3a3a-9515-4437-a4bf-b4228a10ae57
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
---

# Kernel Contact List Interest & OpenTimeline Replacement

## Actor Command

Use `ActorCommand::OpenContactListSubscription { kinds: BTreeSet<u32> }` to open a subscription. The legacy `ActorCommand::OpenTimeline` is replaced by this new command, which carries the set of kinds as an explicit payload rather than relying on the kernel to resolve the active user's follows from its internal `seed_contacts` cache. [^6e4c3-2]



Android must call `openTimeline()` after `createAccount()`, `signInNsec()`, and `switchAccount()` so the kernel re-opens the timeline subscription for the active account. [^f3d8d-11]
## Kernel Kind Hardcoding Violation

Hardcoding `kinds: {1, 6}` inside the kernel's `follow_feed_interest()` is a D0 violation that bakes Chirp-specific social knowledge into the substrate. The kernel must remain agnostic to application-specific kind semantics. [^6e4c3-3]

## LogicalInterestSource::ContactListAuthors

The `LogicalInterestSource::ContactListAuthors { viewer: Pubkey, kinds: BTreeSet<u32> }` enum variant is the declarative seam that replaces the hardcoded kinds in the kernel. When a kind:3 event is ingested where the pubkey matches the `ContactListAuthors` viewer, the kernel must re-resolve the follow set and diff/recompile the per-author interests. If a kind:3 event for the viewer is missing, it resolves to an empty set, which constitutes a CLEAR operation (not a no-op). The `InterestId` for `ContactListAuthors` interests derives from `(tag, viewer, kinds_hash, author)` so that multiple apps registering different kinds over the same viewer do not collide. [^6e4c3-4]

## Viewer's Own Notes

Auto-including the viewer's own notes in the follow feed is app policy, not a kernel contract. Apps must register a separate `Direct` interest if they want the viewer's own notes included. [^6e4c3-5]

## V-45 Backlog Update

The V-45 backlog item wording should be updated to reference NIP-02 (contact list) instead of NIP-05 (DNS) and to describe the action as adding `LogicalInterestSource::ContactListAuthors` and replacing `ActorCommand::OpenTimeline` with `OpenContactListSubscription`. Note that the original V-45 (LogicalInterest::SocialTimeline) was intentionally closed and replaced by the composition-root/ActiveFollowSet closure approach per ADR-0036.

<!-- citations: [^6e4c3-6] [^42908-8] -->
## See Also

