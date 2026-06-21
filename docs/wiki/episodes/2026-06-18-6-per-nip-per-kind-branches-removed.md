---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-content
  - nmp-router
  - nmp-core
  - nip19
supersedes:
  - 2026-06-18-7-per-nip-branches-removed-from-generic
related_claims: []
source_lines:
  - 26-27
  - 302-316
  - 342-367
captured_at: 2026-06-18T20:12:30Z
---

# Episode: Per-NIP/per-kind branches removed from generic layers (D0)

## Prior State

Generic layers contained per-NIP/per-kind branches: nmp-content had bare kind literals (30023, etc.) in mode/dispatch code; the router had a duplicate classify_kind table that could diverge from the canonical one; nmp-core/nip19.rs hand-rolled bech32/TLV internals instead of delegating to the nostr crate.

## Trigger

Issue #1493 audit (P2) identified per-NIP branching as a D0 violation. Several findings were verified stale (repost triple-path is test-only; nip21/tags are compliant codecs; longform/embed_registry already use named consts).

## Decision

Removed classify_kind from the generic router (including a duplicate in test_router.rs that could diverge from prod). Replaced bare kind literals in nmp-content with named constants. nip19 adapter (delegating to nostr::nips::nip19) designed but deferred on disk crisis.

## Consequences

- Router routing is now kind-agnostic; no per-NIP classification table in the generic layer.
- test_router.rs no longer has a shadow classify_kind that could diverge from prod.
- nmp-content uses named kind constants instead of bare literals.
- nip19 thin-adapter (same public API, delegate to nostr crate) designed but not yet implemented — deferred due to disk crisis.

## Open Tail

- nip19 thin adapter is designed but unimplemented; branch fix/1493-p2-nip19-adapter exists at origin/master.

## Evidence

- transcript lines 26-27
- transcript lines 302-316
- transcript lines 342-367

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-6-per-nip-per-kind-branches-removed.json`](transcripts/2026-06-18-6-per-nip-per-kind-branches-removed.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-6-per-nip-per-kind-branches-removed.json`](transcripts/raw/2026-06-18-6-per-nip-per-kind-branches-removed.json)
