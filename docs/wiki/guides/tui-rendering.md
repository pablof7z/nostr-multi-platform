---
title: TUI Rendering
slug: tui-rendering
topic: tui
summary: The TUI must use ratatui (or equivalent) with animations for power-user polish
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-25
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:4f37753c-0654-4478-9c19-e799f1b10d39
  - session:93c599f0-3aea-440a-9c42-1de6cd8771fe
  - session:e4d33847-af62-4a40-a7f2-1a77b96605a3
---

# TUI Rendering

## Framework & Animations

The TUI must use ratatui (or equivalent) with animations for power-user polish. A --basic mode must be shipped from day one, stripping animations for SSH and low-color terminals, modeled on bottom (btm). <!-- [^4f377-25] -->

## Name & Avatar Rendering

The TUI must render names inlined in the timeline and support iTerm2-capable avatar display with inline images shown in the console. Inline image previews are opt-in per message rather than rendered by default, to mitigate bandwidth and cache concerns. <!-- [^4f377-26] -->

## Image Protocol Fallback

Image rendering must follow the fallback chain: Kitty → iTerm2 → Sixel → Unicode halfblocks, using ratatui-image's Picker for runtime protocol detection. The Picker must be initialized with Picker::from_query_stdio() wrapped in an IsTerminal guard to avoid deadlocks in non-TTY CI environments. iTerm2 inline image rendering requires StatefulProtocol/StatefulImage rather than stateless Protocol/Image to prevent silent blank squares when the render rect doesn't match the pre-encoded rect.

<!-- citations: [^4f377-27] [^93c59-18] -->
## Sub-Cell Characters

Braille characters (U+2800–U+28FF) must be used for sub-cell resolution in sparklines, graphs, and engagement metrics; Block Elements for solid bars. <!-- [^4f377-28] -->

## Toast Notifications

Toast notifications appear above the status bar, stack up to 3 visible, auto-dismiss after 5 seconds, and never steal focus or touch the status bar field. <!-- [^93c59-19] -->

## Content Display

The post list view truncates content at render time based on the terminal width. The post detail view word-wraps the full content for display. <!-- [^e4d33-4] -->
