---
title: Media Renderer Injection — Environment-Based Extensibility Seam
slug: media-renderer-injection
summary: Apps must be able to inject custom image and video loaders (e.g
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-25
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:ec51ad49-af31-4415-aab4-e9123eb63eab
  - session:e7a1d168-3c58-4438-a544-aa645850c388
---

# Media Renderer Injection — Environment-Based Extensibility Seam

## Environment-Based Media Rendering

Apps must be able to inject custom image and video loaders (e.g. Kingfisher) via an environment-based extensibility seam. NmpMediaRenderer must be injectable via .environment(\.nmpMediaRenderer, ...) at any ancestor view. The NmpMediaRenderer seam using CompositionLocal-based media extensibility should be upstreamed from the Android gallery to the registry.

<!-- citations: [^ec51a-13] [^e7a1d-3] -->
## See Also

