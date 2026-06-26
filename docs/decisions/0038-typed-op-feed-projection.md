# ADR-0038 — Typed OP-Feed projection

- **Status:** Accepted / implemented
- **Date:** 2026-06-01
- **Relates to:** ADR-0032, ADR-0033, ADR-0035, ADR-0037

## Context

`nmp.feed.home` is the highest-volume projection in the Chirp-style app surface.
After the OP-centric feed migration, the projection shape is a root-indexed feed:
thread-root cards plus reply attribution and feed-window metadata.

That shape needs its own typed descriptor because it is not the same schema as
the older modular timeline payload. A schema id names the root table identity;
schema versions evolve one identity.

## Decision

The OP-centric home feed is emitted under projection key `nmp.feed.home` with a
typed FlatBuffers sidecar owned by `nmp-nip01`:

- `schema_id = "nmp.nip01.opfeed"`
- `file_identifier = "NOFS"`
- `schema_version = 1`
- root table: `OpFeedSnapshot`

`OpFeedSnapshot` carries root cards, reply attribution, and the feed-window
sub-buffer. It reuses existing typed tables where ownership already exists:

- `TimelineEventCard` from `nmp-nip01`;
- content-tree sub-buffers from `nmp-content`;
- feed-window bytes from `nmp-feed`.

`nmp-nip01` owns the encoder/decoder and descriptor constants. The composition
root registers the typed projection by snapshotting the OP-feed engine and
encoding the result.

## Data Contract

The payload mirrors Rust-owned raw feed data. It does not pre-format display
strings, localize counts, collapse attribution into prose, or move host
presentation policy into Rust. Attribution count is the attribution vector length;
no separate display count is encoded.

## Host Behavior

For `nmp.feed.home`, a host validates the `NOFS` descriptor and decodes
`OpFeedSnapshot`. Descriptor mismatch or decode failure means the projection is
absent for that tick.

## Consequences

- The home feed stays on the typed sidecar path from ADR-0037.
- The feed-window and content-tree ownership boundaries stay intact.
- `nmp-core` remains unaware of OP-feed nouns.
- Future feed-shape changes bump `schema_version` when they preserve schema
  identity, or use a new `schema_id` when they introduce a different root shape.
