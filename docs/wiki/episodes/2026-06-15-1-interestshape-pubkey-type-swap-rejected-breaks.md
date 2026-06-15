---
type: episode-card
date: 2026-06-15
session: c9a794f6-6ad7-4ee9-a620-fc342fd495c3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/c9a794f6-6ad7-4ee9-a620-fc342fd495c3.jsonl
salience: reversal
status: superseded
subjects:
  - interestshape-pubkey-type
  - canonical-filter-hash-stability
  - watermark-store-invalidation
supersedes:
  - 2026-06-15-2-interestshape-type-swap-rejected-breaks-canonical
related_claims: []
source_lines:
  - 879-891
captured_at: 2026-06-15T08:33:24Z
---

# Episode: InterestShape Pubkey type swap rejected — breaks canonical_filter_hash wire contract

## Prior State

Proposal 5 suggested swapping BTreeSet<String> (hex pubkeys) to BTreeSet<[u8;32]> or Vec<[u8;32]> inside InterestShape as a performance optimization to reduce clone/drop cost in lattice::merge.

## Trigger

Opus architectural review found that canonical_filter_hash (plan.rs:170) is stable_hash64(serde_json::to_string(shape)). Changing the authors field type changes the serialized bytes, which churns every sub_id and silently invalidates the watermark store. Additionally, Vec<[u8;32]> would break the sorted/deduped determinism contract required for plan-id stability.

## Decision

Reject the naive BTreeSet<String>→BTreeSet/[Vec]<[u8;32]> swap outright. If pubkey cloning is still hot after plan memoization lands, the correct approach is interning (Arc<[u8;32]> or u32 handle) with a serialize_with/deserialize_with hex adapter that preserves the exact serde wire representation.

## Consequences

- Proposal 5 deferred until Proposal 2's memoization proves pubkey width is still a bottleneck
- Any future InterestShape memory layout change must preserve serde representation byte-for-byte to avoid churning sub_ids and invalidating the watermark store
- Vec<[u8;32]> is permanently inadmissible as a container — breaks sorted/dedup determinism contract (interest.rs:99-103)

## Open Tail

- If interning is pursued later, filter_json_for (wire.rs:215) re-parses hex Pubkey into rust-nostr PublicKey on every emit — typed keys would need to thread through that path too

## Evidence

- transcript lines 879-891
