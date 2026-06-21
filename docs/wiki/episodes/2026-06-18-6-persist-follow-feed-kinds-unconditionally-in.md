---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: product
status: active
subjects:
  - p4-native-policy
  - follow-feed-kinds
  - open-timeline
  - nmp-core
supersedes:
  - 2026-06-18-4-persist-follow-feed-kinds-without-active
related_claims: []
source_lines:
  - 458-459
  - 943-956
captured_at: 2026-06-18T20:25:04Z
---

# Episode: Persist follow_feed_kinds unconditionally in kernel

## Prior State

The kernel dropped follow_feed_kinds when there was no active account, so the sign-in reconcile could not re-register the feed. Android imperatively called openTimeline from signInNsec/createAccount/switchAccount. iOS was already purely View-driven (openTimeline only in HomeFeedView) but the kernel bug still affected it.

## Trigger

Issue #1493 P4 Finding 1 identified a latent both-platforms no-feed-after-signin bug — the kernel was throwing away host-declared feed kinds.

## Decision

open_contact_feed now stores host-declared follow_feed_kinds UNCONDITIONALLY (even with no active account), so sign-in reconcile can re-register the feed. Android imperative openTimeline removed from signInNsec/createAccount/switchAccount. No iOS change needed — the kernel fix repairs both platforms. One PR, no unmasked intermediate state.

## Consequences

- New regression test: open_contact_feed_before_signin_persists_kinds_for_later_reconcile (declare kinds pre-account → sign in → kind:3 → REQ; would be 0 REQs under old behavior)
- P4 Findings 5/6 (web client.ts cache→wasm + chirpConfig.ts drift) deferred to post-v1 follow-up issue #1546
- Finding 4 (transport/concurrent-Intent) accepted as not-a-violation (OS constraint)

## Open Tail

- Issue #1546 filed for web: single-source web config + move ProjectionMergeCache into wasm worker

## Evidence

- transcript lines 458-459
- transcript lines 943-956

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-6-persist-follow-feed-kinds-unconditionally-in.json`](transcripts/2026-06-18-6-persist-follow-feed-kinds-unconditionally-in.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-6-persist-follow-feed-kinds-unconditionally-in.json`](transcripts/raw/2026-06-18-6-persist-follow-feed-kinds-unconditionally-in.json)
