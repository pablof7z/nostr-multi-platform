---
type: episode-card
date: 2026-06-13
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: architecture
status: superseded
subjects:
  - file-size-doctrine
  - baseline-enforcement
  - code-organization
supersedes:
  - 2026-06-13-2-file-size-gate-enforced-as-debt
related_claims: []
source_lines:
  - 8862-8874
  - 9183-9225
captured_at: 2026-06-13T20:26:16Z
---

# Episode: File-size hard cap enforced as zero-tolerance: always split, never baseline-bump

## Prior State

The 500-line hard cap existed in CI but agents routinely grew already-over-baseline files and sometimes masked the growth with baseline bumps — the gate caught violations only after the fact

## Trigger

9 of 19 Wave 1 PRs violated the file-size gate by bolting new code onto already-over-ceiling files (e.g. TypedProjectionGlue.swift 961→988, actor/mod.rs 2482→2514, builder.rs 703→750, swift_projections_registry.rs 921→938)

## Decision

Never baseline-bump; always extract additions into cohesive sibling modules (e.g. TypedProjectionGlue+RelayDiagnostics.swift). Over-ceiling files must shrink, not grow. Zero-debt bar: any PR that grows an over-baseline file is rejected.

## Consequences

- 9 Wave 1 PRs rejected and sent back for proper decomposition (Wave 1b)
- TypedProjectionGlue.swift split pattern (988→881 + 117-line sibling) established as the template for all future splits
- Wave 1b debt-fix wave launched to decompose all 9 offenders
- Sets precedent: every future PR must pass the file-size gate honestly — no masking with baseline bumps
- Actively chips away at #962/#1162 file-size debt

## Open Tail

- Wave 1c still in flight rebasing conflicted PRs
- Remaining over-baseline files in the codebase need progressive decomposition as they're touched

## Evidence

- transcript lines 8862-8874
- transcript lines 9183-9225

