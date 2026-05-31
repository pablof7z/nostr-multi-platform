---
title: Chirp TUI CI Testing & Demo Recording
slug: chirp-tui-ci-testing
summary: VHS is used only for non-image E2E test flows; QuickTime + iTerm2 is used for image-heavy demos since VHS (headless ttyd) cannot render iTerm2/Kitty protocol im
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-21
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:4f37753c-0654-4478-9c19-e799f1b10d39
---

# Chirp TUI CI Testing & Demo Recording

## E2E Test Tooling Split

VHS is used only for non-image E2E test flows; QuickTime + iTerm2 is used for image-heavy demos since VHS (headless ttyd) cannot render iTerm2/Kitty protocol images. [^4f377-12]


The CI testing stack uses TestBackend + insta snapshots + expectrl for PTY-driven E2E tests. [^4f377-13]
## See Also

