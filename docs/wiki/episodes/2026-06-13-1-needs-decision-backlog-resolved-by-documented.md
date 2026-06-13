---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: architecture
status: active
subjects:
  - needs-decision-backlog
  - decision-framework
supersedes:
  - 2026-06-13-4-10-of-11-needs-decision-issues
related_claims: []
source_lines:
  - 8084-8158
captured_at: 2026-06-13T20:04:54Z
---

# Episode: Needs-decision backlog resolved by documented direction (10/11)

## Prior State

11 issues labeled status:needs-decision, all assumed to require individual owner input before implementation could proceed

## Trigger

Owner instructed to run all 11 needs-decision issues by a single Opus agent against the documented product direction, betting most were already implied by aim.md, doctrine, plan.md, and ADRs

## Decision

10 of 11 issues determined by existing documented direction without new owner input; only #1281 genuinely requires an owner call. Implementation dispatched immediately for the determined ones (#1090, #1202, #1250, #1283); stale needs-decision labels cleaned on #1008, #999, #967, #980, #920.

## Consequences

- Validates that documented direction (aim.md, doctrine, plan.md) is sufficient to resolve most architectural decisions without owner escalation
- 5 implementation PRs started immediately from previously-blocked issues
- Only #1281's backfill-semantics question genuinely needed owner voice
- #1283 and #920 identified as sharing the same architectural pattern (resolve protocol logic above kernel, ship typed)

## Open Tail

- #1283 (3-PR EmbedHost migration) and #1090 (floor-coherent eviction) still in-flight
- #920 needs a status:staged migration plan when scheduled post-v1

## Evidence

- transcript lines 8084-8158

