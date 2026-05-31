---
title: Marmot 3-Client Interop Workflow — MLS Round-Trip Verification
slug: marmot-3-client-interop-workflow
summary: The Marmot 3-client interop workflow must prove MLS messages round-trip and decrypt across iOS, Android, and TUI; `chirp-repl` serves as the scriptable headless
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-21
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
---

# Marmot 3-Client Interop Workflow — MLS Round-Trip Verification

## 3-Client Interop Workflow

The Marmot 3-client interop workflow must prove MLS messages round-trip and decrypt across iOS, Android, and TUI; `chirp-repl` serves as the scriptable headless TUI leg since chirp-tui cannot be subprocess-driven without a TTY. [^4edd4-223]


The Marmot 3-client interop workflow must prove MLS messages round-trip and decrypt across iOS, Android, and TUI; `chirp-repl` serves as the scriptable headless TUI leg since chirp-tui cannot be subprocess-driven without a TTY. Marmot group chat auto-scrolls to the latest message on first load via onAppear. [^19e07-13]
## See Also

