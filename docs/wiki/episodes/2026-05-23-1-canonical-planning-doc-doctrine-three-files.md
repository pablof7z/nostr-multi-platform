---
type: episode-card
date: 2026-05-23
session: 9fc44c34-8e49-4959-91b3-714d4722ac3d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/9fc44c34-8e49-4959-91b3-714d4722ac3d.jsonl
salience: architecture
status: active
subjects:
  - planning-doctrine
  - docs-plan
  - docs-backlog
  - wip-tracker
supersedes: []
related_claims: []
source_lines:
  - 74-109
  - 163-231
  - 233-239
  - 337-359
  - 446-447
captured_at: 2026-06-18T05:13:59Z
---

# Episode: Canonical planning-doc doctrine: three files, non-overlapping roles, no duplication

## Prior State

Multiple planning documents existed with overlapping and conflicting roles. docs/plan.md was stale (milestone ladder behind the codebase). docs/BACKLOG.md incorrectly claimed to supersede WIP.md. No single file gave an accurate, current overview. Scattered notes and duplicated state across files with no explicit ownership boundaries.

## Trigger

User identified that plan.md was badly out of date and asked for a reconciliation into one current overarching plan file. User then explicitly directed that planning discipline be codified in AGENTS.md and CLAUDE.md, forbidding scattered or duplicated plan files.

## Decision

Established three canonical planning files with strict non-overlapping roles: (1) docs/plan.md — durable overview (milestones, doctrine, v1 exit criteria), (2) docs/BACKLOG.md — tactical queue (active violations, pending decisions, ordered feature backlog), (3) WIP.md — live in-flight branch tracker. Added a 'Planning discipline' section to AGENTS.md with six enforceable rules: no new top-level plan files, no duplicated state, plan files outrank scattered notes, single source of truth per fact (D4 applied to docs), edit-in-place rather than append-parallel, fewer files when in doubt. Created CLAUDE.md as a thin pointer (deliberately not duplicating content). Fixed WIP.md and BACKLOG.md to correctly reference each other instead of claiming supersession.

## Consequences

- Any future PR that introduces a new top-level plan file or duplicates state across docs will be rejected and folded back per the codified rule.
- The v1 exit checklist in plan.md now tracks three previously-untracked items from the 2026-05-23 opus review: honest cross-platform claim, bespoke-FFI deprecation calendar, snapshot serialization CI regression gate.
- New post-v1 backlog items added: Cashu wallet (NIP-60/61), Android parity (blocked on UniFFI M14), Nostr-aware UI component registry (blocked on stable snapshot projection contracts).
- WIP.md is confirmed gitignored (local-only), so its role is strictly agent workflow, not durable record.

## Open Tail

- The three untracked v1 exit items from the opus review (cross-platform honesty, FFI deprecation calendar, serialization regression gate) are noted in plan.md but not yet promoted into BACKLOG.md §4 with sequence numbers.
- plan.md's milestone ladder (M0–M17) is reconciled against actual codebase state but the underlying milestone detail files (docs/plan/m*.md) may still be stale individually.

## Evidence

- transcript lines 74-109
- transcript lines 163-231
- transcript lines 233-239
- transcript lines 337-359
- transcript lines 446-447

