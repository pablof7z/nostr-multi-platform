---
type: episode-card
date: 2026-05-26
session: 54fc9b94-b995-46c6-8372-59c4abe0f95a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/54fc9b94-b995-46c6-8372-59c4abe0f95a.jsonl
salience: architecture
status: superseded
subjects:
  - flatbuffers-version-pins
  - ffi-abi-drift
  - wire-format-fixture
supersedes: []
related_claims: []
source_lines:
  - 90-152
  - 157-198
  - 199-216
  - 569-575
  - 596-605
captured_at: 2026-06-18T05:51:50Z
---

# Episode: FlatBuffers transport contract invariants: version pins, ABI guard, wire fixture

## Prior State

No CI enforcement of FlatBuffers runtime version consistency across platforms or FFI callback ABI signatures; risk of divergent flatc versions producing incompatible runtime guards (e.g., FLATBUFFERS_25_2_10 mismatches)

## Trigger

Review feedback identified risk of cross-platform version drift and silent ABI regression on the hot UpdateCallback path

## Decision

Enforce intentionally asymmetric FlatBuffers runtime version pins via CI (Rust+Swift 25.12.19, Web/TypeScript 25.9.23, Android/Kotlin 25.2.10); add explicit UpdateCallback ABI drift check across all FFI headers (NmpCore.h × 2, NmpGallery.h) that fails if typedef or function signature changes; add v1 wire format golden fixture (update_frame_snapshot_v1.fb.hex); declare nmp-core as owning the canonical FlatBuffers schema in crate-boundaries.md

## Consequences

- Regenerating bindings with a wrong flatc version fails CI before mismatch reaches platform builds
- UpdateCallback ABI signature changes across any FFI header fail CI
- Wire format drift detected by fixture test
- Crate-boundaries.md updated to reflect nmp-core ownership of canonical FlatBuffers schema and Rust encode/decode helpers
- Schema file documents the asymmetric pin rationale inline

## Open Tail

*(none)*

## Evidence

- transcript lines 90-152
- transcript lines 157-198
- transcript lines 199-216
- transcript lines 569-575
- transcript lines 596-605

