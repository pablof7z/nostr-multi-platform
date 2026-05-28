---
title: "Typed Feed Transport Sources 2026-05-28"
summary: "Source notes for UpdateFrame, typed projection sidecars, nmp.feed.home typed payloads, and host fallback behavior."
tags: [repo, flatbuffers, feed, transport]
source_type: repo-snapshot
repo: /Users/pablofernandez/Work/nostr-multi-platform
commit: 50ecae23b3587affa1ae167baa067a1e07b9a677
ingested: 2026-05-28
updated: 2026-05-28
---

# Typed Feed Transport Sources 2026-05-28

## Primary Source Files

- `crates/nmp-core/schema/nmp_update.fbs`
- `crates/nmp-core/src/update_envelope.rs`
- `docs/decisions/0037-typed-flatbuffers-runtime-projections.md`
- `crates/nmp-feed/schema/feed_home.fbs`
- `crates/nmp-feed/src/typed_wire/mod.rs`
- `crates/nmp-nip01/schema/timeline_snapshot.fbs`
- `crates/nmp-nip01/src/typed_wire.rs`
- `apps/chirp/nmp-app-chirp/src/ffi/register.rs`
- `apps/chirp/chirp-tui/src/snapshot.rs`
- `ios/Chirp/Chirp/Bridge/KernelUpdateFrameDecoder.swift`
- `ios/Chirp/Chirp/Bridge/TypedHomeFeedDecoder.swift`
- `apps/nmp-gallery/android/app/src/main/kotlin/org/nmp/gallery/bridge/NmpUpdateFrameDecoder.kt`

## UpdateFrame Envelope

The runtime envelope is a FlatBuffers `nmp.transport.UpdateFrame` with file
identifier `NMPU`. It has two variants: `Snapshot` and `Panic`.

Snapshot frames carry the generic JSON-like `Value` tree plus optional
`typed_projections`. Typed entries are keyed by projection key and described
by a `(schema_id, schema_version, file_identifier)` tuple. The payload bytes
are opaque to `nmp-core`.

## Sidecar Rationale

ADR-0037 rejects a transport-level union of app projection types. New typed
projections should land in the projection-owning crate, not in `nmp-core`.
The transport schema carries opaque typed bytes and a descriptor so hosts can
select a decoder without making the kernel schema know app or protocol nouns.

## Feed Window Schema

`nmp-feed` owns the structural feed-window schema:

- schema id: `nmp.feed.window`
- file identifier: `NFWM`
- fields: page, cursor, and window metrics

It deliberately carries no event cards. Protocol projection schemas embed this
buffer instead of duplicating cursor/page tables.

## NIP-01 Timeline Schema

`nmp-nip01` owns the typed `ModularTimelineSnapshot` schema for the
`nmp.feed.home` pilot:

- schema id: `nmp.nip01.timeline`
- file identifier: `NFTS`
- schema version: `1`

It carries blocks, cards, author display facts, relation counts, content
render data, repost attribution, typed content-tree bytes, and embedded
`nmp-feed` window bytes.

## Chirp Emitter

`nmp_app_chirp_register` registers a typed snapshot producer for
`"nmp.feed.home"`. It reads the same `ModularTimelineProjection` current
window used by the generic feed controller and encodes it through
`nmp_nip01::typed_wire::encode_modular_timeline_snapshot`.

## Host Decoders

`chirp-tui` decodes `UpdateFrame` with `decode_snapshot_with_typed`, then
prefers the typed `"nmp.feed.home"` sidecar when its schema id matches
`nmp.nip01.timeline`. It converts the typed snapshot back into the generic
serde shape so the existing renderer can stay unchanged during migration.

iOS has generated update bindings, a `TypedProjectionEnvelope` type, and a
`TypedHomeFeedDecoder` for `NFTS` payloads. Before claiming iOS render
adoption, inspect the current call path: `KernelUpdateFrameDecoder.decode`
returns the generic decoded update, and the private typed-projection extractor
is the local seam for lifting sidecars.

Android gallery decodes the generic `UpdateFrame` tree. This source set did
not show an Android typed home-feed decoder.

## Authority Notes

The host preference contract is in ADR-0037. Actual host adoption is determined
by the platform bridge files listed above, not by the ADR rollout order.
