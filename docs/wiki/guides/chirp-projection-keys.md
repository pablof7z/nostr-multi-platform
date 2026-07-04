---
title: Chirp Projection Keys and Timeline Namespace
slug: chirp-projection-keys
topic: app-projection
summary: Chirp's `nmp.feed.home`/`.author.<pubkey>`/`.thread.<event_id>` projection keys are renamed to `chirp.timeline.home`/`chirp.timeline.author.<pubkey>`/`chirp.tim
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

# Chirp Projection Keys and Timeline Namespace

## Projection Key Namespace

Chirp's `nmp.feed.home`/`.author.<pubkey>`/`.thread.<event_id>` projection keys are renamed to `chirp.timeline.home`/`chirp.timeline.author.<pubkey>`/`chirp.timeline.thread.<event_id>`. This rename is complete and verified across all five surfaces: Rust (tui/desktop/android-FFI), iOS (arm64 build-clean), Android (gradle-green + tests), and web (hand-reviewed). The `CHIRP_TIMELINE_NAMESPACE_PREFIX` constant in `nmp_chirp_config` is the single source for the `chirp.timeline.` projection-key prefix, used by all production code instead of hand-typing the literal. `ProjectionKey` is serialized as a bare string via `#[serde(try_from = "String", into = "String")]`.

<!-- citations: [^dcc80-c9485] [^dcc80-82377] [^dcc80-ec2c9] [^dcc80-5c6ac] [^dcc80-dde2c] -->
## OP-Feed FlatBuffers Schema

The OP-feed FlatBuffers schema relocated from `nmp.nip01.opfeed`/`NOFS`/`TimelineEventCard`/v1 to `nmp.note_feed.opfeed`/`NNFS`/`NoteFeedItem`/v2. The wire-format fields `relation_counts`, `author_display_name`, `author_picture_url`, and `content_preview` are dropped from the op-feed wire (not a bug); display data is component-owned via `LocalNostrProfileHost`, and `hosted_group` is new and wired through.

<!-- citations: [^dcc80-32b85] [^dcc80-2e4c7] -->
## Mobile Decoders

Chirp's iOS and Android decoders use projection key `chirp.timeline.home`, schema id `nmp.note_feed.opfeed`, and file identifier `NNFS`. <!-- [^dcc80-6b974] -->
