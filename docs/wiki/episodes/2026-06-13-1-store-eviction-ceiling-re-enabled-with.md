---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: product
status: superseded
subjects:
  - nmp-store
  - gc-budget
  - hot-event-ceiling
  - lmdb-growth
supersedes: []
related_claims: []
source_lines:
  - 8070-8082
  - 8120-8158
captured_at: 2026-06-13T19:35:42Z
---

# Episode: Store eviction ceiling re-enabled with floor-coherent pinning (#1090)

## Prior State

HOT_EVENT_CEILING was disabled (max_total_events = usize::MAX), so the on-disk event store grew without bound on long-lived devices. A persisted-watermark mechanism existed but had zero production writers.

## Trigger

Agent analysis of aim.md, doctrine, and zero-debt/single-mechanism-cache-serve principles determined the ceiling must be re-enabled with floor-coherent pinning; the prior watermark machinery is dead code.

## Decision

GO on floor-coherent eviction: pin every stored event matching an active subscription's since-floor, then re-enable the ceiling. Stage 2 (derived pin set + watermark coherence) + Stage 3 (re-enable ceiling + delete dead persisted-watermark machinery). Implementation dispatched.

## Consequences

- Unbounded LMDB growth will be capped at the production ceiling
- Dead persisted-watermark claims machinery to be deleted
- Floor-coherent eviction ensures events reachable by active queries are never evicted
- Engineers dispatched for Stages 2–3 implementation

## Open Tail

- Sub-call: confirm DELETE of dead watermark machinery (recommended)

## Evidence

- transcript lines 8070-8082
- transcript lines 8120-8158

