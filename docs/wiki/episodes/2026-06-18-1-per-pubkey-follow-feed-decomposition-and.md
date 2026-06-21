---
type: episode-card
date: 2026-06-18
session: 7c780fef-d33c-4d22-bcdb-2d9ab625a4f9
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/7c780fef-d33c-4d22-bcdb-2d9ab625a4f9.jsonl
salience: architecture
status: active
subjects:
  - nmp-store-multi-author-query
  - timeline-author-limit
  - follow-feed-interest-decomposition
  - per-author-limit-1000
supersedes: []
related_claims: []
source_lines:
  - 268-273
  - 504-527
  - 528-596
  - 598-622
  - 623-649
captured_at: 2026-06-18T07:03:16Z
---

# Episode: Per-pubkey follow-feed decomposition and 500-cap are symptoms of missing multi-author store primitive

## Prior State

The follow-feed registers one LogicalInterest per followed pubkey (each with limit:Some(1000)), forcing a TIMELINE_AUTHOR_LIMIT=500 cap at parse time. StoreQuery only has AuthorKind{author:PubKey} (single-author). The merge lattice's rule5_limit refuses to merge any shape carrying a limit, so per-author interests can never coalesce. People following >500 accounts silently lose authors 501+.

## Trigger

User questioned why one cache query per pubkey is needed, pointing out the database should support multi-author queries and that REQ fragmentation should happen at a lower (outbox/compiler) layer — the per-pubkey decomposition and 500 cap are not fundamental constraints.

## Decision

Two changes, clearly layered: (1) Base/substrate layer — add StoreQuery::AuthorsKind{authors,kinds,since,until} as a multi-author scan primitive. (2) Higher/timeline layer — express the follow feed as a single InterestShape with all follows + no per-author limit, which makes TIMELINE_AUTHOR_LIMIT and per-pubkey decomposition both dead. The per-author limit:1000 is a questionable default that should simply be dropped; most REQs don't need a limit. The earlier claim that relays reject large-author filters was fabricated and retracted — no sharding rule is needed at the compiler layer.

## Consequences

- rule5_limit no longer blocks merging once per-author limit is removed — the shape {authors:[…], kinds:[…]} with no limit merges trivially or is constructed as one interest directly
- TIMELINE_AUTHOR_LIMIT=500 and capped_contact_follows become unnecessary — no fan-out to bound if there's one interest
- GitHub issue #1497 filed with category:feature, area:store+core, priority:p2, clearly separating base-layer fix (AuthorsKind) from higher-layer follow-feed restructuring
- Multi-author store query is needed regardless of timeline semantics — any consumer (thread view, search, notifications) benefits

## Open Tail

- Implement StoreQuery::AuthorsKind with k-way merge over the existing idx_author_kind LMDB secondary index
- Reconstruct follow-feed registration to use a single timeline_for(all_follows, host_kinds) interest with no limit
- Decide recency semantics for the follow feed: time-window (since) vs no limit vs global limit — now a clean isolated product decision
- Remove TIMELINE_AUTHOR_LIMIT and capped_contact_follows once the above are in place

## Evidence

- transcript lines 268-273
- transcript lines 504-527
- transcript lines 528-596
- transcript lines 598-622
- transcript lines 623-649

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-per-pubkey-follow-feed-decomposition-and.json`](transcripts/2026-06-18-1-per-pubkey-follow-feed-decomposition-and.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-per-pubkey-follow-feed-decomposition-and.json`](transcripts/raw/2026-06-18-1-per-pubkey-follow-feed-decomposition-and.json)
