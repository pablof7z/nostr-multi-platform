# ADR-0030 — Binding surface split

- **Status:** Accepted
- **Date:** 2026-05-23
- **Relates to:** ADR-0009, ADR-0010, ADR-0037, ADR-0044

## Context

NMP has two different host binding problems:

1. **Write/register verbs**: lifecycle, callback installation, capability
   callbacks, identity helpers, and generic action dispatch.
2. **Read/update decoding**: the high-volume pushed update stream and typed
   projection payloads.

Those surfaces have different owners. UniFFI is a fit for object/verb bindings.
It is not the transport for high-volume update frames.

## Decision

Keep the write/register surface on the existing ABI until the UniFFI migration is
scheduled as its own binding milestone.

Generate and check in typed read decoders for the update stream through
`nmp-codegen` and schema-owned FlatBuffers generation. The read surface is typed
snapshot envelope fields plus typed projection sidecars; host decoder drift is a
codegen problem, not a UniFFI problem.

## Rules

- New write verbs need a clear reason they cannot be routed through generic
  action dispatch.
- Projection/read schema changes must regenerate host decoder glue and pass the
  checked-in diff gate.
- UniFFI migration work must not take ownership of the update stream.

## Consequences

- The write surface can migrate deliberately without blocking typed read safety.
- Host projection drift becomes a generated-code diff instead of a hand-mirrored
  decoder bug.
- Platform hosts decode update frames through schema-owned generated bindings.
