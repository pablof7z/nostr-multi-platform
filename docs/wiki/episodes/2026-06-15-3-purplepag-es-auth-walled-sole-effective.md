---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - purplepages-auth-wall
  - indexer-redundancy
  - chirp-profile-baseline
supersedes: []
related_claims: []
source_lines:
  - 2003-2013
  - 2095-2098
captured_at: 2026-06-15T01:10:04Z
---

# Episode: purplepag.es AUTH-walled — sole effective indexer is primal.net

## Prior State

purplepag.es was configured as one of Chirp iOS's two indexer relays, expected to serve kind:0 profiles for anonymous queries alongside primal.net.

## Trigger

Baseline measurement of 1054 follows showed only 108 profiles resolved via indexer-only path (10.2%). Investigation revealed purplepag.es returns 0 kind:0 to anonymous queries because it requires NIP-42 AUTH, making it effectively dead for anonymous profile resolution. The entire 108-profile baseline comes from primal.net alone.

## Decision

Finding documented as a root cause compounding the profile resolution failure. No immediate architecture change (the registry migration already routes to author's own relays, bypassing the indexer bottleneck). Issue awareness propagated to the team.

## Consequences

- Current production architecture has no indexer redundancy — single point of failure on primal.net for anonymous profile resolution
- Explains why the baseline is as low as 10.2% rather than the ~20% expected with two working indexers
- Outbox fix (querying author's own relays) mitigates this by reducing dependence on indexers, but indexers remain the cold-start path for users without NIP-65 relay lists

## Open Tail

- Consider NIP-42 AUTH support for indexer relays, or replace purplepag.es with a non-AUTH-walled indexer
- Indexer redundancy strategy needs explicit design

## Evidence

- transcript lines 2003-2013
- transcript lines 2095-2098
