# ADR-0038 — Typed OP-Feed projection

- **Status:** Accepted / implemented
- **Date:** 2026-06-01
- **Relates to:** ADR-0032, ADR-0033, ADR-0035, ADR-0037

## Context

Chirp-style home timelines are high-volume app-owned feed projections. After
the OP-centric feed migration, their payload shape is a root-indexed feed:
thread-root cards plus reply attribution and feed-window metadata.

That shape needs its own typed descriptor because it is not the same schema as
the older modular timeline payload. A schema id names the root table identity;
schema versions evolve one identity.

## Decision

OP-centric product feeds are emitted under caller/app-owned projection keys with
a typed FlatBuffers sidecar whose schema/codec is owned by `nmp-note-feed`:

- `schema_id = "nmp.note_feed.opfeed"`
- `file_identifier = "NNFS"`
- `schema_version = 1`
- root table: `OpFeedSnapshot`

`OpFeedSnapshot` carries concrete note-feed items, reply attribution, and the
feed-window sub-buffer. It embeds owned lower-level payloads where ownership
already exists:

- `NoteFeedItem` / repost attribution from `nmp-note-feed`;
- content-tree sub-buffers from `nmp-content`;
- feed-window bytes from `nmp-feed`.

This is a new schema identity because the owner and root row shape changed from
NIP-01 timeline cards to note-feed items. Social/action-row facts are opened
through their concept owners when a host needs them; the OP-feed projection
stays a raw feed/item surface.

`nmp-note-feed` owns the encoder/decoder and descriptor constants. The
composition root registers the typed projection by snapshotting the OP-feed
engine and encoding the result.

## Data Contract

The payload mirrors Rust-owned raw feed data. It does not pre-format display
strings, localize counts, collapse attribution into prose, or move host
presentation policy into Rust. Attribution count is the attribution vector length;
no separate display count is encoded.

## Host Behavior

For an app-owned OP-feed projection, a host routes by the projection key it
opened, validates the `NNFS` descriptor, and decodes `OpFeedSnapshot`.
Descriptor mismatch or decode failure means the projection is absent for that
tick.

## Consequences

- App home feeds stay on the typed sidecar path from ADR-0037 without reserving
  a framework-owned product key.
- The feed-window and content-tree ownership boundaries stay intact.
- `nmp-nip01` remains a lower-level note/thread fact owner.
- `nmp-core` remains unaware of OP-feed nouns.
- Future feed-shape changes bump `schema_version` when they preserve schema
  identity, or use a new `schema_id` when they introduce a different root shape.
