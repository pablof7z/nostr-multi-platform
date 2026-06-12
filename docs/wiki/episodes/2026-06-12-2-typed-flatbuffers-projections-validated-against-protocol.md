---
type: episode-card
date: 2026-06-12
session: 954c56b2-d292-4021-8b55-977d3fd8df4d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/954c56b2-d292-4021-8b55-977d3fd8df4d.jsonl
salience: architecture
status: active
subjects:
  - flatbuffers-projections
  - nmp-ffi
  - coracle-comparison
supersedes: []
related_claims: []
source_lines:
  - 1196-1205
captured_at: 2026-06-12T06:21:33Z
---

# Episode: Typed FlatBuffers projections validated against protocol-level anti-typing doctrine

## Prior State

An unexamined question of whether coracle-rust's repeated warnings against type-system-ifying open-ended data (Tag as Vec<String> not enum, Kind as trait not enum, reject rust-nostr's 60-variant TagStandard) might challenge NMP's deliberately-typed FlatBuffers projection approach.

## Trigger

Comparative analysis of coracle-rust revealed that its anti-typing stance targets protocol-level data where meaning is kind-dependent and open-ended — a different layer than NMP's app-facing view models, which are closed and ours to define.

## Decision

No conflict exists between the two positions. Coracle-rust's warning applies only where NMP exposes protocol-shaped data through FFI (filter JSON, raw tags). NMP's typed projections are app-facing view models and are architecturally sound.

## Consequences

- FlatBuffers projection doctrine (F-05) is reaffirmed, not weakened
- The one surface where the warning does apply — raw filter/tag data crossing FFI — is exactly where the M2 hazard card applies
- No need to reconsider the typed-projection codegen approach

## Open Tail

*(none)*

## Evidence

- transcript lines 1196-1205

