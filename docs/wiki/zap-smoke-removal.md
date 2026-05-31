---
title: Zap Smoke Artifact Removal
slug: zap-smoke-removal
summary: The zap-smoke artifact is deleted from the workspace.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-29
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:f8eb6e59-19f0-4591-a9b4-47453c051d45
  - session:f5503f3a-d44c-4626-b8de-0492ad1f2a6c
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# Zap Smoke Artifact Removal

## Removal

The zap-smoke artifact is deleted from the workspace. The release manifest (`release/nmp-release.toml`) must not reference the deleted package `zap-smoke`. The iOS-zap collision (formerly duplicate V-68) is renumbered to V-106 in the backlog.

<!-- citations: [^f8eb6-5] [^f5503-7] [^42908-26] -->
## See Also

