---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - profile-resolution
  - outbox-model
  - nmp-core-registry
  - nip-65
supersedes:
  - 2026-06-15-1-third-party-profile-resolution-migrated-from
related_claims: []
source_lines:
  - 48-84
  - 2095-2098
captured_at: 2026-06-15T03:41:03Z
---

# Episode: Third-party profile resolution: indexer-only → outbox model via registry with kind:10002 discovery

## Prior State

The outbox model (NIP-65) machinery existed in the router but was inert for third-party profiles: kind:10002 was only fetched for the self/active account at startup. Arbitrary authors' kind:0 queries were routed solely to the operator indexer relay set (purplepag.es + primal.net). Any user who only published kind:0 to their own NIP-65 write relays would silently never resolve.

## Trigger

User reported ~50% of pubkeys never resolve in Chirp iOS. Multi-agent investigation confirmed the outbox model was implemented but not effectively usable — Lane 1 (outbox) resolved to empty for third-party pubkeys because MailboxCache had no entry, so queries fell through to indexer-only (Lane 6). Measurement showed 10.2% resolution with indexer-only vs 50% with indexer ∪ own-relays.

## Decision

Migrate profile-claim and F-TTL reverify onto a unified registry chokepoint that proactively discovers each author's kind:10002 write relays before routing their kind:0 query. The registry also enables retry-on-miss (re-arm on genuine indexer reconnects, not every connect to avoid churn) and liveness hints (feed avatars/inline claims use .cacheOk; profile screens use .live for aggressive re-verification).

## Consequences

- Profile resolution rate improved ~5x: 10.2% → 50% of followed users resolvable
- purplepag.es requires NIP-42 AUTH and returns nothing anonymously — the entire current baseline comes from primal.net alone; this is a separate discovery that changes future relay-selection thinking
- 57.6% of followed users have discoverable kind:10002 (M=608 of N=1054); the remaining ~43% still rely on indexer coverage
- Breaking FFI change: claim_profile went from 4-arg to 5-arg (adding liveness), requiring coordinated iOS header + Swift updates
- The proactive kind:0 fetch on note ingest was already removed (F-CR-00); profiles are fetched only on UI component claims

## Open Tail

- The ~43% of users without kind:10002 still depend on indexer relay coverage; further improvement requires broader indexer adoption or alternative discovery
- purplepag.es AUTH requirement means it contributes zero anonymous resolution — should it be replaced or should AUTH be supported?

## Evidence

- transcript lines 48-84
- transcript lines 2095-2098
