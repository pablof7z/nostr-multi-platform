---
type: episode-card
date: 2026-05-21
session: 4f37753c-0654-4478-9c19-e799f1b10d39
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/4f37753c-0654-4478-9c19-e799f1b10d39.jsonl
salience: root-cause
status: active
subjects:
  - chirp-tui-profile-cache
  - timeline-snapshot-shape
supersedes:
  - 2026-05-21-1-profile-resolution-routing-gap-non-author
related_claims: []
source_lines:
  - 326-434
captured_at: 2026-06-18T05:00:59Z
---

# Episode: Snapshot data model is incomplete — TUI must build client-side profile cache

## Prior State

TimelineEventCard in the snapshot was assumed to carry enough data to render a full timeline (display names, avatars, engagement counts)

## Trigger

Snapshot survey agent found that cards contain only hex pubkeys — no display_name, picture_url, nip05, reaction counts, reply counts, or repost counts. Profile data exists in kernel cache but is not exposed in the snapshot.

## Decision

TUI maintains a client-side profile resolver (pubkey → display_name, picture_url, nip05, avatar_color) that queries the kernel profile cache separately from the snapshot. Engagement metrics come from nmp-reactions domain queries.

## Consequences

- Timeline render requires a two-pass process: snapshot ingest + profile resolution
- Avatar rendering must handle missing profiles gracefully with avatar_initials + avatar_color fallback (available in kernel, not in snapshot)
- Reaction/repost/reply counts require a separate domain query — not available in the hot path without additional FFI calls
- Thread parent linkage is implicit in block structure only; no explicit reply_to tag in cards

## Open Tail

- Whether to batch profile lookups per render tick or eagerly subscribe to kind:0 events
- Whether to add profile data to snapshot projections via nmp_app_register_snapshot_projection()

## Evidence

- transcript lines 326-434

