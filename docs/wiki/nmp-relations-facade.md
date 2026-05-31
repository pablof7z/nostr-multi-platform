---
title: NMP Reactive Relations & Social Facade
slug: nmp-relations-facade
summary: NMP provides reactive relation accessors (replies, reactions, zaps, comments, reposts, thread) so apps do not need to hand-roll subscriptions for common queries
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-28
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:590ca0cd-3665-42f5-96ab-3ea035a79d67
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
---

# NMP Reactive Relations & Social Facade

## Reactive Relation Accessors

NMP provides reactive relation accessors (replies, reactions, zaps, comments, reposts, thread) so apps do not need to hand-roll subscriptions for common queries like 'give me the likes' or 'generate a reply event'. The `Relations` facade in `nmp-reactions` provides `for_event(id, kind)` returning bundled view specs and builder entrypoints (`reply_to`, `react_to`, `repost`, `zap_request`, `comment_on`) as pure free-function composition with no store reference. [^590ca-9]


Lazy sub-stores (MarmotStore, GroupChatStore, DmInboxStore, FollowListStore, DiscoveredGroupsStore) each receive their slice on every tick via `apply(snapshot:)` to keep feature mirrors in sync. [^54ae9-23]
## See Also

