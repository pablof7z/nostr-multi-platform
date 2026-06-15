---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: active
subjects:
  - mailbox-probe-epoch
  - indexer-lane-recovery
  - subscription-lifecycle
supersedes: []
related_claims: []
source_lines:
  - 4358-4410
captured_at: 2026-06-15T18:08:06Z
---

# Episode: Mailbox-probe epoch replaces per-socket gate to eliminate multi-indexer churn

## Prior State

Per-socket indexer_socket_was_down gate caused churn with multiple indexers: a single flapping indexer re-blasted the entire kind:10002 mailbox probe batch while sibling indexers stayed live.

## Trigger

B3 task identified that per-socket fix (#1436) was insufficient for multi-indexer environments — only a genuine full-outage→recovery should re-trigger probes, not a single-indexer flap among live siblings.

## Decision

Replaced per-socket gate with a lane-level outage epoch: SubscriptionLifecycle::note_indexer_lane_recovered tracks down→up edges with a monotonic probe_epoch. Only a genuine recovery (every indexer was down, then one returned) bumps the epoch and emits IndexerSetChanged. Sibling-still-live reconnects and cold-start first connects are no-ops.

## Consequences

- Multi-indexer deployments no longer experience probe-batch re-blasts on single-indexer flaps
- epoch is monotonic and re-arm is edge-triggered only on genuine full-outage recovery
- Deleted dead indexer_socket_was_down/connection_state helpers

## Open Tail

*(none)*

## Evidence

- transcript lines 4358-4410
