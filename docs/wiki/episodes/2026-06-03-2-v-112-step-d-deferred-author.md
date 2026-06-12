---
type: episode-card
date: 2026-06-03
session: f1b740a8-d601-4b63-8633-072c83a6de22
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/f1b740a8-d601-4b63-8633-072c83a6de22.jsonl
salience: reversal
status: active
subjects:
  - v-112
  - flatfeed
  - author-view
  - thread-view
  - open-author
supersedes: []
related_claims: []
source_lines:
  - 3917-3947
captured_at: 2026-06-11T23:05:43Z
---

# Episode: V-112 Step D deferred: author_view/thread_view carry unreplaced display fields

## Prior State

Plan was to migrate Swift screens to FlatFeed and then immediately delete the legacy `open_author`/`open_thread` machinery (Step D delete cascade)

## Trigger

Code investigation by V-112 agent found that `author_view`/`thread_view` carry Rust-authored display fields (`primary_action`, `note_count_display`, thread `previous/next_count_label`, `focused_event_id`) that FlatFeed does not produce; relay backfill not working in test environment proved the cutover would blank screens

## Decision

Defer the delete cascade; ship only the FlatFeed decode infrastructure (Step C as PR #941 — `extractFeedProjections`/`FeedProjectionKey`, `authorFeed`/`threadFeed` accessors, 4 C-ABI wrappers); do NOT cutover screens until Rust survivor fields and deterministic relay backfill are verified

## Consequences

- Legacy `open_author`/`open_thread` machinery stays alive indefinitely
- A Rust-side survivor must be built in `nmp-app-chirp` for `primary_action`/counts before Step D can proceed
- Relay backfill must be verified with a seeded `nak serve` before any screen cutover
- iOS build was NOT environmentally blocked — the 'blocked' belief was self-inflicted xcodegen pbxproj churn (reverted; sim build succeeds)

## Open Tail

- Verify flat-feed delivery with seeded `nak serve` (removes public-relay flakiness)
- If green, take the one-line ProfileView cutover
- Build Rust survivor in `nmp-app-chirp` for `primary_action`/counts, then run Step D delete cascade
- PR #941 still needs CI green before merge

## Evidence

- transcript lines 3917-3947

