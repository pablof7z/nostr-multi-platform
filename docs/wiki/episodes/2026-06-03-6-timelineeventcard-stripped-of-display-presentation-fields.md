---
type: episode-card
date: 2026-06-03
session: cf071d35-ee9b-4a1f-a3b8-885c651e8cce
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/cf071d35-ee9b-4a1f-a3b8-885c651e8cce.jsonl
salience: architecture
status: active
subjects:
  - timeline-event-card
  - nmp-nip01
  - display-concerns
supersedes: []
related_claims: []
source_lines:
  - 81-88
  - 955-962
captured_at: 2026-06-11T23:04:09Z
---

# Episode: TimelineEventCard stripped of display/presentation fields

## Prior State

TimelineEventCard carried display-oriented fields: author_display (composed name), author_display_name, author_picture_url, content_preview, content_render, and RepostAttribution with full author metadata.

## Trigger

Design phase of #922: display/presentation fields are app-level concerns (D8), not protocol-level concerns. The kernel should not compose display names or render content previews.

## Decision

Strip display fields from TimelineEventCard. author_display, author_display_name, author_picture_url, content_preview, content_render removed. RepostAttribution reduced to raw author_pubkey + note_created_at only. Display names resolve via the `resolved_profiles` projection at the app layer.

## Consequences

- Apps must resolve display names from `resolved_profiles` (BTreeMap<String, ProfileCard>) rather than receiving pre-composed names.
- Content rendering is an app responsibility, not projected by the kernel.
- TimelineEventCard is now a pure protocol/data type with no presentation logic.

## Open Tail

*(none)*

## Evidence

- transcript lines 81-88
- transcript lines 955-962

