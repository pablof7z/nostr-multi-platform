---
type: episode-card
date: 2026-06-12
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: root-cause
status: active
subjects:
  - merge-safety
  - review-process
  - ram-eviction
  - nmp-core
supersedes: []
related_claims: []
source_lines:
  - 3645-3667
  - 3736-3739
captured_at: 2026-06-12T06:14:07Z
---

# Episode: Conflict-free merge silently breaks compilation — durable root cause

## Prior State

Git conflict markers were assumed to be the primary risk when merging concurrent PRs; clean merges were considered safe by default

## Trigger

PR #1100 (legacy deletion) merged cleanly into master containing just-merged #1096 (open-view pin code) — zero conflict markers, but 7 compilation errors because #1100 deleted the view-state structs that #1096's `open_view_pins()` referenced. The pin code silently became a no-op.

## Decision

Treat conflict-free merges touching overlapping modules as potentially dangerous. The reviewer's protocol: merge current master locally, compile --workspace, and test the actual merged result — not just the PR's own head. The example-compile gap (examples not built by workspace builds) was identified as a separate gap class and added to the validation playbook.

## Consequences

- Review protocol now requires local merge + full workspace build for PRs touching overlapping modules
- cargo build --workspace --examples added to validation playbook (the claimed_profiles visibility bug would have been caught)
- The ram_eviction tests were migrated (not deleted) because they guard a live invariant, establishing precedent that invariant-guarding tests survive the deletion of their original call sites

## Open Tail

*(none)*

## Evidence

- transcript lines 3645-3667
- transcript lines 3736-3739

