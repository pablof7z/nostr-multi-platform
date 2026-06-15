---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: product
status: superseded
subjects:
  - chirp-ios-claim-coverage
  - mentions-attribution-claims
supersedes:
  - 2026-06-14-2-nostravatar-is-sole-owner-of-profile
  - 2026-06-15-2-liveness-hint-distinguishes-feed-vs-profile
related_claims: []
source_lines:
  - 2582-2632
captured_at: 2026-06-15T04:48:56Z
---

# Episode: iOS self-claim profiles for all visible pubkey surfaces

## Prior State

Chirp iOS UI only called claim_profile for feed-row avatars. Mentions in note text, reply attribution authors, and standalone name displays never triggered a profile fetch — meaning many displayed pubkeys remained unresolved even if the kernel could have fetched them.

## Trigger

User reported ~50% of pubkeys don't resolve; iOS investigation showed the UI only self-claimed on some surfaces, leaving a large fraction of displayed pubkeys with no claim and thus no kernel fetch attempt.

## Decision

All inline/list self-claiming surfaces (mentions, reply attribution, standalone names) now call claim_profile with .cacheOk liveness. Profile screen claims with .live. KernelBridge.swift updated to pass the 5-arg signature with appropriate liveness values.

## Consequences

- Profile resolution now covers all visible pubkey surfaces in Chirp iOS, not just feed avatars
- Combined with the kernel outbox discovery fix, addresses both layers of the ~50% failure rate
- NmpCore.h updated to 5-arg signature, restoring ABI consistency with merged kernel

## Open Tail

*(none)*

## Evidence

- transcript lines 2582-2632
