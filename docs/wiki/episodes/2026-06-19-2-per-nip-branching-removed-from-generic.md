---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: active
subjects:
  - kind-routing
  - nip19-adapter
  - classify-kind
supersedes:
  - 2026-06-19-6-per-nip-classify-kind-table-removed
related_claims: []
source_lines:
  - 26-27
  - 2025-2026
captured_at: 2026-06-19T11:51:39Z
---

# Episode: Per-NIP branching removed from generic layers

## Prior State

NIP-specific decode/branch logic scattered through generic layers: router classify_kind table, timeline builder NIP-18 repost handling, nip19/21 codecs living in nmp-core.

## Trigger

#1493 audit P2 finding: per-NIP/per-kind branches in generic layers violate D0.

## Decision

classify_kind D0 table removed from router; nip19 moved to thin rust-nostr adapter; nip29 kind resolution unified through the proper seam.

## Consequences

- 2 latent bugs found in the nip19 adapter during migration
- 3 PRs merged (#1529, #1533, #1542)
- Generic layers are now NIP-agnostic; kind-specific logic lives in proper protocol crates

## Open Tail

*(none)*

## Evidence

- transcript lines 26-27
- transcript lines 2025-2026

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-2-per-nip-branching-removed-from-generic.json`](transcripts/2026-06-19-2-per-nip-branching-removed-from-generic.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-2-per-nip-branching-removed-from-generic.json`](transcripts/raw/2026-06-19-2-per-nip-branching-removed-from-generic.json)
