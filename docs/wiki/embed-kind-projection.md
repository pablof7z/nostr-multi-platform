---
title: EmbedKindProjection — Rust-Owned Kind Dispatch Policy
slug: embed-kind-projection
summary: Rust owns the EmbedKindProjection enum, which determines which variant of projection data maps to a given event kind
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-28
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:1572547f-2b2d-49fb-a383-e95ca25d0bc3
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
---

# EmbedKindProjection — Rust-Owned Kind Dispatch Policy

## EmbedKindProjection

Rust owns truth; native components render snapshots, not policy. Rust owns the EmbedKindProjection enum, which determines which variant of projection data maps to a given event kind. The Unknown variant provides extensibility so that adding handlers for new kinds requires no Rust changes; native handlers check the kind and pull data from tags and content_tree fields. [^15725-10]

<!-- citations: [^15725-10] [^54ae9-6] -->
## See Also

