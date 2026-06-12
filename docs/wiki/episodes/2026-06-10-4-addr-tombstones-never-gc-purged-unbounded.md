---
type: episode-card
date: 2026-06-10
session: 8db7983d-2852-4213-9b8c-43650a958e7a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/8db7983d-2852-4213-9b8c-43650a958e7a.jsonl
salience: root-cause
status: active
subjects:
  - nmp-store
  - gc
  - addr-tombstones
  - parameterized-replaceable
supersedes: []
related_claims: []
source_lines:
  - 1039-1057
captured_at: 2026-06-11T23:11:53Z
---

# Episode: addr_tombstones never GC-purged — unbounded store growth

## Prior State

Both `lmdb/gc.rs` and `mem/gc.rs` Phase 3 iterated only `inner.tombstones` / `st.tombstones`. The `addr_tombstones` table (written by kind:5 `a`-tag deletes, read as an insert gate for parameterized-replaceable events) was never iterated in GC.

## Trigger

Audit finding S-2 (HIGH): addr_tombstones accumulated without bound in both store backends.

## Decision

Added Phase 3b to both backends: purge stale `addr_tombstones` after 90 days. Safety analysis: a legitimate new version has `created_at > deleted_at` and bypasses the gate regardless; purging only allows stale copies to re-enter — same risk class the per-id tombstone age policy already accepts. `GcReport` extended with `addr_tombstones_purged: usize` (non-breaking, default 0).

## Consequences

- Store growth for `addr_tombstones` is now bounded
- LMDB test suite requires adequate disk headroom for the 32 GiB mmap (pre-existing environment constraint)

## Open Tail

*(none)*

## Evidence

- transcript lines 1039-1057

