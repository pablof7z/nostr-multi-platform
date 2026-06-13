---
type: episode-card
date: 2026-06-13
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: architecture
status: superseded
subjects:
  - supersession-policy
  - mechanism-census
  - excellence-program
  - scope-exclusions
supersedes: []
related_claims: []
source_lines:
  - 4834-4850
  - 4889-4913
  - 4917-4936
  - 4940-4952
  - 4954-4958
captured_at: 2026-06-13T18:45:45Z
---

# Episode: Excellence program: deletion-first doctrine and scope exclusions

## Prior State

Mechanisms accumulated across generations with 'legacy' comments that never executed their deprecation (e.g., action_stages). Multiple approaches had been considered for ongoing problems: saga coordinators for payments, delta protocols, LateWiring diagnostics, per-envelope bunker unseal, big-bang optimizer rewrites. No CI-enforced mechanism census existed.

## Trigger

16-agent architecture review found that every QUESTIONABLE verdict fell into one of two failure modes: the right mechanism existed but its predecessor wasn't buried, or the right mechanism was built but never wired. The review produced a structured excellence program identifying six patterns (P1–P6) and their keystone discharges.

## Decision

Committed as reviews/EXCELLENCE-PROGRAM.md (~33KB). Three core doctrines: (1) Supersession = deletion — a PR introducing a superseding mechanism must delete the predecessor in the same milestone or land a dated deprecation with tracking issue; enforced by a mechanism_census test that fails CI on any second generation. (2) Wire-or-delete — no merging vocabulary without a production writer/caller unless registered in a dormant-surface inventory with deadlines. (3) Comments claiming cross-module contracts must cite a test. Explicitly NOT doing: saga coordinator for zaps (compensation impossible in Lightning), delta protocol (WireDelta was shipped, unconsumed, deleted; snapshot bet empirically validated), LateWiring diagnostic (#618 failure made inexpressible by spawn-at-start), per-envelope bunker unseal RPC (O(2N) round-trips), big-bang expected-coverage optimizer, per-projection dirty-tracking rework, multi-session bunker broker without correlation tokens.

## Consequences

- D20 (no ambient authority), D21 (correlation linearity), D22 (presence-floor ban) doctrine-lint rules to be added under bin/doctrine-lint/rules/
- mechanism_census test fails CI on any second generation of a capability
- dormant-surface inventory test with issue + deadline per entry
- bunker parity matrix test ensures second-classness cannot silently regress
- 30-day prioritization: K1 → K2 → K3, with money fix (3.4) in parallel
- Week-one act: land supersession policy + empty census test before any keystone

## Open Tail

- Supersession policy text not yet committed to AGENTS.md (recommended as week-one act before K1)
- mechanism_census test skeleton not yet created
- D20/D21/D22 lint rules not yet implemented
- K2 and K3 not yet started

## Evidence

- transcript lines 4834-4850
- transcript lines 4889-4913
- transcript lines 4917-4936
- transcript lines 4940-4952
- transcript lines 4954-4958

