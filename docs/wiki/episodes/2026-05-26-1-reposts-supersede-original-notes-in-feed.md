---
type: episode-card
date: 2026-05-26
session: 6e6bcf78-bf6b-4ddd-a2b8-4fb829d86604
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/6e6bcf78-bf6b-4ddd-a2b8-4fb829d86604.jsonl
salience: product
status: active
subjects:
  - feed-repost-supersession
  - nmp-threading-grouper
  - timeline-event-card
supersedes: []
related_claims: []
source_lines:
  - 1-9
  - 690-730
  - 856-870
  - 877-906
  - 1020-1058
  - 1727-1728
captured_at: 2026-06-18T06:00:30Z
---

# Episode: Reposts supersede original notes in feed instead of appearing as duplicates

## Prior State

Reposts (NIP-18 kind:6) were treated as independent feed items. When a user reposted a note already on the feed, both the original and the repost appeared as separate entries, duplicating the content instead of bumping the original to the top.

## Trigger

User reported that reposting calle's note showed it as a second entry on the feed rather than bumping the original item to the top with a repost attribution.

## Decision

Introduced a kind-agnostic `supersedes(event) -> Option<EventId>` hook on `ParentResolver` in `nmp-threading`, with `Nip10Resolver` implementing it for kind:6 reposts. When a superseder arrives, the grouper evicts the target's standalone block, suppresses late-arriving originals, and restores the target on superseder removal. The `TimelineEventCard` surfaces the original note's author/kind/content with an optional `reposted_by` attribution; `created_at` uses the repost's timestamp for feed ordering but the card displays the original note's creation time.

## Consequences

- Feed no longer shows duplicate entries for reposted notes — the original is bumped to the top
- chirp-tui renders a '↻ <reposter> reposted <age>' attribution line above the author row
- Grouper tracks `superseded_by: BTreeMap<target, BTreeSet<superseder>>` for multi-superseder bookkeeping; removing all superseder restorations restores the original block
- Reposts inside existing reply chains (Module blocks) are left intact — only Standalone targets are evicted
- `TimelineRow` gained a `repost` field; all test fixtures updated
- The `reposted_by` field on `TimelineEventCard` uses `skip_serializing_if = Option::is_none`, preserving forward-compatibility with Swift/Kotlin consumers that haven't added attribution rendering yet

## Open Tail

- The `supersedes()` hook is kind-agnostic but only NIP-18 kind:6 implements it currently; future protocol crates (e.g., NIP-29 group boosts) could declare their own supersession semantics

## Evidence

- transcript lines 1-9
- transcript lines 690-730
- transcript lines 856-870
- transcript lines 877-906
- transcript lines 1020-1058
- transcript lines 1727-1728

