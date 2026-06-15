---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - d9-clamp-relocation
  - hostile-future-timestamp-protection
supersedes: []
related_claims: []
source_lines:
  - 2182-2184
  - 2197-2209
  - 2269-2278
captured_at: 2026-06-15T13:54:27Z
---

# Episode: D9 timestamp clamp relocated to chokepoint observer fan-out

## Prior State

The D9 `created_at` future-to-now clamp was applied only in the timeline observer fan-out. The chokepoint emitted raw `KernelEvent` with unclamped timestamps to all observers.

## Trigger

Codex review caught a regression in the initial PR 1 implementation: after moving the D9 clamp to the read-cache only (while observers fired raw), a hostile future-dated event's raw `created_at` would pin it to the top of every app feed — `nmp-feed/src/types.rs` orders cursors by `KernelEvent.created_at`, fed from the observer. This was flagged as a BLOCKER before landing.

## Decision

Apply the D9 future→now clamp on the observer-delivered `KernelEvent.created_at` at the single `notify_event_observers` site inside `verify_and_persist` (the chokepoint). Also retain the read-cache clamp in `project_timeline_event` (strictly stronger — clamps the kernel's own timeline ordering too). The authoritative `EventStore` row retains the original wire timestamp for protocol correctness. ADR-0057 §D9 rewritten to match.

## Consequences

- All feed consumers (timeline and non-timeline) are protected from hostile future-dated events pinning to the top
- The store retains raw wire timestamps for NIP-01 replaceable/ephemeral handling and protocol correctness
- Past-dated events pass through unchanged (`min(wire, now)`)
- Auto-compiled wiki page `kernel-timestamp-clamp.md` has a stale 'where' clause — deliberately not hand-edited to avoid fighting the wiki compiler; ADR-0057 is the authoritative source

## Open Tail

- Wiki compiler will refresh `kernel-timestamp-clamp.md` from corrected sessions; verify eventual consistency

## Evidence

- transcript lines 2182-2184
- transcript lines 2197-2209
- transcript lines 2269-2278
