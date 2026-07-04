---
title: Inline Video Player Component
slug: inline-video-player
topic: ui-components
summary: Inline video players in note content views use a dedicated `NostrInlineVideoPlayer` view with `@State` so the `AVPlayer` is constructed exactly once per view id
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:f308bb0b-7b74-4684-9a5b-1fce8ffcab35
---

# Inline Video Player Component

## Inline Video Player

Inline video players in note content views use a dedicated `NostrInlineVideoPlayer` view with `@State` so the `AVPlayer` is constructed exactly once per view identity, not on every SwiftUI body re-evaluation. `NostrInlineVideoPlayer.swift` is a new Swift source file following the project's one-component-per-file convention, registered via `xcodegen generate`. <!-- [^f308b-08e34] -->
