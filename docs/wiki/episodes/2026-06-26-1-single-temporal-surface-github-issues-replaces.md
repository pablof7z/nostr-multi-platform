---
type: episode-card
date: 2026-06-26
session: 5f0cae74-2bae-4eab-b33c-978eeca433c9
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/5f0cae74-2bae-4eab-b33c-978eeca433c9.jsonl
salience: architecture
status: active
subjects:
  - temporal-tracking
  - github-issues
  - release-planning
supersedes:
  - 2026-05-23-1-canonical-planning-doc-doctrine-three-files
related_claims: []
source_lines:
  - 19-19
  - 54-58
  - 129-169
  - 200-212
  - 350-367
  - 407-420
  - 785-801
captured_at: 2026-06-26T08:08:53Z
---

# Episode: Single temporal surface: GitHub Issues replaces docs/plan.md as canonical release tracker

## Prior State

Two canonical temporal surfaces: docs/plan.md (release-plan view, milestone ladder M12–M17, forward specs) and GitHub Issues (tactical queue), explicitly coordinated in planning-discipline doctrine. Temporal facts lived in both places; code comments and build gates referenced plan.md for exit criteria and milestone status.

## Trigger

User directive: 'we've moved to track stuff as github issues — so remove that file and anything talking about it.' Team has transitioned to GitHub Issues–only workflow.

## Decision

Deleted docs/plan.md and docs/plan/ directory entirely (17 files, ~600 lines of spec). Rewrote planning-discipline doctrine in AGENTS.md and CLAUDE.md to name GitHub Issues as the sole canonical temporal and release tracker. Swept all references across 66 files, redirecting plan-file citations to GitHub Issues or durable owners (ADRs, mdk-api.md, contract-pinning tests). Captured untracked future work as high-level issues (#2121, #2122, #2124) before deletion.

## Consequences

- GitHub Issues is now the single source of truth for all temporal facts: release planning, milestone status, work queue, exit criteria (previously dual-surfaced between plan.md and Issues)
- System invariant enforced: 'single source of truth per fact' — temporal facts live ONLY in GitHub Issues or durable docs, never in time-bound plans
- Doctrine (AGENTS.md §83) now explicitly forbids creating 'docs/plan/', 'PLAN.md', 'ROADMAP.md', or per-feature plan files at repo root or under docs/
- Code comments, build assertions, and perf gates redirected from plan.md citations to GitHub Issues or durable owners (ADRs, mdk-api.md, contract tests)
- Durable understanding (architecture, product-spec, decisions) fully decoupled from temporal surfaces — no plan document survives as reference documentation
- Three new issues created to capture previously untracked future work: #2121 (v1 release train), #2122 (DX onboarding), #2124 (CI gates)

## Open Tail

*(none)*

## Evidence

- transcript lines 19-19
- transcript lines 54-58
- transcript lines 129-169
- transcript lines 200-212
- transcript lines 350-367
- transcript lines 407-420
- transcript lines 785-801

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-26-1-single-temporal-surface-github-issues-replaces.json`](transcripts/2026-06-26-1-single-temporal-surface-github-issues-replaces.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-26-1-single-temporal-surface-github-issues-replaces.json`](transcripts/raw/2026-06-26-1-single-temporal-surface-github-issues-replaces.json)
