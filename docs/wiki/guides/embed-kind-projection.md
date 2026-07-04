---
title: "EmbedKindProjection: Typed Content-Kind Projection and Cross-Platform Dispatch"
slug: embed-kind-projection
topic: ui-components
summary: EmbedKindProjection is the per-kind typed projection struct in nmp-content that maps a raw Nostr event to a renderable embed, dispatched through a single match
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# EmbedKindProjection: Typed Content-Kind Projection and Cross-Platform Dispatch

## Architecture

EmbedKindProjection is the per-kind typed projection struct in nmp-content that maps a raw Nostr event to a renderable embed, dispatched through a single match on event.kind. Each content kind has a typed XProjection struct defined in nmp-content/src/embed_projection/variants.rs.

Every platform dispatch registry exhaustively matches the EmbedKindProjection enum across tui, swiftui, compose, desktop, and nmp-gallery preview apps.

The embed_sidecar FlatBuffers wire union carries embed projections cross-FFI, with each variant requiring a table plus encode and decode arms.

<!-- citations: [^d8bc6-e12d2] [^d8bc6-d96f8] [^d8bc6-7b4c5] -->
## Adding a New Content Kind

Adding a new content kind to nmp-content requires wiring through EmbedKindProjection, FlatBuffers embed_sidecar wire (schema + encode/decode), golden/fixture tests, all platform dispatch registries (tui, swiftui, compose, desktop, gallery previews), and then per-platform card components + registry manifests. Concretely, adding a new content kind requires a new EmbedKindProjection variant + typed projection struct, a match dispatch in resolve_embed_projection, FlatBuffers embed_sidecar wire encode/decode arms, golden/fixture test updates, and exhaustive registry updates across tui/swiftui/compose/desktop platforms plus nmp-gallery previews.

<!-- citations: [^d8bc6-0d3a3] [^d8bc6-bca0f] -->
## Issue Decomposition

Issue #2928 (content-kind-39000 NIP-29 group card) is reclassified as post-v1 Backlog because it was filed as a ~5-file registry add but is actually a ~15-20 file cross-cutting change requiring a new EmbedKindProjection variant, FlatBuffers wire union, 4 platform renderers, gallery previews, and registry manifests. It should be split into (a) a prerequisite domain/wire issue for EmbedKindProjection + FlatBuffers wiring and (b) the 4 platform card components + registry manifests once (a) lands, or accepted as one larger single-PR feature.

NMP#3016 is a framework gap: the embed-resolver returns wrong field data for articles (raw image URL as title) and quoted short notes (empty author name/content). <!-- [^dcc80-04982] -->

<!-- citations: [^d8bc6-43746] [^d8bc6-5c17b] [^d8bc6-fa7f3] -->
