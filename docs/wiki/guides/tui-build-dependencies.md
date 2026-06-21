---
title: TUI Build Dependencies
slug: tui-build-dependencies
topic: tui
summary: The dependency stack must be based on ratatui 0.30 + crossterm 0.29
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
---

# TUI Build Dependencies

## Dependency Stack

The dependency stack must be based on ratatui 0.30 + crossterm 0.29. ratatui-image 8.1.1 is the last version compatible with ratatui 0.29; versions 9.0.0+ require ratatui 0.30. The ratatui-image Picker fallback font size should be (9, 18) instead of (8, 16) for typical iTerm2 on macOS. tui-textarea requires either a fork or pinning the tree to ratatui 0.29 due to its lagging upstream release. Standalone TUI mockup crates under a Cargo workspace must declare an empty [workspace] table in their Cargo.toml to opt out of the parent workspace.

<!-- citations: [^4f377-1] [^93c59-3] -->
