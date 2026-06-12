---
type: episode-card
date: 2026-06-11
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: root-cause
status: active
subjects:
  - adr-0045
  - store-replay
  - nmp-core
  - watermark-replay-interlock
supersedes: []
related_claims: []
source_lines:
  - 3257-3280
captured_at: 2026-06-11T23:22:45Z
---

# Episode: Store replay must bypass insert — Duplicate arm is a deliberate no-op

## Prior State

The 'obvious' fix for offline re-rendering would be to replay stored events through store.insert, but the Duplicate arm (kernel/ingest/timeline.rs:117-130) is a deliberate no-op — insert-based replay would silently surface nothing.

## Trigger

ADR-0045 design investigation verified that store.insert's Duplicate arm returns without updating any projection, making replay-through-insert a silent no-op for all previously-seen events.

## Decision

Replay feeds existing post-store projection functions (insert_timeline_id_sorted + events cache; notify_event_observers) directly with a Provenance::LocalStore marker, bypassing store.insert entirely. Watermark and replay must interlock: 'no watermark floor without replay coverage for the same shape.'

## Consequences

- Stages 1-2 (timeline + DM offline rendering) recommended to gate v1
- Watermark rewrite (#1091) must be guarded by replay coverage for the same shape
- The interlock invariant is now a design constraint for all future projection work

## Open Tail

- Owner must adjudicate whether stages 1-2 gate v1 or are early-post-v1
- Stage 3 (thread/long-form/mentions generalization) is early-post-v1

## Evidence

- transcript lines 3257-3280

