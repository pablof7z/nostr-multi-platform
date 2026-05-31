---
title: Chirp iOS Scroll FPS Performance Gate
slug: chirp-ios-scroll-fps-performance-gate
summary: Scroll FPS performance gate requires ≥58fps with hitch <5ms/s as measured by XCTOSSignpostMetric.scrollDecelerationMetric
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-31
updated: 2026-05-31
verified: 2026-05-31
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Chirp iOS Scroll FPS Performance Gate

## Scroll FPS Performance Gate

Scroll FPS performance gate requires ≥58fps with hitch <5ms/s as measured by XCTOSSignpostMetric.scrollDecelerationMetric. Navigation transitions must have 0 dropped frames on push/pop as measured by .navigationTransitionMetric. [^4edd4-266]

## See Also

