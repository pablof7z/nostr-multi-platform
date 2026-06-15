---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - cache-serve-fan-out-unification
  - d9-clamp-gap
  - profiles-ver-gap
supersedes: []
related_claims: []
source_lines:
  - 3004-3022
  - 3297-3301
  - 3309-3315
captured_at: 2026-06-15T15:14:58Z
---

# Episode: Cache-serve post-store fan-out must unify with live chokepoint

## Prior State

feed_served_event in cache_serve had its own observer-notify path that systematically diverged from the live chokepoint's post-store fan-out; it built KernelEvent with raw created_at (no D9 clamp) and performed no mailbox/dm-relay/profile transition sweep

## Trigger

Two independent findings traced to the same root cause: (1) codex adversarial review found that future-dated events served from cache after cold-restart still warp the feed because feed_served_event uses raw created_at without the D9 clamp; (2) PR 2 codex review found profiles_ver not bumped on cache-serve replay (stale UI after restart)

## Decision

Extract the live chokepoint's post-store fan-out (D9 clamp + mailbox/dm-relay/profile transition sweep + observer notify) into one shared helper used by both ingest_accepted_event and feed_served_event, kind-agnostically per ADR-0045's single-mechanism principle

## Consequences

- PR 1b (narrow D9 clamp fix) superseded — folded into the unified helper rather than patched separately
- Future capability migrations (contacts, etc.) automatically get cache-serve coverage without per-concern patching
- No more risk of the two paths silently diverging on any new post-store concern

## Open Tail

- PR 2 rework implementing this unification is in flight; must also delete the fake test writer (inject_profile) that bypasses the real verify_and_persist → dispatcher → TestKind0Parser path

## Evidence

- transcript lines 3004-3022
- transcript lines 3297-3301
- transcript lines 3309-3315
