---
type: episode-card
date: 2026-06-03
session: cf071d35-ee9b-4a1f-a3b8-885c651e8cce
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/cf071d35-ee9b-4a1f-a3b8-885c651e8cce.jsonl
salience: architecture
status: active
subjects:
  - timeline-item
  - nmp-core
  - nmp-nip01
  - d0-violation
supersedes: []
related_claims: []
source_lines:
  - 43-68
captured_at: 2026-06-11T23:04:09Z
---

# Episode: Converge-and-delete replaces struct relocation for TimelineItem

## Prior State

TimelineItem was assumed to be simply relocated from nmp-core to nmp-nip01, preserving the struct and its producer.

## Trigger

Agent analysis (issue #920) revealed that the D0 violation is the *producer* (`views.rs::timeline_item()` computing kind:6 repost semantics and NIP-57 metadata), not the struct's address. Moving only the struct would force Layer-3 core to import a Layer-4 type to construct it — a worse inversion.

## Decision

Adopt a converge-and-delete strategy: retire the legacy `nmp-core::TimelineItem` and its kind:6-aware producer onto the already-shipped `nmp-nip01::TimelineEventCard` path, which composes NIP-18/NIP-57 as crate dependencies rather than hardcoding them.

## Consequences

- The producer `timeline_item()` must die with the struct; it cannot be left in core.
- The successor type `TimelineEventCard` becomes the single canonical timeline card.
- All consumers of the feed projection keys must migrate to the `nmp.feed.home` OP-feed path.

## Open Tail

- Full struct deletion blocked on issue #911 (frozen FFI symbols for author_view/thread_view).

## Evidence

- transcript lines 43-68

