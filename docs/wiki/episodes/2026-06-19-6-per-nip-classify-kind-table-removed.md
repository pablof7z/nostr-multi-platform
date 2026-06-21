---
type: episode-card
date: 2026-06-19
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - classify-kind
  - router
  - per-nip-branching
  - nip19-adapter
supersedes:
  - 2026-06-18-4-per-nip-branches-eliminated-from-generic
related_claims: []
source_lines:
  - 25-27
  - 2025-2026
captured_at: 2026-06-19T06:25:53Z
---

# Episode: Per-NIP classify_kind table removed from generic router layer

## Prior State

NIP-specific branching lived in generic layers: classify_kind tables in the router, NIP-18 repost decode in the timeline builder, NIP-19/21 codecs in nmp-core. This violated D0 (uniform architecture) and required per-NIP conditional code in platform-neutral crates.

## Trigger

P2 audit finding (D0 violation: per-NIP/per-kind branches in generic layers)

## Decision

Removed classify_kind D0 table from router. Replaced NIP-19 codec with a thin rust-nostr adapter. NIP-18 repost decode moved out of the generic timeline builder.

## Consequences

- Generic layers no longer branch on specific NIP kinds
- Two latent bugs found and fixed during the NIP-19 adapter migration
- Future NIP additions no longer require modifying router/timeline builder

## Open Tail

*(none)*

## Evidence

- transcript lines 25-27
- transcript lines 2025-2026

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-6-per-nip-classify-kind-table-removed.json`](transcripts/2026-06-19-6-per-nip-classify-kind-table-removed.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-6-per-nip-classify-kind-table-removed.json`](transcripts/raw/2026-06-19-6-per-nip-classify-kind-table-removed.json)
