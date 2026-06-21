---
title: Swift Snake-Case Conversion
slug: swift-snake-case-conversion
topic: ffi-runtime
summary: The Swift snake-case conversion preserves leading and trailing underscores while removing underscores between words, matching the Rust snapshot key conversion s
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-06-18
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:37e351ee-aa2b-43eb-9793-482de338f883
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# Swift Snake-Case Conversion

## Swift Snake-Case Conversion

The Swift snake-case conversion preserves leading and trailing underscores while removing underscores between words, matching the Rust snapshot key conversion so that future private-looking fields cannot alias public names. The nip29 ß→SS Unicode expansion in iOS initials (prefix(2).uppercased()) is left as-is since the original Rust comment explicitly accepted that behavior.

<!-- citations: [^37e35-5] [^11850-110] -->
