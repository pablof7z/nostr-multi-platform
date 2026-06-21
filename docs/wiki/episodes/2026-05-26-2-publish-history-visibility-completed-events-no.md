---
type: episode-card
date: 2026-05-26
session: 7174d4d4-371b-4b8e-87a6-91024c2b4c2a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/7174d4d4-371b-4b8e-87a6-91024c2b4c2a.jsonl
salience: product
status: active
subjects:
  - publish-history
  - publish-queue
  - chirp-tui-outbox
supersedes: []
related_claims: []
source_lines:
  - 762-858
  - 860-913
captured_at: 2026-06-18T05:57:29Z
---

# Episode: Publish history visibility: completed events no longer vanish from TUI outbox

## Prior State

publish_outbox evicted rows once all relays settled; chirp-tui only rendered in-flight publishes. Users could not see where events were sent after completion — published events simply disappeared from the outbox view.

## Trigger

User asked: 'once an event is published it just disappears from the chirp-tui -- is there no way of seeing that stuff somehow? I want to see where an event is being sent or was sent'

## Decision

Parse the kernel's publish_queue projection (durable history of all settled events) and render a read-only 'Published' section beneath the active outbox list. TUI filters out still-in-flight rows from history to avoid duplication. History shown newest-first, capped at 20 (kernel caps at 16). TerminalOutcome now carries relay_reasons so rationale survives eviction from in-flight state.

## Consequences

- TerminalOutcome carries relay_reasons map so per-relay rationale is available for history rows, not just in-flight
- TUI history pane shows all past publishes (kind:0 on account create, reactions, notes, relay lists, etc.) with per-relay status dots and reason strings
- No duplication between active outbox and history pane (in-flight rows excluded from history)

## Open Tail

*(none)*

## Evidence

- transcript lines 762-858
- transcript lines 860-913

