---
title: Android Frame Decoder
slug: android-frame-decoder
topic: mobile-build-config
summary: Android consumers must skip v0.3.0 and pin v0.4.0 directly due to a completely dark Android frame decoder in v0.3.0
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
---

# Android Frame Decoder

## Version Pinning

Android consumers must skip v0.3.0 and pin v0.4.0 directly due to a completely dark Android frame decoder in v0.3.0. Podcast-player's Android has no Kotlin-side UpdateFrame/payload decoder; frames are decoded in Rust and sent as JSON via mpsc, so no Tier-3 Kotlin rebuild was needed for the v0.4.0 migration.

<!-- citations: [^da6b1-62] [^da6b1-77] -->
