---
type: episode-card
date: 2026-06-15
session: c9a794f6-6ad7-4ee9-a620-fc342fd495c3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/c9a794f6-6ad7-4ee9-a620-fc342fd495c3.jsonl
salience: architecture
status: superseded
subjects:
  - interestshape
  - nmp-planner-interest
  - canonical-filter-hash
  - watermark-store
supersedes: []
related_claims: []
source_lines:
  - 879-891
captured_at: 2026-06-15T08:00:37Z
---

# Episode: InterestShape type swap rejected: breaks canonical_filter_hash wire stability

## Prior State

Proposal 5 suggested replacing BTreeSet<String> (hex pubkeys) with Vec<[u8;32]> or BTreeSet<[u8;32]> in InterestShape to reduce clone/drop cost in lattice::merge.

## Trigger

Opus review found canonical_filter_hash (plan.rs:170) is stable_hash64(serde_json::to_string(shape)). Changing the container type changes the serialized bytes, which churns every sub_id and invalidates the watermark store. Vec<[u8;32]> also breaks the sorted/deduped determinism contract (interest.rs:99-103).

## Decision

Proposal 5 rejected as written. If pursued at all after memoization, must use interning (Arc<[u8;32]> or u32 handle) with serialize_with/deserialize_with hex adapter that preserves the exact serialized form.

## Consequences

- Naive BTreeSet<[u8;32]> swap would cause every live subscription to get CLOSE+REQ re-issued at deploy, and silently invalidate persisted watermarks
- Vec<[u8;32]> reintroduces order-dependence and duplicate-pubkey hazards — correctness regression in dedup/registry
- The 31ms merge cost is the smallest driver; if proposal 2's memoization works, this cost largely disappears without touching the type

## Open Tail

- If interning is pursued, filter_json_for (subs/wire.rs:215) re-parses hex Pubkey into rust-nostr PublicKey on every emit — typed keys would need to thread through that path

## Evidence

- transcript lines 879-891
