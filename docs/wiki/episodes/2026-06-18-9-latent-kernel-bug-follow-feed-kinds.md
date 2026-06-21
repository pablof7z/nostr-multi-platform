---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: root-cause
status: superseded
subjects:
  - follow-feed-kinds
  - open-contact-feed
  - kernel-bug
supersedes: []
related_claims: []
source_lines:
  - 331-332
  - 453-462
captured_at: 2026-06-18T19:42:43Z
---

# Episode: Latent kernel bug: follow_feed_kinds dropped when no account active, masked by Android openTimeline

## Prior State

Android's `bridge.openTimeline()` call after sign-in masked a kernel bug: `open_contact_feed` in publish.rs drops the host-declared `follow_feed_kinds` when no account is active (returns `toast_no_account`). Fresh launch → user opens timeline tab first (openTimeline with no account = kinds NOT stored) → signs in → `reconcile_follow_feed_after_identity_change` re-registers with EMPTY kinds → no feed. This affects both platforms (iOS HomeFeedView is also a tab, not login-gated).

## Trigger

P4 investigation of Finding 1 (Android post-identity openTimeline) revealed the underlying kernel bug that the imperative call was masking.

## Decision

Widen P4 scope to fix the kernel persistence of follow_feed_kinds + the native openTimeline deletion in one PR, avoiding an unmasked intermediate state where the bug is exposed but not fixed.

## Consequences

- Both platforms will correctly receive feed content after sign-in without relying on an imperative openTimeline call
- No intermediate state where the native deletion unmasks the kernel bug
- P4 Finding 1 native deletion ready to ship the moment the kernel seam lands

## Open Tail

- Kernel seam (persist follow_feed_kinds pre-account) and native deletion not yet implemented

## Evidence

- transcript lines 331-332
- transcript lines 453-462

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-9-latent-kernel-bug-follow-feed-kinds.json`](transcripts/2026-06-18-9-latent-kernel-bug-follow-feed-kinds.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-9-latent-kernel-bug-follow-feed-kinds.json`](transcripts/raw/2026-06-18-9-latent-kernel-bug-follow-feed-kinds.json)
