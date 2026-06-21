---
type: episode-card
date: 2026-05-21
session: 19e076ce-1291-4c21-80a6-950623f0d9b8
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/19e076ce-1291-4c21-80a6-950623f0d9b8.jsonl
salience: product
status: active
subjects:
  - chirp-note-content
  - media-rendering
supersedes: []
related_claims: []
source_lines:
  - 6879-6880
captured_at: 2026-06-18T04:47:47Z
---

# Episode: Multi-image posts now render all images instead of just the first

## Prior State

NoteContentView.mediaView() only rendered urls.first, so posts with multiple attached images displayed only the first image. The Rust grouper correctly produced multi-URL media nodes (Segment::Media with multiple URLs), but the Swift thin shell discarded all but the first.

## Trigger

Agent investigation of NoteContentView.mediaView() revealed the mismatch between the Rust grouper (which collapses consecutive image URLs into one Segment::Media with multiple urls) and the Swift renderer (which called imageView(urls.first!) ignoring the rest).

## Decision

Render all URLs in a media group, not just the first. PR #203 (fix(chirp): render all images in multi-image posts) implements this.

## Consequences

- Multi-image Nostr posts now display every attached image rather than silently dropping extras.
- The thin-shell doctrine is preserved: the Rust projection still owns grouping and classification; the Swift change is purely presentational (iterating all URLs).

## Open Tail

- PR #203 is still open (not yet merged).

## Evidence

- transcript lines 6879-6880

