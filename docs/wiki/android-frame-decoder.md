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
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
---

# Android Frame Decoder

## Version Pinning

Android consumers must skip v0.3.0 and pin v0.4.0 directly due to a completely dark Android frame decoder in v0.3.0. Podcast-player's Android has no Kotlin-side UpdateFrame/payload decoder; frames are decoded in Rust and sent as JSON via mpsc, so no Tier-3 Kotlin rebuild was needed for the v0.4.0 migration. Android's decodeSucceeds = isNotEmpty() check is an acceptable D3-4 realization because the iOS per-key real-decoder preflight does not reject non-empty corrupt payloads either (FlatBuffers getRoot is unchecked), so both platforms commit non-empty corrupt bytes and fail-closed on re-decode via try/catch + identifier check.

<!-- citations: [^da6b1-62] [^da6b1-77] [^78c8e-449] -->
