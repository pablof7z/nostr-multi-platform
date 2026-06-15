---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: root-cause
status: superseded
subjects:
  - d9-future-date-clamp
  - cache-serve-replay
  - observer-notify-completeness
supersedes:
  - 2026-06-15-1-unified-accepted-event-ingest-chokepoint-replaces
related_claims: []
source_lines:
  - 3002-3022
captured_at: 2026-06-15T14:44:31Z
---

# Episode: Cache-serve replay path bypasses D9 future-date clamp

## Prior State

PR 1 added the D9 future-date clamp to the live `verify_and_persist` observer-notify path, clamping `KernelEvent.created_at` to `now` in the timeline observer. The cache-serve replay path (`feed_served_event` in `cache_serve/continuation.rs:232-239`) was assumed to be covered by the same mechanism.

## Trigger

Adversarial scenario-gaps review (codex second-opinion, scenario #10) identified that `feed_served_event` builds the observer `KernelEvent` with raw `created_at` (lines 6, 22, 26, 44 of the grep output show `created_at: ev.created_at` with no clamp) and calls `notify_event_observers` — OUTSIDE `verify_and_persist`. Code inspection confirmed: `cache_serve/continuation.rs` passes raw timestamps to observers without clamping.

## Decision

D9 clamp must be uniform across ALL observer-notify sites, not just the live ingest path. The cache-serve replay path needs the same `created_at` clamping as the live path.

## Consequences

- A future-dated event served from cache after cold-restart would still warp the feed if not fixed — the PR 1 fix was incomplete
- All observer-notify sites (not just the chokepoint) must be audited for D9 compliance going forward
- PR 1b dispatched as focused fix for the cache_serve module

## Open Tail

- PR 1b (cache-serve D9 clamp) in flight — needs to land and be codex-reviewed
- Stress harness scenario 5.1 should explicitly test the cache-serve replay path with a future-dated event to prevent regression

## Evidence

- transcript lines 3002-3022
