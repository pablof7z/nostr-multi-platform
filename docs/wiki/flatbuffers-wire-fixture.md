---
title: FlatBuffers Snapshot v1 Wire Format Fixture
slug: flatbuffers-wire-fixture
summary: The FlatBuffers snapshot v1 wire format is verified against a stable hex fixture to prevent accidental wire drift.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-29
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:37e351ee-aa2b-43eb-9793-482de338f883
  - session:e4861768-9a00-4d83-b7a3-a39d07749d1c
  - session:56db993b-6de7-49f9-82b1-a9416cef3294
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
  - session:cd331450-f93f-48d0-960e-3c73e927775e
  - session:38935d82-0cbf-4e85-98d3-a0f056fd450c
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# FlatBuffers Snapshot v1 Wire Format Fixture

## Wire Format Fixture Verification

The FlatBuffers snapshot v1 wire format is verified against a stable hex fixture to prevent accidental wire drift. The golden hex fixture (`update_frame_snapshot_v1.fb.hex`) is decoded and asserted by both a vitest case and a JVM unit test to catch the regenerated-with-wrong-flatc class of bugs the version-pin guard alone cannot see. Golden .fb.hex wire fixtures pin the ContentTreeWire and ModularTimelineSnapshot wire shapes. The populated-row Swift conformance test for Marmot FlatBuffers decoding asserts camelCase field mappings (id_hex→idHex, sender_pubkey_hex→senderPubkeyHex, created_at→createdAt) to guard against the keyNotFound regression class that previously wiped feeds.

The Rust actor emits a complete snapshot as binary FlatBuffers at a configurable frequency (default 4Hz, tunable via `emitHz`).

The typed NOFS FlatBuffers migration for the home feed (ADR-0038) is complete across all four stages (T1-T4).

SnapshotProjections includes a `ClaimedEventDto` and `claimedEvents: [String: ClaimedEventDto]?` field for typed FlatBuffers decoding. The embed architecture complies with ADR-0037 by reading typed Swift structs from SnapshotProjections instead of raw JSON.

<!-- citations: [^54ae9-5] [^cd331-6] [^37e35-3] [^e4861-2] [^56db9-3] [^38935-5] [^4edd4-8] -->
## See Also

