---
type: episode-card
date: 2026-07-03
session: dcc80382-bcc0-45ea-8b9c-1a2fc741f872
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/dcc80382-bcc0-45ea-8b9c-1a2fc741f872.jsonl
salience: product
status: active
subjects:
  - identity-free-reads
  - viewer-parameter
  - concept-read-output-contract
supersedes: []
related_claims: []
source_lines:
  - 1323-1324
  - 1346-1348
  - 1364-1365
captured_at: 2026-07-03T09:43:37Z
---

# Episode: Identity-free concept reads: raw output only, no viewer parameter

## Prior State

The zaps agent initially added a viewer-pubkey parameter and viewer_zapped/viewer_total_msats fields to the read output, implying concept reads should derive viewer-specific state.

## Trigger

The reposts agent deliberately left 'active user reposted' to the shell, establishing the precedent that concept reads take no viewer parameter. This was then aligned across all sibling reads, including a mid-task course correction on the zaps agent to remove its viewer fields.

## Decision

All concept reads expose only raw per-actor data (reposter_pubkeys, reactor_pubkeys, zappers with per-sender totals). No viewer parameter is accepted by any open_<concept> doorway. The shell derives viewer-specific state ('did I react/zap/repost') by comparing its own active-account pubkey against the raw output. Concepts are identity-free.

## Consequences

- Consistent output contract across all four reads: raw data only, shell derives viewer state
- Concept crates have no dependency on viewer/account identity, keeping them simpler and more reusable
- Prevents concept reads from becoming viewer-aware, which would have leaked identity concerns into the read layer

## Open Tail

*(none)*

## Evidence

- transcript lines 1323-1324
- transcript lines 1346-1348
- transcript lines 1364-1365

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-07-03-3-identity-free-concept-reads-raw-output.json`](transcripts/2026-07-03-3-identity-free-concept-reads-raw-output.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-07-03-3-identity-free-concept-reads-raw-output.json`](transcripts/raw/2026-07-03-3-identity-free-concept-reads-raw-output.json)
