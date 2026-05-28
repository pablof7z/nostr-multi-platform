---
title: "OP Feed and Typed Projections"
summary: "How the reusable feed engine, NIP-01 timeline schema, typed sidecar transport, and host decoders relate around nmp.feed.home."
tags: [feed, op-feed, flatbuffers, chirp]
created: 2026-05-28
updated: 2026-05-28
verified: 2026-05-28
volatility: warm
confidence: high
sources:
  - "raw/repos/2026-05-28-typed-feed-transport-sources.md"
  - "raw/repos/2026-05-28-app-composition-and-chirp-sources.md"
---

# OP Feed and Typed Projections

`nmp.feed.home` is both a product surface and a stress test for NMP's ownership
rules. It is high-volume, it crosses FFI, and it mixes generic feed mechanics
with NIP-01/NIP-10 timeline semantics. The current design keeps those concerns
separate.

## Ownership Split

`nmp-feed` owns reusable viewport mechanics: cursors, pages, window state,
transitive card inclusion, and the feed-controller registry. It knows nothing
about NIP-10 replies, reposts, author profiles, or Chirp ranking.

`nmp-nip01` owns the NIP-01/NIP-10 timeline projection shape. Its typed
`ModularTimelineSnapshot` carries timeline blocks, event cards, relation
counts, author display facts, content render data, repost attribution, and an
embedded `nmp-feed` window buffer.

`apps/chirp/nmp-app-chirp` composes the projection into the running app and
registers the feed key.

## Typed Sidecar Chain

The transport frame remains one `NMPU` update frame. For the home feed, the
typed sidecar chain is:

1. `SnapshotFrame.typed_projections[]` contains key `"nmp.feed.home"`.
2. Its descriptor says schema id `nmp.nip01.timeline`, version `1`, file id
   `NFTS`.
3. The `NFTS` payload is decoded by `nmp-nip01` bindings.
4. Inside that payload, feed page/cursor/window data is embedded as the
   `nmp.feed.window` / `NFWM` buffer owned by `nmp-feed`.
5. Content trees are embedded as the typed `nmp-content` buffer, not as
   display-formatted host strings.

The generic `Value` projection remains during compatibility. A host that
recognizes the descriptor should prefer typed data; a host that does not should
fall back to the generic tree.

## Host Adoption Is Per Host

`chirp-tui` already exercises the intended migration pattern: decode the typed
home-feed sidecar when present, convert it back into the existing serde shape,
and let the old renderer continue from the same projection slot.

iOS has generated bindings and a `TypedHomeFeedDecoder`, but current adoption
should be verified from the live call path rather than inferred from the file's
existence. The decoder seam is present; the generic `KernelUpdateFrameDecoder`
still returns a decoded generic update.

Android gallery decodes the generic update tree in the inspected source set.

## Why This Matters

The typed sidecar is an encoding optimization, not a product-model change.
It must preserve the same raw-data contract as the generic projection:
pubkeys and ids are raw protocol strings, counts are numbers, and profile
metadata is absent until the corresponding event has been observed.

If a typed projection starts carrying preformatted display decisions, the bug
is not FlatBuffers. The bug is that projection ownership moved from Rust
product logic into a host-facing transport shape.

## See Also

- [[runtime-update-transport|Runtime Update Transport]] ([Runtime Update Transport](runtime-update-transport.md))
- [[app-composition-and-chirp-wiring|App Composition and Chirp Wiring]] ([App Composition and Chirp Wiring](app-composition-and-chirp-wiring.md))
- [[rust-owned-logic-boundary|Rust-Owned Logic Boundary]] ([Rust-Owned Logic Boundary](../concepts/rust-owned-logic-boundary.md))

## Sources

- [Typed Feed Transport Sources](../../raw/repos/2026-05-28-typed-feed-transport-sources.md)
- [App Composition and Chirp Wiring Sources](../../raw/repos/2026-05-28-app-composition-and-chirp-sources.md)
