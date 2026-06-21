---
type: episode-card
date: 2026-05-26
session: e3b42d41-ffd2-44b3-9e5a-93832feb46e0
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/e3b42d41-ffd2-44b3-9e5a-93832feb46e0.jsonl
salience: product
status: active
subjects:
  - chirp-tui-timeline-content
  - content-preview-removal
supersedes:
  - 2026-05-25-1-stop-truncating-note-content-at-storage
related_claims: []
source_lines:
  - 88-98
  - 152-156
  - 254-262
  - 479-491
captured_at: 2026-06-18T05:44:10Z
---

# Episode: Timeline content no longer truncated to preview

## Prior State

TimelineRow.content held a truncated preview (max 96 chars) produced by the content_preview() function, discarding the full note text

## Trigger

Local fix commit (e1a6838a) showed content loss; after hard-reset to origin, the change was deliberately re-applied, confirming the product decision

## Decision

Store the full content string in TimelineRow.content and delete the content_preview() function entirely

## Consequences

- Downstream PR #576 conflict resolution deliberately excluded re-introduction of content_preview
- TimelineRow.content is now the authoritative full text, shifting rendering responsibility to the UI layer
- Any future truncation must happen at render time, not at data-construction time

## Open Tail

*(none)*

## Evidence

- transcript lines 88-98
- transcript lines 152-156
- transcript lines 254-262
- transcript lines 479-491

