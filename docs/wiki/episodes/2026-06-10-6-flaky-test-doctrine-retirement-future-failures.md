---
type: episode-card
date: 2026-06-10
session: 8db7983d-2852-4213-9b8c-43650a958e7a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/8db7983d-2852-4213-9b8c-43650a958e7a.jsonl
salience: architecture
status: active
subjects:
  - testing-doctrine
  - memory
  - flaky-tests
supersedes: []
related_claims: []
source_lines:
  - 1264-1274
captured_at: 2026-06-11T23:11:53Z
---

# Episode: Flaky-test doctrine retirement — future failures are real regressions

## Prior State

Two memory entries instructed that `executor_failure_test` and `v58 relay_worker` tests were known flakes — re-run and ignore, don't chase regressions.

## Trigger

Both flaky tests were root-caused in this session and revealed genuine production bugs (TOCTOU on queue_depth counter; edge-triggered poll-event loss). The 're-run and ignore' policy was masking real defects.

## Decision

Both memory entries rewritten: these tests failing now indicates a real regression. The flake classification is permanently retired.

## Consequences

- Future CI failures on these tests must be investigated, not dismissed as flaky
- The session's lesson — 'what everyone called a flake was a real user-visible bug' — is now encoded in project memory

## Open Tail

*(none)*

## Evidence

- transcript lines 1264-1274

