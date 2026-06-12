---
type: episode-card
date: 2026-06-03
session: cf071d35-ee9b-4a1f-a3b8-885c651e8cce
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/cf071d35-ee9b-4a1f-a3b8-885c651e8cce.jsonl
salience: product
status: active
subjects:
  - timeline-item
  - nip-57
  - zap
  - domain-rule
supersedes: []
related_claims: []
source_lines:
  - 26-36
  - 75-76
captured_at: 2026-06-11T23:04:09Z
---

# Episode: NIP-57 zap awareness expelled from timeline projection

## Prior State

TimelineItem carried `author_lnurl` (NIP-57 zap metadata) baked into the timeline row, implying zap display is a kernel-level timeline concern.

## Trigger

User correction (line 26): 'timeline item should not have ANY nip-57 awareness!' and (line 32): 'if an app wants to show zaps in an event that's up to the app to decide!'

## Decision

Zap/lnurl data is an app-level presentation concern, not a kernel projection concern. `TimelineEventCard` carries only zap *counts* (via `note_relations.rs`), never a baked lnurl. Apps decide whether and how to render zaps.

## Consequences

- author_lnurl removed from the timeline projection domain permanently.
- NIP-57 enters timeline only as counts, never as display metadata.
- Apps that want zap rendering must derive it from their own NIP-57 subscription, not from the feed projection.

## Open Tail

*(none)*

## Evidence

- transcript lines 26-36
- transcript lines 75-76

