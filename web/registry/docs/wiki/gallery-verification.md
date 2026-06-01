---
title: Gallery Verification
slug: gallery-verification
summary: Nothing is called DONE until the 64-cell matrix is walked on running apps with every box checked — no compile-green shortcuts, no image hand-waving.
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

# Gallery Verification

## Verification Standard

Nothing is called DONE until the 64-cell matrix is walked on running apps with every box checked — no compile-green shortcuts, no image hand-waving. [^6a951-16]


## Verification PDF

The verification PDF at `docs/testing/nmp-gallery-verification-report.pdf` shows every component's real screenshot beside its pass criterion with honest ✓/⚠/✗ annotations; the all-green result is 48 pass / 0 warn / 0 fail across 16 components × 3 platforms. The verification PDF generator always shows the real screenshot with per-cell annotations rather than replacing failing cells with a placeholder, so the viewer can see what actually renders. [^6a951-17]

## Matrix Documentation

The matrix doc retracts the false 'Android kind:1 fetch gap' theory and documents the real presentation-layer defects (typed-article renderer gap, raw epoch, NIP-05 `_@`). [^6a951-18]

## Screenshot Checklist

A Sonnet audit agent produced `docs/testing/nmpui-website-screenshot-checklist.md` with a full inventory of every registry entry, its screenshot reference, file-existence status (OK/DANGLING/EMPTY), per-entry verification criteria, and gap list. [^6a951-19]
## See Also

