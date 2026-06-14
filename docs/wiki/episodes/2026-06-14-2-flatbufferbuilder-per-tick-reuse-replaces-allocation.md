---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: active
subjects:
  - flatbuffer-builder-reuse
  - kernel-encode-path
  - tier3-frame
supersedes:
  - 2026-06-14-2-r3-s2-kernel-owned-flatbufferbuilder-reuse
related_claims: []
source_lines:
  - 8431-8498
  - 8513-8557
  - 8559-8671
captured_at: 2026-06-14T09:02:27Z
---

# Episode: FlatBufferBuilder per-tick reuse replaces allocation (ADR-0055 R3-S2)

## Prior State

The 4Hz kernel encode path allocated a fresh FlatBufferBuilder on every tick, with no reuse across ticks.

## Trigger

ADR-0055 Rung 3 ladder step S2 — per-tick allocation kill.

## Decision

Hold one FlatBufferBuilder<'static> in the Kernel struct; reset() at start of each encode; copy finished bytes out via to_vec() before return. The aux path (encode_snapshot_frame, test-only callers) is left allocating fresh — it has no kernel to own a persistent builder and is not on the 4Hz hot path.

## Consequences

- Kernel !Send + exclusive &mut borrow guarantees no re-entrancy hazard on the shared builder
- to_vec() is the sole ownership-transfer point; reset() only runs on next-tick entry, so no use-after-reset window exists
- encode_snapshot_with_envelope signature widened to accept &mut FlatBufferBuilder<'_> as first param
- Review caught a vacuous test (earlier_frame_not_mutated_by_later_encode asserted None==None); rewritten to clone frame1 bytes before later encodes and assert byte-identity after
- Byte-identity structurally guaranteed: same field-population order, same args; reset() only affects allocation reuse, not output

## Open Tail

*(none)*

## Evidence

- transcript lines 8431-8498
- transcript lines 8513-8557
- transcript lines 8559-8671

