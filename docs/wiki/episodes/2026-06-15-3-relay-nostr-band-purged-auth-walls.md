---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - relay-nostr-band
  - nmp-test-infrastructure
supersedes: []
related_claims: []
source_lines:
  - 3294-3295
  - 3340-3351
  - 3367-3390
  - 3492-3496
captured_at: 2026-06-15T10:39:31Z
---

# Episode: relay.nostr.band purged — AUTH-walls anonymous clients, effectively dead

## Prior State

relay.nostr.band was referenced in 13 files across tests, docs, seed configs, and a gallery TUI log line as a functional relay.

## Trigger

Empirical measurement showed relay.nostr.band returned 0 kind:0 events to anonymous bulk REQs (requires NIP-42 AUTH). It is effectively dead for any unauthenticated client path, making all references misleading.

## Decision

Removed all references. Replaced with actually-functional relays: purplepag.es for real-relay test fixtures, relay.damus.io for AUTH/NIP-42 tests, nos.lol for docs. Fixed the bogus gallery TUI log line that falsely advertised a 4-relay seed list. PR #1451 merged.

## Consequences

- Tests and docs now reference relays that actually respond to anonymous queries
- git grep confirms zero remaining nostr.band references
- Diagnosed the recurring file-size CI flake: duplicate content.ts entry in .file-size-baseline making the gate non-deterministic

## Open Tail

*(none)*

## Evidence

- transcript lines 3294-3295
- transcript lines 3340-3351
- transcript lines 3367-3390
- transcript lines 3492-3496
