---
type: episode-card
date: 2026-05-21
session: 161ad3af-aeba-42f7-98ab-a71d2fda69a7
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/161ad3af-aeba-42f7-98ab-a71d2fda69a7.jsonl
salience: root-cause
status: active
subjects:
  - chirp-repl-render
  - timeline-event-card-wire-format
supersedes: []
related_claims: []
source_lines:
  - 1-29
  - 135-176
  - 180-208
captured_at: 2026-06-18T04:57:44Z
---

# Episode: Card author field name mismatch — renderer expected pubkey, wire format uses author_pubkey

## Prior State

The chirp-repl event renderer read the author field from JSON key `pubkey`, but TimelineEventCard serializes author identity under `author_pubkey` — causing all authors to display as `?`

## Trigger

User ran `home` and observed every card showing `author:?`; user asked why; investigation traced the serialization schema in timeline_projection.rs (line 23: `pub author_pubkey: String`) versus the renderer's `event.get("pubkey")`

## Decision

Fixed the renderer to read `author_pubkey` (the actual card wire field) with a fallback to `pubkey` for raw event objects, ensuring authors display correctly in both card and raw-event contexts

## Consequences

- Authors now render as truncated pubkey prefixes instead of `?` in the chirp-repl home feed
- Any future code consuming card JSON must use `author_pubkey`, not `pubkey` — this field name is the canonical contract

## Open Tail

- The linter race condition (file reverted between edit and build) required a chained patch-and-build approach; this workflow constraint may recur

## Evidence

- transcript lines 1-29
- transcript lines 135-176
- transcript lines 180-208

