---
title: Chirp FlatBuffers Codegen and Wire Schema
slug: chirp-flatbuffers-codegen
topic: app-codegen
summary: Each Chirp app runs `flatc` locally and checks in the generated FlatBuffers wire-type decoders rather than consuming a prebuilt bindings package from NMP
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# Chirp FlatBuffers Codegen and Wire Schema

## Local FlatBuffers Codegen

Each Chirp app runs `flatc` locally and checks in the generated FlatBuffers wire-type decoders rather than consuming a prebuilt bindings package from NMP. NMP does not ship a prebuilt bindings package for iOS, Android, or web.

Android's FlatBuffers regen script (`apps/android/scripts/regen-flatbuffers.sh`) hard-requires the exact `flatc` version (25.2.10) matching the checked-in Kotlin runtime. It downloads a matching release binary and does not rely on brew, because a mismatched `flatc` silently emits code calling a `Constants` method that doesn't exist in the pinned runtime.

<!-- citations: [^dcc80-56206] -->
## Wire Schema Evolution

The FlatBuffers OP-feed schema relocated from namespace `nmp.nip01` (schema_id `nmp.nip01.opfeed`, file_identifier `NOFS`, card type `TimelineEventCard`, schema version 1) to namespace `nmp.note_feed` (schema_id `nmp.note_feed.opfeed`, file_identifier `NNFS`, card type `NoteFeedItem`, schema version 2).

The card type `NoteFeedItem` dropped `relation_counts` and added `hosted_group` relative to the old `TimelineEventCard`.

Chirp's home feed card no longer carries `relation_counts`, `author_display_name`, `author_picture_url`, or `content_preview` on the wire. Display data is now component-owned via `LocalNostrProfileHost`.

<!-- citations: [^dcc80-be2f0] [^dcc80-dcbde] -->
