---
type: episode-card
date: 2026-06-13
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: architecture
status: active
subjects:
  - supersession-policy
  - mechanism-census
  - doctrine-lint
supersedes:
  - 2026-06-13-2-excellence-program-deletion-first-doctrine-and
related_claims: []
source_lines:
  - 4887-4913
  - 4760-4952
captured_at: 2026-06-13T19:22:03Z
---

# Episode: Supersession-deletion doctrine prevents mechanism fossil accumulation

## Prior State

Superseded mechanisms coexisted indefinitely with 'legacy' comments that never executed their deprecation; dead vocabulary (enum variants, hooks with no production caller) accumulated across crate seams without cleanup; cross-module contract comments stated invariants without test citations and some were false.

## Trigger

The architecture review found that every 'wrong shape' verdict was actually the right mechanism existing but its predecessor not being buried — the program's stated purpose is to bury these fossils.

## Decision

Institute three AGENTS.md policies enforced at review time: (1) Supersession = deletion — a PR introducing a superseding mechanism must delete the predecessor or land a dated deprecation with tracking issue in the same milestone; (2) Wire-or-delete — no merging vocabulary without a production writer/caller unless registered in the dormant inventory with a deadline; (3) Comments stating cross-module contracts must cite a test. Backed by a mechanism_census CI test (fails on second-generation mechanisms) and a dormant-surface inventory with deadlines.

## Consequences

- mechanism_census test provides CI teeth — any second generation fails CI until the PR deletes one
- Dormant-surface inventory test catches unlisted dead vocabulary
- Five stale-contract comments were found and corrected during the review
- D20 (no-ambient-authority), D21 (correlation-linearity), D22 (presence-floor-ban) doctrine lints planned as enforcement
- Explicitly NOT building: saga coordinator, delta protocol, LateWiring diagnostic, per-envelope bunker unseal RPC, big-bang optimizer

## Open Tail

- D20/D21/D22 lints not yet implemented
- mechanism_census test starts empty — its value depends on future reviewers actually registering mechanisms

## Evidence

- transcript lines 4887-4913
- transcript lines 4760-4952

