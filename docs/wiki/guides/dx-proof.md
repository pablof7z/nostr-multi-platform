---
title: DX Clean-Room Proof Gate
slug: dx-proof
topic: dx-proof
summary: "Issue #2256 is the clean-break DX gate: a clean-room onboarding proof"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-06-29
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:898a41b5-68e0-4b0f-b16c-c6072454bd6a
  - session:019f0dc3-5b56-79d1-a14b-5746c93e5879
---

# DX Clean-Room Proof Gate

## DX Clean-Room Proof (#2256)

Issue #2256 is the clean-break DX gate: a clean-room onboarding proof. A developer with a fresh checkout and published docs only must build a small app in ≤2 hours, using solely the new model — typed read sessions, explicit composition, and the construction/signing/publishing split — without `register_defaults` or raw projection vocabulary. The proof must include defining one app-private kind with generated builders; it should not pass while custom app kinds still require raw hand-rolling. #2256 blocks release blocker #2121; when the proof passes, #2121 drops onboarding/DX as a release blocker, which is the migration green light.

<!-- citations: [^898a4-f4778] [^019f0-cea2e] -->
