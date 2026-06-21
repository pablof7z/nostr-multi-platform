---
type: episode-card
date: 2026-05-25
session: e4d33847-af62-4a40-a7f2-1a77b96605a3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/e4d33847-af62-4a40-a7f2-1a77b96605a3.jsonl
salience: reversal
status: superseded
subjects:
  - chirp-tui
  - content-rendering
  - timeline-row
supersedes:
  - 2026-05-25-1-reply-detail-view-showed-truncated-note
related_claims: []
source_lines:
  - 1-385
captured_at: 2026-06-18T05:37:32Z
---

# Episode: Stop truncating note content at storage time

## Prior State

TimelineRow.content was pre-truncated to 96 characters via content_preview() at row-construction time (timeline.rs:85), so all downstream consumers—including the detail view—rendered already-clipped text

## Trigger

User complaint: 'is chirp-tui STILL truncating the content? I hate that shit!' — confirmed by finding content_preview() called on line 85

## Decision

Store full raw content in TimelineRow.content; remove the content_preview() function from timeline.rs. Truncation now only happens at render time: post_list.rs truncates to terminal width, post_detail.rs word-wraps full text

## Consequences

- Detail view now displays full note content instead of clipped fragments
- content_preview() in timeline.rs is deleted (dead after the change)
- Dead-code warnings revealed unused tree-aware renderers: append_content_lines and prefix_line in post_detail.rs, content_preview in post_list.rs

## Open Tail

- Unused append_content_lines and prefix_line in post_detail.rs flagged but not yet cleaned up
- Unused content_preview in post_list.rs flagged but not yet cleaned up

## Evidence

- transcript lines 1-385

