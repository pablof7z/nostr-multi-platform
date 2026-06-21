---
type: episode-card
date: 2026-05-25
session: b48d81e1-411c-45db-a440-340bcaee2631
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/b48d81e1-411c-45db-a440-340bcaee2631.jsonl
salience: product
status: superseded
subjects:
  - chirp-tui
  - timeline-row
  - content-preview
supersedes: []
related_claims: []
source_lines:
  - 1-311
captured_at: 2026-06-18T05:31:04Z
---

# Episode: Reply/detail view showed truncated note content

## Prior State

TimelineRow.content was stored at construction time as a 95-char-capped, ellipsis-appended preview via content_preview(). The detail/reply view (post_detail.rs) word-wrapped correctly but operated on already-truncated data, so replies were cut off.

## Trigger

User reported that chirp-tui was truncating messages in replies and stated it shouldn't.

## Decision

Store full raw content in TimelineRow.content instead of content_preview(&content). Removed the now-dead content_preview function entirely.

## Consequences

- Detail/reply view now displays full note content with proper word-wrapping
- List view is unaffected because post_list.rs already truncates to terminal width at render time
- Pre-existing unrelated test failure (help_overlay_renders_keybindings) confirmed not caused by this change

## Open Tail

*(none)*

## Evidence

- transcript lines 1-311

