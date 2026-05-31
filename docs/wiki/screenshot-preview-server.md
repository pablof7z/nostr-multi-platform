---
title: Screenshot Preview Server
slug: screenshot-preview-server
summary: The Vite preview server must serve from the rebuilt dist directory that includes screenshot PNG files from public/screenshots/.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-25
updated: 2026-05-25
verified: 2026-05-25
compiled-from: conversation
sources:
  - session:1231660f-79c1-4b38-9651-9111cc20afb0
  - session:53838558-81bd-433d-a46d-d117ecebb361
---

# Screenshot Preview Server

## Preview Server Configuration

The Vite preview server must serve from the rebuilt dist directory that includes screenshot PNG files from public/screenshots/. All registry screenshots must be full-screen iPhone simulator captures (e.g. 1206×2622), not cropped component previews. Screenshot files are named using the pattern {slug}-{platform}-preview.png (e.g. user-avatar-swift-preview.png).

<!-- citations: [^12316-5] [^53838-15] -->
## See Also

