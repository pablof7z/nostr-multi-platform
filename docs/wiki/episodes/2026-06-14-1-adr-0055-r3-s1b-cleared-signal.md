---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: superseded
subjects:
  - incremental-emission
  - cleared-signal
  - omit-unchanged
  - projection-manifest
supersedes:
  - 2026-06-14-1-cleared-signal-completeness-for-incremental-emission
related_claims: []
source_lines:
  - 8672-8971
captured_at: 2026-06-14T09:52:50Z
---

# Episode: ADR-0055 R3-S1b: Cleared-signal completeness for conditional projections

## Prior State

omit_unchanged iterated only typed.into_iter(), so conditionally-present projections (action_results, signed_events, action_stages, action_lifecycle) that went empty never emitted a Cleared signal. An incremental host would cache stale UI forever (spinners never dismiss, continuations replay). Also, note_copy_emit parked pending_presence=Changed on the non-empty arm, causing perpetual re-emission every 4Hz tick — a byte leak that defeated the savings the ADR exists to deliver.

## Trigger

R9 adversarial audit (#1390) found 9 confirmed findings that would silently break incremental hosts. Codex overturned the issue's own finding-7 proposed counter-bump fix. Opus review found the perpetual-Changed byte leak and its hidden dependency on ack_action_stage rev correctness.

## Decision

Three-part fix: (1) Inverse pass in omit_unchanged synthesizes payload-less Cleared rows for manifest-Cleared keys absent from typed vector. Predicate narrowed: Cleared→always synthesize; Changed+conditional-key→defensive belt; Changed-but-absent-otherwise→debug_assert+warn (never mask producer bugs). (2) note_copy_emit parks pending_presence ONLY on the Cleared edge (non-empty→empty); steady-state left to rev-vs-last-emit rule. ack_action_stage now bumps settlement_enqueue_ver so partial-ack legitimately advances rev instead of being masked by a perpetual override. (3) declare_incremental_apply FFI gate returns Result/i32 (hard error, not debug_assert).

## Consequences

- incremental_apply capability is safe to enable without stale UI risk — all four conditional keys now emit Cleared exactly once on the non-empty→empty transition
- Steady-state non-empty keys resolve to Unchanged/omitted, preserving byte savings instead of re-emitting full payload every tick
- Regression test proven to fail on master (5/6 cases), gating the rung before incremental_apply is ever flipped on
- action_stages/action_lifecycle gained their own Cleared-edge state machine (note_copy_emit), making them first-class drain-equivalents rather than relying on rev bumps
- Hard-assert on unexpected Changed-but-absent rows prevents future producer bugs from being silently masked

## Open Tail

- Finding 4 (host-side clear reorder-guard) deferred to S3 interposer
- Finding 6 (lock coalesce) acknowledged but not blocking

## Evidence

- transcript lines 8672-8971

