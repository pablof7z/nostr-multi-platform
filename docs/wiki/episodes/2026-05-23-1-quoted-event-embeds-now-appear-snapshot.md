---
type: episode-card
date: 2026-05-23
session: c5325e71-7d4e-451e-8c15-81cdae440f5f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/c5325e71-7d4e-451e-8c15-81cdae440f5f.jsonl
salience: root-cause
status: active
subjects:
  - chirp-event-embed
  - modular-timeline-snapshot
  - chirpSnapshot-refresh
supersedes: []
related_claims: []
source_lines:
  - 1-3
  - 595-597
  - 640-676
captured_at: 2026-06-18T05:14:29Z
---

# Episode: Quoted-event embeds now appear — snapshot refresh guard was too narrow

## Prior State

chirpSnapshot() was only called when items changed (guard: if update.items != priorItems). This was a performance optimization that assumed cards only grow alongside item changes.

## Trigger

User reported that event embeds (quoted posts) were not rendering — the referenced event's card was never delivered to Swift. Root-cause: discovery oneshots insert referenced events into the projection's cards map without changing items (the followed-author list), so the snapshot was never refreshed and the card never reached the UI.

## Decision

Removed the items-change guard; chirpSnapshot() now runs every tick. The existing equality check (nextTimeline != modularTimeline) still prevents spurious SwiftUI re-renders, so the optimization is partially preserved.

## Consequences

- Quoted/referenced events now appear in note embeds as soon as their discovery oneshot resolves
- Snapshot serialization cost is now paid every tick rather than only on item mutations, but view-layer re-renders remain gated by the struct equality check
- The assumption that cards only grow with items is now documented as incorrect in the code comment

## Open Tail

*(none)*

## Evidence

- transcript lines 1-3
- transcript lines 595-597
- transcript lines 640-676

