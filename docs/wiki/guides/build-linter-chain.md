---
title: Build-Linter Chain
slug: build-linter-chain
topic: developer-workflow
summary: Build commands and file writes are chained in a single shell invocation to prevent the linter from reverting changes between the edit and compilation steps
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-06-18
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:161ad3af-aeba-42f7-98ab-a71d2fda69a7
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:019edc59-7035-7ba3-95cc-789d362adff2
  - session:019edc92-b628-7ce1-be8a-c3d1013f2969
---

# Build-Linter Chain

## Build and Lint Integration

Build commands and file writes are chained in a single shell invocation to prevent the linter from reverting changes between the edit and compilation steps. Always-on local gates include the doctrine lint smoke test and a workspace compile-only build when public symbols, module paths, Cargo.toml dep paths, or workspace members change. Golden wire fixtures and shell compile must be regenerated for every projection-shape change; fast checks (flatc-drift, file-size) can pass while golden fixtures and shell compile still fail.

<!-- citations: [^161ad-1] [^11850-26] [^019ed-92] [^11850-50] [^019ed-138] -->
