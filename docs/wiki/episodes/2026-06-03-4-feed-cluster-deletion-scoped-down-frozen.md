---
type: episode-card
date: 2026-06-03
session: cf071d35-ee9b-4a1f-a3b8-885c651e8cce
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/cf071d35-ee9b-4a1f-a3b8-885c651e8cce.jsonl
salience: root-cause
status: active
subjects:
  - timeline-item
  - ffi-freeze
  - author-view
  - thread-view
supersedes: []
related_claims: []
source_lines:
  - 860-919
captured_at: 2026-06-11T23:04:09Z
---

# Episode: Feed cluster deletion scoped down — frozen FFI blocks full struct removal

## Prior State

Issue #920 Step 3 was to 'delete TimelineItem from nmp-core with no replacement type.'

## Trigger

Step 3 agent discovered TimelineItem feeds three producer clusters (feed, author_view via `nmp_app_open_author`, thread_view via `nmp_app_open_thread`), not just the feed cluster. The single constructor `timeline_item()` is shared by all three. Full deletion would orphan frozen C-ABI symbols with live iOS/Android callers — out of scope and blocked on issue #911's `nmp_app_open_interest` ADR.

## Decision

Adopt Option A: delete only the feed cluster (visible_items, diff_items, 4 projection keys, last_emitted_items) while preserving `timeline_item()`, `TimelineItem` struct, author_view, and thread_view until #911 resolves the frozen FFI symbols.

## Consequences

- TimelineItem struct and timeline_item() producer remain in nmp-core as internal carriers for author_view/thread_view.
- The feed projection keys (timeline/inserted/updated/removed) are permanently gone from nmp-core.
- iOS/Android consumers of the now-empty feed keys will receive empty data until they migrate to nmp.feed.home.
- Full struct deletion is blocked on the nmp_app_open_interest ADR (issue #911).

## Open Tail

- Issue #911 and the open_interest ADR must be completed before TimelineItem can be fully removed.

## Evidence

- transcript lines 860-919

