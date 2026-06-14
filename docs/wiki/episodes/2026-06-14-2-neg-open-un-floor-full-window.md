---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: product
status: active
subjects:
  - neg-open-unfloor
  - nip77-reconciliation
  - eligible-filter-unfloored
supersedes: []
related_claims: []
source_lines:
  - 6020-6034
captured_at: 2026-06-14T13:26:23Z
---

# Episode: NEG-OPEN un-floor: full-window reconciliation for NIP-77

## Prior State

NEG-OPEN (NIP-77) inherited the since-floored filter from ReqFrameContext, so set reconciliation only covered [floor, ∞) — exactly the window the floor declared uninteresting. Below-floor gaps could never be repaired. For follow feeds with ≥50 author×kind fanout, a single stray below-floor stored event permanently suppressed an author's backfill.

## Trigger

H2 finding from the 16-journey read-path review: the presence floor's since-lowering applied to NEG-OPEN creates a class of permanent backfill holes that reconciliation cannot self-heal.

## Decision

Introduce EligibleFilter::unfloored() that drops the since lower bound on the NEG path (for both local_items and the NEG-OPEN filter), while keeping the floor on plain REQs. NIP-77 reconciliation now operates over the full window and is self-healing.

## Consequences

- Below-floor gap repair is automatic for NEG-eligible shapes
- Plain REQ path still benefits from the floor (avoids re-fetching cached events)
- TDD oracle proves a below-floor backfill gets repaired under the unfloored path

## Open Tail

*(none)*

## Evidence

- transcript lines 6020-6034

