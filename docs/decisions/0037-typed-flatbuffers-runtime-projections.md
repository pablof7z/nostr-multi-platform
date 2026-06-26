# ADR-0037 — Typed FlatBuffers sidecars for runtime projections

- **Status:** Accepted / implemented
- **Date:** 2026-05-30
- **Relates to:** ADR-0032, ADR-0033, ADR-0044

## Context

NMP pushes snapshots through a stable FlatBuffers update envelope. High-volume
projection payloads, especially feed payloads, need a host-readable encoding that
does not make each platform walk string-keyed generic data on every snapshot
tick.

At the same time, the transport crate must not learn app or protocol nouns. A
transport-level union over every projection type would make `nmp-core` regenerate
bindings whenever an app adds a view, and would repeat the bespoke FFI coupling
that ADR-0025 rejected.

## Decision

Runtime projections that need typed wire speed are emitted as typed sidecars:

```text
SnapshotFrame
  typed_projections: [TypedProjection]

TypedProjection
  key: projection key
  payload: TypedPayload

TypedPayload
  schema_id: string
  schema_version: u32
  file_identifier: string
  payload: bytes
```

`nmp-core` treats the sidecar payload bytes as opaque. The descriptor tells a host
which app or protocol-owned decoder is allowed to read the bytes.

## Ownership

The crate that owns a projection's data shape owns its typed schema, checked-in
bindings, encoder, decoder, descriptor constants, and schema-version policy.

For the feed family:

- `nmp-feed` owns feed window/cursor/page tables.
- `nmp-content` owns content-tree tables.
- `nmp-nip01` owns short-note timeline and OP-feed tables.
- `nmp-defaults` wires the default registrations that compose those pieces for a
  concrete app build.

New typed projections do not require edits to the transport schema unless the
transport envelope itself changes.

## Data Contract

Typed projection fields carry raw protocol data, not host presentation decisions.
Pubkeys and event ids are encoded as canonical hex, timestamps as Unix seconds,
counts as raw integers, and optional protocol fields as explicit typed fields.
Formatting, localization, row layout, and image treatment remain host
presentation work.

## Registration

The typed registration seam lives on the app-host abstraction used by reusable
protocol/feed crates. A registration closure returns an optional typed projection
payload for a projection key. Snapshot emission includes the sidecar when the
projection has changed and the closure has data.

## Host Behavior

A host validates the projection key and descriptor before decoding. Unknown
schema ids, unsupported schema versions, wrong file identifiers, or invalid bytes
fail closed for that projection tick.

## Consequences

- Hot-path projections decode by field offset instead of map lookup.
- `nmp-core` remains closed to app/protocol projection types.
- Schema-version skew is handled per projection descriptor.
- The FlatBuffers runtime pin discipline applies to every schema-owning crate,
  not only to the transport envelope.
