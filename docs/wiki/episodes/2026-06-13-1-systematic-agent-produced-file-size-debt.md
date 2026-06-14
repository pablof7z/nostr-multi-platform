---
type: episode-card
date: 2026-06-13
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: root-cause
status: active
subjects:
  - file-size-gate
  - agent-pr-quality
  - zero-debt-doctrine
supersedes:
  - 2026-06-13-2-file-size-hard-cap-enforced-as
related_claims: []
source_lines:
  - 8860-8895
  - 8920-8935
  - 9186-9224
  - 9257-9269
  - 9356-9358
captured_at: 2026-06-13T20:53:18Z
---

# Episode: Systematic agent-produced file-size debt — enforce split, ban baseline bumps

## Prior State

The 500-LOC hard-cap and .file-size-baseline doctrine existed in AGENTS.md, but in practice agents routinely bolted new code onto already-over-ceiling files, expecting the gate to pass via baseline grandfathering or bump.

## Trigger

PR #1298 grew TypedProjectionGlue.swift from 961→988 lines (already 461 LOC over the 500 hard cap), tripping the real file-size gate. Investigation then revealed 9 of 20 Wave-1 PRs had the same pattern — growing already-over-baseline files (actor/mod.rs 2482→2514, builder.rs 703→750, swift_projections_registry.rs 921→938, etc.) instead of extracting into cohesive sibling modules.

## Decision

Enforce the zero-debt doctrine as a hard merge gate: over-ceiling files must be split into sibling modules/submodules, never grown further, and .file-size-baseline bumps are prohibited. PRs that grow an already-over-limit file are blocked and sent back for extraction, not admin-merged past the gate.

## Consequences

- Wave 1b was created specifically to split 9 debt-carrying PRs before they could land
- The #1298 fix was used as the template: extract RelayDiagnostics mappers from TypedProjectionGlue.swift into TypedProjectionGlue+RelayDiagnostics.swift (961→881 + new 117-line sibling), reducing the baselined file below its baseline
- Zero baseline bumps landed across all waves
- The systematic pattern (9/20 PRs) establishes that agent codegen defaults to bolting onto existing files — future agent prompts/workflows must include explicit extraction instructions to avoid repeated Wave-1b-style remediation rounds

## Open Tail

- The 9/20 violation rate suggests agent instructions should proactively prescribe file splitting; no prompt-level fix has been applied yet
- Several baselined files remain far over 500 LOC (actor/mod.rs ~2482, TypedProjectionGlue.swift ~881) — technical debt that future work could chip away at via further extraction

## Evidence

- transcript lines 8860-8895
- transcript lines 8920-8935
- transcript lines 9186-9224
- transcript lines 9257-9269
- transcript lines 9356-9358

