---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: root-cause
status: superseded
subjects:
  - nmp-core
  - actor-commands
  - follow-feed
supersedes:
  - 2026-06-18-9-latent-kernel-bug-follow-feed-kinds
related_claims: []
source_lines:
  - 194-412
  - 395-399
captured_at: 2026-06-18T20:12:30Z
---

# Episode: Persist follow_feed_kinds without active account

## Prior State

open_contact_feed (publish.rs:708) dropped host-declared follow_feed_kinds when no account was active, returning toast_no_account. On fresh launch, if the user opened the timeline tab before signing in, kinds were never stored; after sign-in, reconcile re-registered with EMPTY kinds → no feed. This affected both platforms (iOS HomeFeedView is also a tab, not login-gated); Android masked it with an imperative openTimeline call.

## Trigger

P4 Finding 1: removing the imperative Android openTimeline call (a native-policy violation) unmasks the latent kernel bug where kinds are lost pre-account.

## Decision

Widen P4 scope: kernel must persist host-declared follow_feed_kinds even without an active account so sign-in reconcile re-registers correctly, AND delete the imperative openTimeline call from Android, both in one PR to avoid an unmasked intermediate state.

## Consequences

- Both platforms get correct feed registration after first sign-in.
- Android no longer calls bridge.openTimeline imperatively; View layer drives it via LaunchedEffect (matching iOS).
- The kernel change is in nmp-core (publish.rs + contacts.rs) — a different lane's file, requiring cross-lane coordination.

## Open Tail

- PR #1545 is in CI; must land the kernel persist and native deletion atomically.

## Evidence

- transcript lines 194-412
- transcript lines 395-399

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-4-persist-follow-feed-kinds-without-active.json`](transcripts/2026-06-18-4-persist-follow-feed-kinds-without-active.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-4-persist-follow-feed-kinds-without-active.json`](transcripts/raw/2026-06-18-4-persist-follow-feed-kinds-without-active.json)
