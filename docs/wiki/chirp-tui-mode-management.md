---
title: Chirp TUI Mode Management
slug: chirp-tui-mode-management
summary: close_palette() only resets mode to Normal when still in Palette mode, preserving InputBar mode set by the Zap action.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-29
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:95156e27-58fe-4e26-9530-1778033c4559
  - session:16ca6097-5734-4d49-8b9a-0a20d42a324b
---

# Chirp TUI Mode Management

## Mode Transitions

close_palette() only resets mode to Normal when still in Palette mode, preserving InputBar mode set by the Zap action. [^95156-1]


The TUI welcome screen path must render the modal form overlay so that modals (like account creation) are visible when triggered from the welcome screen. [^16ca6-5]
## See Also

