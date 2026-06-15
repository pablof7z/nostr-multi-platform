---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: product
status: active
subjects:
  - chirp-ios-profile-claims
  - ios-claim-coverage
supersedes:
  - 2026-06-15-2-ios-self-claiming-surfaces-for-mentions
related_claims: []
source_lines:
  - 3160-3168
captured_at: 2026-06-15T10:01:35Z
---

# Episode: iOS self-claiming surfaces for profile resolution

## Prior State

Chirp iOS only called claim_profile for note authors in rendered feeds. Mentions, attributions (reaction/repost authors), and standalone name displays never triggered a profile claim, leaving those pubkeys permanently unresolved regardless of the kernel's ability to fetch them.

## Trigger

Investigation revealed the ~50% unresolved rate had two compounding causes: (1) the kernel outbox gap, and (2) the iOS UI simply never requesting profiles for entire categories of displayed pubkeys.

## Decision

Added claim_profile calls for all author-display surfaces: mentions in note text, attribution authors for reactions/reposts/zaps, and standalone name displays. Each uses the appropriate liveness hint (CacheOk for feed-row avatars, Live for dedicated profile screens).

## Consequences

- Doubled the claim surface in Chirp iOS, directly contributing to the 10%→50% measured improvement
- Self-claim rule documented: any UI surface that renders a pubkey's identity must claim their profile
- Shipped as PR #1437 (separate from kernel fix #1436)

## Open Tail

*(none)*

## Evidence

- transcript lines 3160-3168
