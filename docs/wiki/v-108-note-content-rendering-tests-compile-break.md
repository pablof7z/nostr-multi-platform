---
title: V-108 — NoteContentRenderingTests.swift Compile Break (Not CI-Gated)
slug: v-108-note-content-rendering-tests-compile-break
summary: "Backlog item V-108: NoteContentRenderingTests.swift has a compile break since commit 98dcd313; ChirpTests is not CI-gated, so the break was invisible."
tags:
  - ios
  - backlog
  - v-108
  - tests
  - ci-gap
volatility: cold
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# V-108 — NoteContentRenderingTests.swift Compile Break (Not CI-Gated)

> Backlog item V-108: NoteContentRenderingTests.swift has a compile break since commit 98dcd313; ChirpTests is not CI-gated, so the break was invisible.

## Overview

V-108 is a backlog item tracking a compile break in NoteContentRenderingTests.swift that has existed since commit 98dcd313. The test file does not compile, but because ChirpTests is not CI-gated, the break was invisible to all automated checks. The test target has likely been broken since the commit without anyone noticing. [^4edd4-92]

## Impact

ChirpTests is not part of the CI pipeline, so this break does not block PRs or releases. However, it means the ChirpTests target cannot be run at all — any test added to it will also not compile. This should be fixed before adding new tests that depend on the ChirpTests target compiling. [^4edd4-93]

## Discovery Context

V-108 was discovered during the Swift instrumentation PR (#824) work. While adding new unit tests for the profile display name fallback chain, the compile break in NoteContentRenderingTests.swift surfaced as an obstacle. The break was filed in BACKLOG.md for future resolution. [^4edd4-94]

## See Also

