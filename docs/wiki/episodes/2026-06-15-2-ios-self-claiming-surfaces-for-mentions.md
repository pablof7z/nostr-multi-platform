---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: product
status: superseded
subjects:
  - chirp-ios-claim-coverage
  - ios-ui-profile-claim
supersedes:
  - 2026-06-15-2-chirp-ios-self-claim-coverage-for
related_claims: []
source_lines:
  - 41-43
  - 2626-2638
captured_at: 2026-06-15T09:49:18Z
---

# Episode: iOS self-claiming surfaces for mentions, attributions, standalone names

## Prior State

Chirp iOS only called claim_profile for note-author avatars and explicit profile-screen views. Mentions (@-references), reply attribution lines, and standalone name labels never triggered a claim — meaning ~50% of visible pubkeys had no profile fetch initiated at all. The UI passively waited for profiles that were never requested.

## Trigger

Investigation of the ~50% unresolved rate revealed two independent causes: (1) kernel outbox inertness (architectural) and (2) iOS UI simply never claiming profiles for many pubkey surfaces. The UI agent identified that inline contexts (mentions, reply attribution, standalone names) were missing claim calls entirely.

## Decision

Added self-claiming surfaces: all inline/list contexts (mentions, reply attributions, standalone names) now call claim_profile with liveness=.cacheOk. Profile-screen views use liveness=.live (Tailing subscription). Feed/list avatars remain .cacheOk (OneShot).

## Consequences

- Dramatically expands the set of pubkeys for which Chirp iOS initiates profile resolution
- Combined with the kernel outbox fix, resolves the reported ~50% unresolved-pubkey rate
- PR #1437 merged to master with the liveness hint wired across all surfaces
- Swift-side claimProfile wrappers now accept force/liveness parameters (defaulting to CacheOk)

## Open Tail

- Reaction and repost author avatars may still be unclaimed if they appear in UI contexts not yet covered
- Battery/network impact of expanded claims not yet measured

## Evidence

- transcript lines 41-43
- transcript lines 2626-2638
