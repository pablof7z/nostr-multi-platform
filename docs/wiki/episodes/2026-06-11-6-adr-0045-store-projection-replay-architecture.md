---
type: episode-card
date: 2026-06-11
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: architecture
status: active
subjects:
  - adr-0045
  - store-replay
  - watermark
  - offline-rendering
supersedes: []
related_claims: []
source_lines:
  - 3257-3280
captured_at: 2026-06-11T23:31:21Z
---

# Episode: ADR-0045: store→projection replay architecture

## Prior State

No mechanism existed for store replay at interest-open time. Reopening the app showed nothing offline because watermark floors were above all stored content. The 'obvious' fix — replay through `store.insert` — would silently surface nothing because the Duplicate arm is a deliberate no-op.

## Trigger

Design need for offline rendering; the verified finding that `store.insert` Duplicate is a no-op kills the naive replay approach.

## Decision

Budgeted store-replay seam at interest-open/compile time: `CompileTrigger::ViewOpened` maps `InterestShape` → `StoreQuery`, `query_visit`s the store newest-first up to the projection's visible window, feeds existing projection functions with `Provenance::LocalStore` marker. Watermark rewrite stays, guarded by invariant 'no watermark floor without replay coverage for the same shape'. Staged: stages 1–2 (timeline + DM) gate v1, stage 3 (generalize) early-post-v1.

## Consequences

- This is the missing architectural piece that makes 'reopen app, see your feed offline' work
- The Duplicate-arm no-op finding prevents a whole class of naive replay bugs
- v1-line decision deferred to owner: stages 1–2 add ~1–2 weeks to v1

## Open Tail

- Owner adjudication on v1 scope for stages 1–2
- Stage 3 generalization post-v1

## Evidence

- transcript lines 3257-3280

