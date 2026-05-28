---
title: "Runtime Update Transport"
summary: "The actor emits state frames over a binary FlatBuffers transport; hosts apply monotonic snapshots and may use typed sidecars for hot projections."
tags: [runtime, flatbuffers, ffi]
created: 2026-05-28
updated: 2026-05-28
verified: 2026-05-28
volatility: warm
confidence: high
sources:
  - "raw/repos/2026-05-28-source-map.md"
  - "raw/repos/2026-05-28-typed-feed-transport-sources.md"
---

# Runtime Update Transport

NMP's runtime loop is actor-owned. Native code dispatches an action and receives
later update frames. There is no synchronous "return current state" path.

The durable model is TEA on one actor thread: action in, state mutation inside
Rust, update out. Hosts apply updates only after a UI-thread hop and a monotonic
revision guard.

## Update Shape

The runtime update envelope is a FlatBuffers `UpdateFrame`. Snapshot frames
carry a generic `Value` tree plus optional typed projection sidecars. Panic
frames carry crash data. The file identifier distinguishes runtime frames from
arbitrary bytes.

The generic tree remains useful for low-frequency projections. The hot path can
avoid string-keyed tree walking by attaching a typed sidecar keyed by projection
name, schema id, schema version, and FlatBuffers file identifier. For example,
the `nmp.feed.home` projection can be decoded through a schema owned outside
`nmp-core` while the core transport sees only opaque bytes.

The transport code path is `crates/nmp-core/src/update_envelope.rs` over the
schema in `crates/nmp-core/schema/nmp_update.fbs`. Its current constants are:

- update-frame file identifier: `NMPU`;
- snapshot schema version: `1`;
- frame variants: `Snapshot` and `Panic`;
- typed sidecar entry: projection key plus schema id, schema version, file
  identifier, and opaque payload bytes.

## Host Rule

For a projection key, a host prefers a typed sidecar only when it recognizes the
descriptor. Otherwise it falls back to the generic payload. During migration,
both representations may exist for the same projection key.

This rule keeps transport backward-compatible without creating an app-specific
union inside `nmp-core`.

Host adoption is not global. `chirp-tui` currently decodes typed
`nmp.feed.home` sidecars and merges the typed result back into the generic
projection slot used by its renderer. iOS has generated bindings and a typed
home-feed decoder, but the live render path should be verified from
`KernelUpdateFrameDecoder` and `KernelBridge` before claiming typed feed render
adoption. Android gallery decodes the generic tree in the inspected source set.

## Projection Ownership

The sidecar descriptor prevents schema ownership from drifting into the
transport layer. `nmp-core` owns the envelope. `nmp-feed` owns feed-window
structure (`nmp.feed.window` / `NFWM`). `nmp-nip01` owns the home timeline
payload (`nmp.nip01.timeline` / `NFTS`). `nmp-content` owns content-tree typed
subpayloads.

The boundary is the important fact: a new app projection should not add a union
member to `nmp_update.fbs`.

## What This Does Not Mean

FlatBuffers is not the product model. It is the transport encoding. It does not
move product rules into native code, and it does not permit formatted display
strings in projections that should carry raw protocol data.

## See Also

- [[rust-owned-logic-boundary|Rust-Owned Logic Boundary]] ([Rust-Owned Logic Boundary](../concepts/rust-owned-logic-boundary.md))
- [[crate-boundaries-and-module-ownership|Crate Boundaries and Module Ownership]] ([Crate Boundaries and Module Ownership](crate-boundaries-and-module-ownership.md))
- [[op-feed-and-typed-projections|OP Feed and Typed Projections]] ([OP Feed and Typed Projections](op-feed-and-typed-projections.md))

## Sources

- [NMP Source Map 2026-05-28](../../raw/repos/2026-05-28-source-map.md)
- [Typed Feed Transport Sources](../../raw/repos/2026-05-28-typed-feed-transport-sources.md)
