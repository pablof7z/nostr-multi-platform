---
type: episode-card
date: 2026-06-12
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: root-cause
status: active
subjects:
  - store-replay
  - adr-0045
  - projection-seam
supersedes: []
related_claims: []
source_lines:
  - 3257-3280
captured_at: 2026-06-12T00:32:21Z
---

# Episode: Store→Projection Replay Must Bypass store.insert (ADR-0045)

## Prior State

The obvious approach to replaying stored events into projections on re-open would be to feed them through the existing store.insert path, which already dispatches to projection functions.

## Trigger

ADR-0045 design session verified that store.insert's Duplicate arm (kernel/ingest/timeline.rs:117-130) is a deliberate no-op — insert-based replay would silently surface nothing because already-persisted events are deduplicated away before reaching projection functions.

## Decision

Replay must go through the existing post-store projection functions directly (insert_timeline_id_sorted + events cache + notify_event_observers) with a Provenance::LocalStore marker, bypassing store.insert entirely. The watermark rewrite is guarded by the invariant 'no watermark floor without replay coverage for the same shape.'

## Consequences

- Stages 1–2 (timeline + DM offline rendering) proposed as v1-gating
- Replay is budgeted per-tick on the actor thread, avoiding the unbudgeted-scan anti-precedent
- MLS group state explicitly out of scope for initial replay

## Open Tail

- Owner to adjudicate whether stages 1–2 gate v1 or park as early-post-v1

## Evidence

- transcript lines 3257-3280

