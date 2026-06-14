---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: reversal
status: superseded
subjects:
  - option-b-row-deltas
  - adr-0055
  - jank-fix-strategy
supersedes: []
related_claims: []
source_lines:
  - 10799-10824
captured_at: 2026-06-14T17:21:04Z
---

# Episode: Option B (feed row-deltas) not warranted — Rung-6-B ADR stays closed

## Prior State

Option B (feed row-deltas: only send changed rows within the feed) was on the ADR-0055 ladder as a potential follow-up after Option A (full-projection byte-equality omit) landed.

## Trigger

R6-S5 measurement and code-grounded analysis showed: (1) R3's .equatable() List boundary was already the dominant idle-jank lever — it short-circuited the expensive timeline re-render before R6 existed; (2) on a mutating frame the List must re-render anyway because a card genuinely changed; (3) new events are human-paced, not 4Hz; (4) the jank was felt on a Debug build (~17.6× slower encode than Release). Codex (gpt-5.5) concurred on all three questions.

## Decision

Option B is NOT pursued. The Rung-6-B ADR stays closed unless release-device data shows mutating-feed frames missing the frame budget while idle is already clean. Row-deltas save bytes but not the jank-relevant render path.

## Consequences

- No new ADR opened for row-deltas
- The one genuine residual (host-side decode of the retained feed each tick) is left as an open doctrine question: gating TypedHomeFeedDecoder.decode on changedKeys would conflict with D5 (bounded native state) forbidding native caches of derived values

## Open Tail

- Owner on-device Release A/B feel-test recommended (toggle incremental by commenting out nmp_app_declare_incremental_apply at KernelBridge.swift:69) to settle Debug-vs-Release attribution
- If release-device data shows mutating frames missing frame budget while idle is clean, reopen the row-deltas ADR

## Evidence

- transcript lines 10799-10824

