---
type: episode-card
date: 2026-06-15
session: c9a794f6-6ad7-4ee9-a620-fc342fd495c3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/c9a794f6-6ad7-4ee9-a620-fc342fd495c3.jsonl
salience: reversal
status: active
subjects:
  - interestshape-memory-layout
  - canonical-filter-hash
  - nmp-planner-interest
supersedes:
  - 2026-06-15-1-interestshape-pubkey-type-swap-rejected-breaks
related_claims: []
source_lines:
  - 879-892
captured_at: 2026-06-15T08:45:33Z
---

# Episode: InterestShape type swap rejected as wire/storage-format break

## Prior State

Proposal 5 recommended replacing BTreeSet<String> (hex pubkeys) with Vec<[u8;32]> or BTreeSet<[u8;32]> to reduce clone/drop cost in lattice::merge.

## Trigger

Opus review found canonical_filter_hash (plan.rs:170-176) is literally stable_hash64(("canonical-filter", serde_json::to_string(shape))) — the sub_id contract depends on the exact serialized form of InterestShape.

## Decision

UNSOUND as written — reject. Changing BTreeSet<String> to BTreeSet<[u8;32]> changes the serialized bytes, churns every sub_id, and silently invalidates the watermark store (triggering full re-fetch on every device at deploy). Vec<[u8;32]> additionally breaks the sorted/deduped determinism contract. If pubkey cloning is still hot after plan memoization, the correct approach is interning (Arc<[u8;32]> or u32 handle) with a serialize_with/deserialize_with hex adapter that preserves the exact wire representation.

## Consequences

- Proposal 5 deferred — only reconsider after plan memoization proves pubkey width is still a bottleneck
- Any future InterestShape type change MUST preserve byte-identical serde output for canonical_filter_hash stability
- The 31ms lattice::merge clone cost is largely eliminated by plan-input memoization (Fix A) without touching the type at all

## Open Tail

- If interning is pursued, filter_json_for (subs/wire.rs:215) re-parses each hex Pubkey into rust-nostr PublicKey on every emit — typed keys would need threading through that path

## Evidence

- transcript lines 879-892
