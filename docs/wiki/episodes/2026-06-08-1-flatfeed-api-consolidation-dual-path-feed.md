---
type: episode-card
date: 2026-06-08
session: 65edf39e-4cfd-4bfc-9b65-ec4dc1944b1e
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/65edf39e-4cfd-4bfc-9b65-ec4dc1944b1e.jsonl
salience: architecture
status: active
subjects:
  - flat-feed-api
  - feed-projections
  - pr-triage-supersession
supersedes: []
related_claims: []
source_lines:
  - 265-303
  - 437-466
  - 554-571
captured_at: 2026-06-11T23:08:07Z
---

# Episode: FlatFeed API consolidation: dual-path feed design superseded by single-path on master

## Prior State

PRs #940 (M2 Step-C ProfileView/ThreadScreen) and #941 (V-112 FlatFeed decode + plumbing) were open and presumed to need rebasing and merging. #941 introduced a parallel dual-path API: openAuthorFeed/closeAuthorFeed/openThreadFeed/closeThreadFeed, a feedProjections map, and a FeedProjectionKey enum, deferring the ProfileView cutover. #903 (F-00 directory unification) was also open.

## Trigger

Rebase attempts on #940 and #941 produced 5 and 4 Swift conflicts respectively; reading master's actual code revealed that commits 50041a87 and 2b82591d had already shipped the same features with a consolidated single-path design (openAuthor/openThread registering flat feeds directly, using a flatFeeds map + inline keys). Similarly, F-00 was already on master in stages (47add568, 295d49f9, e17b6983).

## Decision

Close all three PRs as superseded rather than force-push non-compiling rebases. Master's consolidated single-path API (openAuthor/openThread → flatFeeds) is canonical. The PR-only dual-path symbols (openAuthorFeed, closeAuthorFeed, openThreadFeed, closeThreadFeed, feedProjections, FeedProjectionKey, extractFeedProjections) are definitively abandoned.

## Consequences

- 7 PR-only API symbols are dead code that cannot compile against master and must not be resurrected
- Master's flatFeeds + openAuthor/openThread pattern is the sole canonical feed-opening architecture
- The zero-tolerance-on-duplication doctrine was enforced: resolving conflicts to keep the PR's side would re-introduce a duplicate of an already-shipped feature
- F-00 directory layout was already fully landed on master; #903's entire scope was redundant

## Open Tail

- #940's TimelineItem(card:) DRY refactor could be cherry-picked as a standalone PR if desired
- #941's TimelineItem.synthetic home-feed refactor is orthogonal to V-112 and could be extracted separately

## Evidence

- transcript lines 265-303
- transcript lines 437-466
- transcript lines 554-571

