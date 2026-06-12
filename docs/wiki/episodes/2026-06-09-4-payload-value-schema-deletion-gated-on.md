---
type: episode-card
date: 2026-06-09
session: 63af4b96-d3d3-45c3-ab96-9f899beafa1b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/63af4b96-d3d3-45c3-ab96-9f899beafa1b.jsonl
salience: architecture
status: active
subjects:
  - typed-projections
  - payload-value-deletion
  - chirp-tui
  - nmp-gallery
  - chirp-desktop
supersedes: []
related_claims: []
source_lines:
  - 6979-7060
captured_at: 2026-06-11T23:10:26Z
---

# Episode: payload:Value schema deletion gated on six remaining consumers

## Prior State

The literal final step of the typed-projections migration was deleting payload:Value from the FlatBuffers schema — the field that all consumers currently decode

## Trigger

Discovery that six other in-repo consumers still decode the generic payload: chirp-tui (~25 refs), nmp-gallery iOS (~13) + tui (~2), chirp-desktop (~7), nmp-app-fixture (~1), plus external Podcastr/tenex-off

## Decision

Do NOT delete payload:Value now; file issue #1065 tracking the multi-app migration required before schema deletion is safe. Each secondary consumer must go typed-only first, then a CHANGELOG wire-break flag marks the deletion.

## Consequences

- payload:Value remains in the schema — the kernel still emits dual representations
- Chirp is the only typed-only consumer; all other apps still read JSON
- Deletion is correctly scoped as a separate multi-app program, not a single PR
- decode_snapshot_payload remains the shared helper for secondary Rust consumers

## Open Tail

- Issue #1065 tracks the gated plan: migrate chirp-tui → chirp-desktop → nmp-gallery → fixture apps → external consumers → then delete payload:Value + producer JSON path + decode_snapshot_payload

## Evidence

- transcript lines 6979-7060

