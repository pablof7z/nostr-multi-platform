---
type: episode-card
date: 2026-06-29
session: 3c942260-311d-4e00-8bcc-204045ea87b3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/3c942260-311d-4e00-8bcc-204045ea87b3.jsonl
salience: reversal
status: active
subjects:
  - uniffi-adoption
  - byte-lane-exception
  - dispatch-ownership
  - architecture-unified
supersedes: []
related_claims: []
source_lines:
  - 2121-2152
  - 2317-2326
  - 2393-2410
  - 2545-2570
captured_at: 2026-06-29T09:52:11Z
---

# Episode: Collapse UniFFI performance assumption; unify to single-surface architecture

## Prior State

Two-surface architecture: internal C byte-lane exception (assumed necessary for hot update-sink push path); dispatch logic in nmp-ffi; multiple symbols marked 'internal exception' behind UniFFI facade.

## Trigger

#2388 benchmark measured UniFFI vs C-lane byte transport; result: surcharged weighted-p99 delta 1,323 ns (0.013% of 16.67ms 60fps budget), 390× below pre-registered 833 µs COLLAPSE threshold.

## Decision

Adopt pure UniFFI architecture with zero internal exceptions. Move dispatch-core logic from nmp-ffi to nmp-native-runtime, establishing FFI-agnostic binding strategy. Establish nmp-uniffi as canonical template for wholesale 56-symbol migration (C1–C8).

## Consequences

- All 4 byte-lane 'exception' candidates migrate cleanly through UniFFI (no hidden paths).
- Eliminates 3 symbols via UniFFI object/Vec<u8> lifetime management.
- Dispatch logic unified in nmp-native-runtime; C-ABI and UniFFI consume same implementation.
- Architecture doctrine simplified from two-surface to single-surface (legacy decision 0030 updated).
- Enables systematic wholesale adoption via 8 disjoint slices (56 symbols); pattern-copying mechanically replicable.
- Removes architectural complexity of parallel code paths.

## Open Tail

- Wholesale adoption incomplete; C1–C8 migration execution (50+ symbols) and M14-D deletion of old C-ABI + ratchet pending.

## Evidence

- transcript lines 2121-2152
- transcript lines 2317-2326
- transcript lines 2393-2410
- transcript lines 2545-2570

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-29-1-collapse-uniffi-performance-assumption-unify-to.json`](transcripts/2026-06-29-1-collapse-uniffi-performance-assumption-unify-to.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-29-1-collapse-uniffi-performance-assumption-unify-to.json`](transcripts/raw/2026-06-29-1-collapse-uniffi-performance-assumption-unify-to.json)
