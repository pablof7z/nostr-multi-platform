---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: active
subjects:
  - adr-maintenance-doctrine
  - single-source-of-truth-docs
supersedes: []
related_claims: []
source_lines:
  - 1645-1718
captured_at: 2026-06-15T13:38:06Z
---

# Episode: ADR correction doctrine: correct in place, no superseded annotations

## Prior State

ADR amendments were done by adding inline 'SUPERSEDED by ADR-XXXX' markers and appending amendment blocks at the end — preserving the wrong text while annotating it as historical. ADR-0042 was initially amended this way with three superseded markers and a full amendment block.

## Trigger

User explicitly rejected the annotation pattern: 'don't leave this was superseded — remove the wrong stuff instead of saying that it is now wrong'

## Decision

Correct wrong ADR content in place to state the current truth. No 'SUPERSEDED' inline markers, no amendment blocks, no wrongness archaeology. The doc reads as if it were always correct, with a single forward pointer to the deciding ADR where the new architecture is specified.

## Consequences

- ADR-0042 edited to remove all superseded markers, amendment block, and wrong admission-framing prose — replaced with correct architecture statements
- Establishes reusable policy for all future ADR corrections: single source of truth, no layered annotations
- Three durable docs (subsystems.md, 08-eventstore.md, 12-publish-and-ledger.md) verified as already correct — no churn applied

## Open Tail

*(none)*

## Evidence

- transcript lines 1645-1718
