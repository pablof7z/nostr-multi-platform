---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-chirp-config
  - relay-roles
  - kind-10002-discovery
  - primal-relay
supersedes:
  - 2026-06-15-2-primal-relay-role-changed-from-indexer
related_claims: []
source_lines:
  - 3319-3423
captured_at: 2026-06-15T10:28:02Z
---

# Episode: Relay role doctrine: primal as app relay, kind:10002 discovery additive to app relays

## Prior State

primal.net was configured as both,indexer (Lane 6 indexer + app relay). The kind:10002 discovery probe was routed exclusively to indexer-lane relays, meaning only dedicated indexers (primal + purplepag) powered NIP-65 relay-list discovery.

## Trigger

After measuring that adding a broad app relay (nos.lol) jumps resolution from 60% to 89%, user directed: 'make primal an app relay instead of an indexer.' Investigation revealed that removing primal from the indexer set would leave purplepag.es (AUTH-walled for anonymous queries) as the sole discovery relay, silently breaking the outbox model for unauthenticated clients.

## Decision

Changed primal.net from both,indexer to both (app relay only). Made the kind:10002 discovery probe additive to app relays in the kernel, so general-purpose app relays (like primal) also serve relay-list discovery — not just dedicated indexers. PR #1448.

## Consequences

- Primal still receives kind:0 queries (via app-lane routing, additive with indexers + author's own relays)
- kind:10002 discovery no longer depends solely on dedicated indexers — app relays contribute too, making discovery robust even if purplepag AUTH-walls
- This is an architecture doctrine change: the kernel now treats app relays as a valid source for NIP-65 relay lists, not just for content events

## Open Tail

- PR #1448 still needs CI confirmation (especially wasm/web feed test touching the same recompile.rs area as the prior regression)
- Purplepag remains the only dedicated indexer; if it AUTH-walls again, discovery still works but with fewer sources

## Evidence

- transcript lines 3319-3423
