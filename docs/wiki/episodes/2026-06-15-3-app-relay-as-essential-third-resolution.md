---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: active
subjects:
  - chirp-relay-config
  - app-relay-resolution-tier
  - no-nip65-cohort
supersedes: []
related_claims: []
source_lines:
  - 3272-3315
captured_at: 2026-06-15T10:01:35Z
---

# Episode: App relay as essential third resolution tier

## Prior State

Chirp shipped only two indexer-role relays (primal, purplepag.es) with no broad content/app relay. The outbox model was assumed to be the primary resolution path, with indexers as fallback. The no-NIP-65 cohort (users who never publish kind:10002) was not recognized as a structurally unreachable group.

## Trigger

Re-measurement of profile resolution showed outbox alone reaches ~60%, but adding a single broad app relay (nos.lol) jumps resolution to 88.8%. Of the 300 net-new follows resolved by the app relay, 204 publish no kind:10002 at all — they are structurally invisible to the outbox model since they have no write relays to query.

## Decision

Three-tier resolution model established as the correct architecture: (1) indexers → ~28% today, (2) + outbox (author's own relays) → ~60%, (3) + broad app relay → ~89%. The no-NIP-65 cohort (31.7% of follows) can only be reached by an app relay; this is a structural limitation, not a fixable bug. Adding a popular general relay to Chirp's default config proposed (pending user approval).

## Consequences

- Adding nos.lol as a default 'both' relay in nmp-chirp-config would close most remaining resolution gap with a one-line config change
- Three-tier stacking is now the understood mental model for NMP profile resolution
- relay.nostr.band appears to require NIP-42 AUTH for bulk queries — excluded from measurement as not representative
- The 88.8% figure is reproducible from ordinary app relays without aggregator magic

## Open Tail

- User asked 'Want me to make that change?' — decision to add nos.lol to Chirp defaults is pending user approval
- Remaining ~11% gap: users who publish kind:0 only to niche relays not in any of the three tiers

## Evidence

- transcript lines 3272-3315
