---
type: episode-card
date: 2026-06-29
session: 898a41b5-68e0-4b0f-b16c-c6072454bd6a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/898a41b5-68e0-4b0f-b16c-c6072454bd6a.jsonl
salience: architecture
status: active
subjects:
  - perf-gates
  - ci-gates
  - clean-break-migration
supersedes: []
related_claims: []
source_lines:
  - 503-525
  - 618-702
  - 746-751
captured_at: 2026-06-29T09:49:41Z
---

# Episode: Disable perf-gates during clean-break migration

## Prior State

All CI gates (doctrine-lint, codegen-drift, test, browser-runtime, supply-chain, file-size, perf-gates) are mandatory PR blockers, enforcing the 'ship the right thing, then lock the door' doctrine during rapid refactor.

## Trigger

User raises concern that CI friction is impeding agent work during active migration. Assistant analyzes gate universe and identifies perf-gates as a hygiene gate (transient regression during refactor is expected) distinct from invariant gates (structural boundaries of the refactor itself).

## Decision

Disable perf-gates by converting workflow trigger from auto (push/pull_request) to manual-only (workflow_dispatch) during clean-break migration phase. Invariant gates (doctrine-lint, codegen-drift, test, browser-runtime, supply-chain, doc ratchets) remain strict. File-size remains strict. Perf-gates available for manual runs and post-v1 re-enablement.

## Consequences

- Agents unblocked from perf-regression holds while merging WRITE-door (#2371, #2372) and M14 PRs (#2389, #2397), accelerating critical path to v1
- Doctrine distinction established in code: invariant gates (structural to refactor) vs. hygiene gates (safe to relax during active refactor phases)
- Pattern created for future high-velocity development phases: distinguish structural constraints from transient-regression concerns
- Perf measurement capability remains available; gate can be restored post-migration without losing measurement infrastructure

## Open Tail

- Perf-gates re-enablement timing and policy post-v1 not yet documented
- File-size gate still flags #2391; gate relaxation on perf-gates does not affect file-size constraint, which still requires scope split

## Evidence

- transcript lines 503-525
- transcript lines 618-702
- transcript lines 746-751

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-29-1-disable-perf-gates-during-clean-break.json`](transcripts/2026-06-29-1-disable-perf-gates-during-clean-break.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-29-1-disable-perf-gates-during-clean-break.json`](transcripts/raw/2026-06-29-1-disable-perf-gates-during-clean-break.json)
