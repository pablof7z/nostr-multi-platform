---
title: Android Kotlin JSON Safety & Null Handling
slug: android-kotlin-json-safety
summary: When accessing JSON elements in `GalleryModel.kt`, use `as? JsonObject` safe casts instead of the `.jsonObject` property
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-25
updated: 2026-05-25
verified: 2026-05-25
compiled-from: conversation
sources:
  - session:c8c2902c-43a6-4b1c-8215-1732dc266895
---

# Android Kotlin JSON Safety & Null Handling

## Safe JSON Element Access

When accessing JSON elements in `GalleryModel.kt`, use `as? JsonObject` safe casts instead of the `.jsonObject` property. This prevents crashes when encountering `JsonNull` values, which would otherwise throw an exception during an unsafe cast. [^c8c29-3]

## See Also

