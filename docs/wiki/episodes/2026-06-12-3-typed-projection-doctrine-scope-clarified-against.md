---
type: episode-card
date: 2026-06-12
session: 954c56b2-d292-4021-8b55-977d3fd8df4d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/954c56b2-d292-4021-8b55-977d3fd8df4d.jsonl
salience: architecture
status: active
subjects:
  - typed-projections
  - ffi-surface-design
  - protocol-vs-app-data
supersedes: []
related_claims: []
source_lines:
  - 1196-1205
captured_at: 2026-06-12T06:08:15Z
---

# Episode: Typed-projection doctrine scope clarified against 'don't type-system-ify open-ended data' warning

## Prior State

Potential concern that coracle-rust's repeated warning — 'don't type-system-ify open-ended data,' reject TagStandard enum, use trait not enum for kinds — might contradict NMP's doctrine of typed FlatBuffers projections and gated v1 schemas

## Trigger

Comparative analysis reveals the two projects sit at opposite poles by design: coracle-rust deliberately untypes protocol data (Tag as Vec<String>, kind as trait), while NMP deliberately types app-facing view models. The warning applies to protocol-level data where meaning is kind-dependent and open-ended; NMP's typed projections are app-facing, closed, and ours to define

## Decision

No doctrine change. The 'don't type-system-ify' warning only bites NMP where we expose protocol-shaped data through FFI (filters, raw tags). Typed projections remain the correct approach for app-facing surfaces

## Consequences

- Validates existing NMP typed-projection doctrine as non-contradictory with coracle-rust's philosophy
- Narrows the 'don't type-system-ify' warning to FFI filter/tag surfaces specifically
- Confirms that the empty coracle-net/coracle-storage stubs validate NMP's choice to solve those layers first

## Open Tail

*(none)*

## Evidence

- transcript lines 1196-1205

