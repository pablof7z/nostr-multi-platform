---
type: episode-card
date: 2026-06-13
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: product
status: active
subjects:
  - chirp-parity
  - product-scope
supersedes: []
related_claims: []
source_lines:
  - 8719-8719
captured_at: 2026-06-13T18:49:50Z
---

# Episode: Chirp scope: full feature parity across all 3 platforms

## Prior State

Chirp's feature parity bar across iOS/Android/desktop was unspecified — issue #1291 was an owner-decision item.

## Trigger

Triage surfaced #1291 as an owner-decision requiring explicit scope. The question posed: 'Chirp feature parity across iOS/Android/desktop (#1291): what's the bar? Chirp is the framework's reusability proof, not a shipping product.'

## Decision

Full parity across all 3 platforms. Chirp exists to exercise every NMP seam identically on iOS, Android, and desktop — feature completeness is the proof of framework correctness, not a nice-to-have.

## Consequences

- #1291 updated with full-parity directive
- The full-parity UI sweep is queued after Wave 2 (projection cluster fixes)
- All three shells must consume identical Rust projections; any feature present on one platform must be present on all three

## Open Tail

- Full-parity work is post-Wave-2; no timeline estimate yet

## Evidence

- transcript lines 8719-8719

