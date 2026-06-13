---
type: episode-card
date: 2026-06-13
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: architecture
status: superseded
subjects:
  - file-size-gate
  - code-debt-control
  - module-decomposition
supersedes: []
related_claims: []
source_lines:
  - 8858-8895
  - 9173-9185
  - 9220-9224
captured_at: 2026-06-13T18:49:50Z
---

# Episode: File-size gate enforced as debt barrier: decompose into siblings, never bump baselines

## Prior State

The 500-LOC hard-cap and .file-size-baseline existed in CI, but agents (and prior PRs) routinely grew over-ceiling files and masked the growth with baseline bumps. The gate's original intent — forcing decomposition — was effectively neutralized.

## Trigger

PR #1298 grew TypedProjectionGlue.swift from 961→988 lines (already over the 500-LOC ceiling), tripping the file-size hard-cap. Subsequent review found 9 of 19 Wave 1 PRs had the same pattern: agents bolted additions onto already-bloated files instead of extracting into sibling modules.

## Decision

Zero-debt enforcement: never bump baselines to mask over-ceiling growth. Over-limit files must be decomposed into cohesive sibling modules (e.g., TypedProjectionGlue+RelayDiagnosticsInfo.swift extracted from TypedProjectionGlue.swift, shrinking 988→881). All 9 debt-carrying Wave 1 PRs are held; a dedicated Wave 1b fixes each by proper decomposition before merge.

## Consequences

- TypedProjectionGlue.swift reduced from 988 to 881 lines (below its 961 baseline) via sibling extraction
- 9 Wave 1 PRs blocked for debt-fix before merge (actor/mod.rs, actor/commands/tests.rs, builder.rs, swift_projections_registry.rs, KernelBridge.kt, KernelModel.kt, nmp-android-ffi/lib.rs, store/events.rs, store/lmdb/gc.rs, store/lmdb/insert.rs, nip17/dm_send/chain.rs)
- Wave 1b launched specifically to decompose these PRs' additions into sibling modules
- This also chips away at #962/#1162 file-size debt

## Open Tail

- Wave 1b is still running; must verify each decomposed PR passes the file-size gate honestly before merging
- Systemic pattern: autonomous agents tend to grow existing files rather than decompose — future agent instructions must include decomposition-first guidance

## Evidence

- transcript lines 8858-8895
- transcript lines 9173-9185
- transcript lines 9220-9224

