---
type: episode-card
date: 2026-05-19
session: 5d893073-9635-450b-b8e9-50648bc1a4e7
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/5d893073-9635-450b-b8e9-50648bc1a4e7.jsonl
salience: product
status: superseded
subjects:
  - kernel-timeline
  - seed-pubkeys
supersedes:
  - 2026-05-18-1-remove-seed-timeline-show-only-logged
related_claims: []
source_lines:
  - 3053-3073
captured_at: 2026-06-18T04:20:28Z
---

# Episode: Remove hardcoded seed timeline — bootstrap from active account only

## Prior State

The kernel permanently unioned three hardcoded pubkeys (fiatjaf, jb55, pablof7z) into timeline_authors, meaning every logged-in user saw their follows PLUS those three people's posts regardless of their own follow list.

## Trigger

Discovered during commit recovery — commit 996963de 'fix(kernel): remove hardcoded seed timeline, bootstrap from active account'

## Decision

Removed all seed-specific REQs from startup_requests. sync_follow_feed_interests now adds the active user's own pubkey to timeline_authors (so users see their own posts) instead of unioning seed pubkeys. should_open_timeline gates on active account's contacts, not seeds. Renamed SeedTimeline status to Timeline.

## Consequences

- Timeline content is now fully determined by the active user's social graph
- No developer's follows are force-injected into every user's feed
- Bootstrap fetches active user's kind:3/profile/relays instead of seed REQs
- sign_in_nsec/create_account/switch_active must reconcile follow-feed and emit bootstrap REQs for the new account

## Open Tail

*(none)*

## Evidence

- transcript lines 3053-3073

