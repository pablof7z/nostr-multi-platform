---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-ffi-claim-profile
  - profile-liveness
supersedes: []
related_claims: []
source_lines:
  - 2554-2558
  - 2732-2733
captured_at: 2026-06-15T04:48:56Z
---

# Episode: Liveness hint distinguishes feed vs profile-screen profile claims

## Prior State

nmp_app_claim_profile was a 4-arg FFI function with no way to distinguish between 'I need this for a feed row avatar' (can tolerate stale/one-shot data) and 'I need this for the profile screen' (needs live/tailing subscription). All claims were treated identically regardless of freshness requirements.

## Trigger

Without a liveness distinction, the kernel could not optimize resource usage: feed rows got over-provisioned tailing subscriptions while profile screens had no way to request live data.

## Decision

Added a 5th argument (liveness: c_int) to nmp_app_claim_profile. 0 = CacheOk (one-shot, for feed-row avatars, mentions, reply attribution), nonzero = Live (tailing subscription, for profile screen). ProfileLiveness::from_ffi maps 0→CacheOk, nonzero→Live.

## Consequences

- Breaking C-ABI change — all consumers must pass the 5th arg (drove v0.7.2→v0.8.0 semver bump)
- Feed avatars use one-shot subscriptions (CacheOk), profile screens use tailing (Live)
- tenex-off, podcast-player required FFI adaptation; hl and win-the-day unaffected (don't call claim_profile)

## Open Tail

*(none)*

## Evidence

- transcript lines 2554-2558
- transcript lines 2732-2733
