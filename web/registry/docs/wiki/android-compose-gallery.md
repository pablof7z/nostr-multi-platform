---
title: Android (Compose) Gallery
slug: android-compose-gallery
summary: The Android typed-article Compose component (`NostrArticleCard`) is published to the registry as `compose/content-kind-30023` with canonical source, regenerated
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-31
updated: 2026-05-31
verified: 2026-05-31
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# Android (Compose) Gallery

## Component Registry

The Android typed-article Compose component (`NostrArticleCard`) is published to the registry as `compose/content-kind-30023` with canonical source, regenerated `registry.json`, vendored copy, and a website entry. [^6a951-1]


The Android `NostrContentView` dispatches kind:30023 inline EventRefs to a typed `NostrArticleCard` Compose component (hero image 16:9 + title + summary + byline) via an `articleCardProvider` closure, rather than routing all inline EventRefs through a generic `NostrQuoteCard`. [^6a951-2]
## See Also

