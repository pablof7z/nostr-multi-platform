---
type: episode-card
date: 2026-05-26
session: fbebb78b-07ed-4e26-8e2e-56fb66929a63
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/fbebb78b-07ed-4e26-8e2e-56fb66929a63.jsonl
salience: root-cause
status: superseded
subjects:
  - publish-outbox-status
  - per-relay-state-priority
supersedes: []
related_claims: []
source_lines:
  - 1-3
  - 5652-5709
  - 5783-5850
captured_at: 2026-06-18T05:45:51Z
---

# Episode: Pending-reaction root cause: status priority checked Pending before Ok

## Prior State

publish_outbox_status() checked for any Pending state before any Ok state; when a reaction succeeded on one relay (Ok) but a p-tag fanout relay failed to connect and reverted to Pending, the overall status showed 'pending' indefinitely. is_complete() required all relay states to be terminal, so the row never evicted from the outbox.

## Trigger

User reported reactions always stuck at 'pending' in chirp-tui. Investigation traced the p-tag fanout path: Nip65OutboxResolver adds recipient read relays → one relay fails to connect → mark_relay_unavailable() reverts InFlight to Pending → publish_outbox_status() sees Pending before Ok → displays 'pending'.

## Decision

Reordered publish_outbox_status() to check for any Ok relay state before checking for Pending. When at least one relay accepted, the row now returns 'queued' (displayed 'Queued') instead of 'pending'. The deeper is_complete() semantic (one-Ok-equals-published → evict) was explicitly deferred.

## Consequences

- Partially-succeeded publishes no longer misleadingly show 'pending'
- Row still stays in outbox until all per-relay states are terminal (is_complete unchanged)
- The deeper question of whether NIP-01 semantics (one accepting relay = published) should drive eviction from in_flight remains open

## Open Tail

- is_complete() could be changed to settle on any Ok, causing eviction — deferred pending user decision

## Evidence

- transcript lines 1-3
- transcript lines 5652-5709
- transcript lines 5783-5850

