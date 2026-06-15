---
type: episode-card
date: 2026-06-14
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: product
status: superseded
subjects:
  - ios-profile-claim-coverage
  - chirp-ios-profile-resolution
supersedes:
  - 2026-06-14-1-ui-claim-coverage-gap-mentions-and
related_claims: []
source_lines:
  - 83-101
captured_at: 2026-06-14T22:41:51Z
---

# Episode: iOS UI only claimed profiles for avatars and profile-view, missing mentions/reactions/reposts/standalone-names

## Prior State

Only NostrAvatar.swift (feed avatars) and ProfileView.swift (profile screen) called claimProfile. Mention authors, reaction/repost attribution authors, and standalone-name contexts (e.g., following list) never triggered profile resolution, contributing to the ~50% unresolved-pubkey symptom.

## Trigger

iOS audit agent traced the UI claim coverage gap as Fault A in the three-fault diagnosis of the ~50% resolution failure.

## Decision

Add claimProfile calls to MentionView, ReactionAttributionView, RepostAttributionView, and standalone-name contexts. These claims will use CacheOk liveness (OneShot) since they are list-item contexts, not live-editing views.

## Consequences

- Profile resolution triggered for all visible author types in the UI, not just avatars and profile screens
- Combined with kernel registry migration, should close the ~50% resolution gap
- Feed scroll performance unchanged (CacheOk = OneShot, no tailing subs for these contexts)

## Open Tail

- iOS PR blocked on kernel FFI liveness param landing; queued in task pipeline

## Evidence

- transcript lines 83-101
