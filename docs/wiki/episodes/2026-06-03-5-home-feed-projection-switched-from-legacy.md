---
type: episode-card
date: 2026-06-03
session: cf071d35-ee9b-4a1f-a3b8-885c651e8cce
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/cf071d35-ee9b-4a1f-a3b8-885c651e8cce.jsonl
salience: product
status: active
subjects:
  - home-feed
  - nmp-feed-home
  - projection-keys
  - ios
  - android
  - chirp-desktop
supersedes: []
related_claims: []
source_lines:
  - 1004-1047
  - 1071-1099
  - 1213-1262
  - 1376-1407
captured_at: 2026-06-11T23:04:09Z
---

# Episode: Home feed projection switched from legacy keys to nmp.feed.home OP-feed

## Prior State

Home feed data was emitted via four projection keys (`timeline`, `inserted`, `updated`, `removed`) populated by the kernel's `visible_items()`/`diff_items()` path, consumed by iOS, Android, and chirp-desktop.

## Trigger

PR #924 deleted the feed cluster from nmp-core, making all four projection keys permanently empty.

## Decision

Home feed data is now exclusively served through the typed `nmp.feed.home` OP-feed sidecar (RootIndexedFeed producing TimelineEventCard cards). All three platforms must read from this path instead of the legacy keys.

## Consequences

- iOS KernelBridge's `.items`, `.inserted`, `.updated`, `.removed` accessors are dead (removed in PR #925).
- chirp-desktop's `projection::<Vec<TimelineItem>>("timeline")` read is dead (removed in PR #927).
- Android's legacy `s.items` fallback path is dead code (removed in PR #929).
- chirp-desktop Home tab must render from the wrapped `OpFeedSnapshot` shape (`cards[].card` + `attribution`), not bare cards (wired in PR #928).
- ProfileScreen.kt on Android still reads `snapshot.items` (now empty) instead of `author_view.items` — latent bug.

## Open Tail

- Android ProfileScreen should be migrated to read author_view.items.
- chirp-desktop TODO for full nmp.feed.home card rendering.

## Evidence

- transcript lines 1004-1047
- transcript lines 1071-1099
- transcript lines 1213-1262
- transcript lines 1376-1407

