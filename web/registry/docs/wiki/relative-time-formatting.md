---
title: Relative Time Formatting
slug: relative-time-formatting
summary: The `created_at` field on the projection carries raw Unix seconds and must be formatted as relative time (e.g., '4d ago') at the presentation layer, not in the
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

# Relative Time Formatting

## Relative Time Formatting

The `created_at` field on the projection carries raw Unix seconds and must be formatted as relative time (e.g., '4d ago') at the presentation layer, not in the projection itself. A shared `NostrRelativeTime` helper (mirroring the `format_ago_secs` buckets from the TUI) formats `createdAt` at the three hydration sites: `ContentComponentPages.swift:382`, `ContentComponentPages.kt:304`, and `EmbedComponentPages.kt:398`. [^6a951-29]

## See Also

