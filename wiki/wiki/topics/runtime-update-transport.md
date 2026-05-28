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

## Host Rule

For a projection key, a host prefers a typed sidecar only when it recognizes the
descriptor. Otherwise it falls back to the generic payload. During migration,
both representations may exist for the same projection key.

This rule keeps transport backward-compatible without creating an app-specific
union inside `nmp-core`.

## What This Does Not Mean

FlatBuffers is not the product model. It is the transport encoding. It does not
move product rules into native code, and it does not permit formatted display
strings in projections that should carry raw protocol data.

## See Also

- [[rust-owned-logic-boundary|Rust-Owned Logic Boundary]] ([Rust-Owned Logic Boundary](../concepts/rust-owned-logic-boundary.md))
- [[crate-boundaries-and-module-ownership|Crate Boundaries and Module Ownership]] ([Crate Boundaries and Module Ownership](crate-boundaries-and-module-ownership.md))

## Sources

- [NMP Source Map 2026-05-28](../../raw/repos/2026-05-28-source-map.md)
