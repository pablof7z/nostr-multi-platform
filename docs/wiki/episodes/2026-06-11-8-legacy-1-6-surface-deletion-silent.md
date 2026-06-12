---
type: episode-card
date: 2026-06-11
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: architecture
status: active
subjects:
  - legacy-deletion
  - symbol-retirement
  - ram-eviction
  - merge-safety
supersedes: []
related_claims: []
source_lines:
  - 3640-3674
captured_at: 2026-06-11T23:31:21Z
---

# Episode: Legacy {1,6} surface deletion — silent merge-break caught

## Prior State

`author_view` and `thread_view` (symbols {1,6}) and their parallel state machines existed as legacy code to be retired per ADR-0042.

## Trigger

ADR-0042 requirement; #958/#957 to delete the legacy surfaces.

## Decision

Delete 5,750 LOC across 66 files. Reviewer caught worst-case interaction: git would merge #1100 with zero conflict markers into #1096's pin code, but `ram_eviction.rs::open_view_pins()` references the deleted `thread_view.selected_thread`, `author_view.selected_author`, etc. — silently breaking compilation in 7 places and killing open-view pinning. Correct fix: re-derive from `lifecycle.registry().iter_active()` + `shape.matches_event_with_id`.

## Consequences

- Legacy state machines removed, reducing complexity
- Pin derivation must be re-derived after this merge — the reviewer's pre-validated solution routes through the interest registry instead of deleted view state
- Registry counts must change from 36/30 to 34/28 (author_view+thread_view removed)
- CHANGELOG needs `### Removed (BREAKING)` entry for v0.3.1 C-ABI break

## Open Tail

- Fix round must: re-derive pins, migrate 4 ram_eviction tests to `open_interest`, delete gallery caller, update registry counts, add CHANGELOG breaking entry

## Evidence

- transcript lines 3640-3674

