---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - classify-kind
  - nip19
  - d0-doctrine
  - router
supersedes: []
related_claims: []
source_lines:
  - 303-316
  - 342-368
captured_at: 2026-06-18T19:42:43Z
---

# Episode: Per-NIP branches removed from generic router layer; nip19 codec to become thin adapter

## Prior State

Router contained a per-NIP classify_kind table (NIP-54/NIP-37 entries) in a generic layer, violating D0. nmp-core/nip19.rs was a 480-line hand-rolled bech32/TLV codec duplicating functionality that nostr::nips::nip19 already provides. The router lib.rs had a doc-lie claiming only 2-6 of 7 lanes were implemented.

## Trigger

#1493 audit finding P2; codex-design-first verified the approach.

## Decision

Thread EventClass through RoutingContext from the NIP-aware caller; delete the classify_kind table. Fix lib.rs doc-lie. Remove duplicate classify_kind in test_router.rs. Rewrite nip19.rs as a thin adapter over nostr::nips::nip19 (keep NMP's public API, delegate internals, delete hand-rolled bech32/TLV). Several originally-flagged findings reclassified as stale: repost triple-path is test-only (nmp-nip18 is canonical), nip21/tags are compliant kind-agnostic codecs, longform/embed_registry already use named consts.

## Consequences

- Router is kind-agnostic; no per-NIP switches in generic layers
- test_router can no longer diverge from production routing (duplicate table removed)
- nip19 maintenance burden reduced (~15 consumers stay source-compatible)
- nmp-content kind-literal cleanup landed (PR #1529)
- Router classify_kind + doc-lie landed (PR #1533)

## Open Tail

- nip19 thin-adapter rewrite designed but deferred due to disk crisis (full nmp-core rebuild required)

## Evidence

- transcript lines 303-316
- transcript lines 342-368

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-7-per-nip-branches-removed-from-generic.json`](transcripts/2026-06-18-7-per-nip-branches-removed-from-generic.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-7-per-nip-branches-removed-from-generic.json`](transcripts/raw/2026-06-18-7-per-nip-branches-removed-from-generic.json)
