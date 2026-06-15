---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: root-cause
status: superseded
subjects:
  - cache-serve-d9-clamp
  - feed-served-event
supersedes:
  - 2026-06-15-1-unified-ingest-chokepoint-with-admission-projection
related_claims: []
source_lines:
  - 3004-3022
captured_at: 2026-06-15T15:03:56Z
---

# Episode: Cache-serve D9 clamp gap — future-date warp on replay

## Prior State

PR 1 landed with the D9 future-date clamp applied in the live verify_and_persist observer-notify path. The cache-serve replay path (feed_served_event in continuation.rs) was assumed to inherit the same protection.

## Trigger

Codex adversarial review found that feed_served_event builds the observer KernelEvent with raw created_at and calls notify_event_observers without clamping — a future-dated event served from cache after cold-restart would still warp the feed to the top.

## Decision

D9 clamp must be uniform across ALL observer-notify sites, including the cache-serve replay path. PR 1b fix dispatched to clamp created_at in feed_served_event before observer notification.

## Consequences

- Real bug in landed PR 1 code caught before harness ran
- Validates the adversarial review strategy — the cache-serve path was architecturally separate from the ingest chokepoint and easy to miss

## Open Tail

- PR 1b implementation still in flight

## Evidence

- transcript lines 3004-3022
