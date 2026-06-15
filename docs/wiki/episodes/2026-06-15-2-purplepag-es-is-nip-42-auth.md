---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: active
subjects:
  - purplepages
  - indexer-relays
  - nip-42-auth
  - profile-resolution
supersedes:
  - 2026-06-15-3-purplepag-es-auth-walled-sole-effective
related_claims: []
source_lines:
  - 1998-2014
captured_at: 2026-06-15T01:36:35Z
---

# Episode: purplepag.es is NIP-42 AUTH-walled — effectively dead for anonymous profile resolution

## Prior State

Chirp configured two indexer relays (purplepag.es and primal.net) assuming both serve kind:0 events for any pubkey to anonymous clients.

## Trigger

Baseline measurement during the investigation tested both relays individually and discovered purplepag.es returns 0 kind:0 events to anonymous (unauthenticated) queries, requiring NIP-42 AUTH.

## Decision

Finding documented: the entire 10.2% baseline resolution comes from primal.net alone. Purplepag.es contributes zero without NIP-42 AUTH, making it a dead relay for the current anonymous-query path.

## Consequences

- Explains why the baseline was as low as 10.2% (not just outbox-inert kernel, but also half the indexer capacity returning nothing)
- The outbox fix's 50% ceiling (vs theoretical 57.6%) is partly because users without NIP-65 lists have no fallback beyond primal.net alone
- Future improvement: implement NIP-42 AUTH to purplepag.es to recover the second indexer for anonymous profile queries

## Open Tail

- NIP-42 AUTH support not yet planned or scheduled

## Evidence

- transcript lines 1998-2014
