---
type: episode-card
date: 2026-07-03
session: dcc80382-bcc0-45ea-8b9c-1a2fc741f872
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/dcc80382-bcc0-45ea-8b9c-1a2fc741f872.jsonl
salience: root-cause
status: active
subjects:
  - dependent-demand
  - readspec-limitation
  - nip09-retraction-routing
  - engine-gap
supersedes: []
related_claims: []
source_lines:
  - 1333-1334
  - 1350-1351
  - 1371-1372
captured_at: 2026-07-03T09:43:37Z
---

# Episode: Static ReadSpec cannot route NIP-09 retractions of not-yet-seen events

## Prior State

Static demand set at open_read time was assumed sufficient for all concept reads — the demand filters are fixed when the read is opened and the engine subscribes to exactly that set.

## Trigger

The reposts concept agent discovered during implementation that NIP-09 kind:5 deletions name the deleted event's own id, not the target's. A repost wrapper's id is only known once observed live, so a static ReadSpec (fixed demand set at open_read time) cannot guarantee routing a stranger's later retraction of an as-yet-unseen wrapper to the reducer. The agent refused to write a private re-subscription loop (forbidden lifecycle code) and documented the gap instead.

## Decision

Filed as engine-owned 'dependent demand' capability (#2818, p1): the engine needs a mechanism to dynamically add demand for events it learns about during the read's lifetime (prior art: kernel's dependent-interest owner that the feed engine already uses). All four concept crates named for same-pass upgrade — including nmp-replies, which has the identical gap silently (it doesn't attempt reply-deletion handling at all).

## Consequences

- Boundary test worked as designed: concept agent hit the wall, refused to work around it, gap flows back to engine
- #2818 was closed (resolved), meaning the dependent-demand capability was added to the engine
- nmp-replies' silent gap (no deletion handling) is now explicitly tracked rather than hidden
- Zap receipts confirmed not affected: receipt author is the LN provider, not the sender/target, so there's no self-retraction relationship

## Open Tail

*(none)*

## Evidence

- transcript lines 1333-1334
- transcript lines 1350-1351
- transcript lines 1371-1372

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-07-03-4-static-readspec-cannot-route-nip-09.json`](transcripts/2026-07-03-4-static-readspec-cannot-route-nip-09.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-07-03-4-static-readspec-cannot-route-nip-09.json`](transcripts/raw/2026-07-03-4-static-readspec-cannot-route-nip-09.json)
