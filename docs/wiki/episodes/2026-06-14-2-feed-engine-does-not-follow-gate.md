---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: active
subjects:
  - feed-engine
  - root-gating
  - over-invalidation-proof
supersedes:
  - 2026-06-14-2-feed-idle-omission-validated-at-97
related_claims: []
source_lines:
  - 10651-10709
captured_at: 2026-06-14T17:21:04Z
---

# Episode: Feed engine does NOT follow-gate roots — only replies

## Prior State

Assumption that stranger pubkeys could serve as a false-resend probe for the byte-equality gate, because they would be rejected by the follow predicate and thus never reach the engine, trivially producing byte-identical snapshots.

## Trigger

Opus review identified that stranger ROOT events pass through ingest_root regardless of follow status — only replies are follow-gated. Investigation confirmed: RootFeedSnapshot carries total_blocks (count of all roots) + has_more, so any new root legitimately changes serialized bytes (~160B measured). A stranger pubkey as the probe would pass the gate even with a broken should_emit.

## Decision

The real over-invalidation test must use a followed author's reply to a root the engine never holds: it passes the predicate, reaches the engine (Inserted → observer fires), mutates internal state (pending_attributions grows), but surfaces no card and leaves total_blocks unchanged → byte-identical → must omit. The stranger probe is retained as an informational predicate sanity check (switched to replies, which ARE follow-gated).

## Consequences

- Corrects understanding: the OP-centric feed engine surfaces ALL roots regardless of author, only follow-gates replies
- RootFeedSnapshot.total_blocks means any new root legitimately re-emits (it changes bytes), which is correct behavior not over-invalidation
- Future over-invalidation testing must use followed-author out-of-window events, not strangers
- Gate 4 now genuinely exercises the byte-equality gate as the suppressor (0/1 false resends)

## Open Tail

*(none)*

## Evidence

- transcript lines 10651-10709

