---
title: Feed Supersession — Bumping and Deduplicating Reposted Notes
slug: feed-supersession-bumping
summary: When a user reposts a note already present in the feed, the original note's block is bumped to the top rather than creating a duplicate standalone block for the
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-26
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:6e6bcf78-bf6b-4ddd-a2b8-4fb829d86604
---

# Feed Supersession — Bumping and Deduplicating Reposted Notes

## Feed Supersession & Bumping

When a user reposts a note already present in the feed, the original note's block is bumped to the top rather than creating a duplicate standalone block for the repost. [^6e6bc-1]


The `ParentResolver` trait in `nmp-threading` provides a kind-agnostic `supersedes(event) -> Option<EventId>` hook (defaulting to `None`) that declares when one event displaces another's block layout. [^6e6bc-2]

The grouper in `nmp-threading` evicts a superseded target's standalone block when a superseder arrives and suppresses late-arriving originals. [^6e6bc-3]

The grouper restores a target block if its superseder is later removed. [^6e6bc-4]

Reply chains containing a superseded target are left intact rather than evicted. [^6e6bc-5]

A repost card displays the original note's author, kind, and content rather than the reposter's, with an optional `reposted_by: RepostAttribution` carrying the reposter's identity. [^6e6bc-6]

A repost card's top-level `created_at` timestamp remains the kind:6 repost time so the feed bumps it, but the displayed age uses the original note's time. [^6e6bc-7]

chirp-tui prepends a '↻ <reposter> reposted <age>' line above the author for reposted items. [^6e6bc-8]

The `reposted_by` field on `TimelineEventCard` uses `skip_serializing_if = "Option::is_none"` so existing consumers that don't decode it remain compatible. [^6e6bc-9]
## See Also

