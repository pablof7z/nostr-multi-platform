---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: reversal
status: superseded
subjects:
  - row-deltas
  - option-b
  - feed-emission
supersedes:
  - 2026-06-14-3-option-b-feed-row-deltas-not
related_claims: []
source_lines:
  - 10794-10850
captured_at: 2026-06-14T18:12:23Z
---

# Episode: Option B (feed row-deltas) closed as not warranted

## Prior State

Option B (feed row-deltas) was on the ADR-0055 ladder as a potential follow-up to reduce mutating-frame costs after Option A (omission) landed.

## Trigger

R6-S5 investigation proved three things: (1) R3's .equatable() List boundary was already the dominant idle-jank lever, short-circuiting expensive re-renders before R6 existed; (2) the felt jank was on a Debug build with ~17.6x slower encode; (3) with feed omission ON, idle timeline re-renders stop entirely — the @Published assignment gate prevents re-render when feed is absent from changedKeys.

## Decision

Option B (row-deltas) is not warranted and the Rung-6-B ADR stays closed. Row-deltas do nothing for the idle case (already fixed by omission), and on a mutating frame the List must re-render anyway because a card genuinely changed — new events are human-paced, not 4Hz.

## Consequences

- No row-delta implementation effort needed post-v1
- The one condition that would reopen it: release-device data showing mutating-feed frames missing the frame budget while idle is already clean
- The dominant idle-jank lever was .equatable() from R3, not R6 — R6 trims the residual (FFI bytes + @Published invalidation)

## Open Tail

- Owner on-device Release A/B (comment out nmp_app_declare_incremental_apply at KernelBridge.swift:69, feel-test idle scroll) to settle Debug-vs-Release attribution

## Evidence

- transcript lines 10794-10850

