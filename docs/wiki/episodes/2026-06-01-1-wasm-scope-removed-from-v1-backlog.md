---
type: episode-card
date: 2026-06-01
session: bbd5fe79-cd71-4de0-ba9f-f3684520a03f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/bbd5fe79-cd71-4de0-ba9f-f3684520a03f.jsonl
salience: reversal
status: active
subjects:
  - wasm-v1-scope
  - backlog-prioritization
supersedes: []
related_claims: []
source_lines:
  - 46-67
captured_at: 2026-06-11T22:55:24Z
---

# Episode: WASM scope removed from v1 backlog

## Prior State

F-01 (IndexedDB persistence) and F-06 (wasm cross-platform claim) were on the v1 feature backlog in Section 4; wasm references also appeared in V-51 Phase 3, F-09 (chirp-web), and F-10 acceptance criteria

## Trigger

User directive: 'f-01.. wasm is not for v1 -- so reprioritize anything on backlog about wasm to be on backlog-v2.md -- let's start cutting down the crap we have on backlog.md'

## Decision

Removed F-01 and F-06 entirely from Section 4; stripped wasm legs from V-51 Phase 3, chirp-web from F-09, and wasm from F-10 acceptance; all demoted items consolidated into existing Section 5 (post-v1) rather than creating a separate backlog-v2.md, citing single-source-of-truth rule for BACKLOG.md

## Consequences

- v1 scope no longer includes any WASM/browser deliverables
- docs/plan.md exit criterion #6 still references F-06 and will need updating when the wasm decision is eventually revisited
- Section 5 of BACKLOG.md is now the canonical post-v1 bucket — no parallel backlog file created

## Open Tail

- docs/plan.md exit criterion #6 references F-06 and needs updating

## Evidence

- transcript lines 46-67

