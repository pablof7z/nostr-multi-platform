---
title: DM Cold-Start E2E Verification & Bunker Receive
slug: dm-cold-start-e2e-verification
summary: DM cold-start receive on a fresh install is unproven end-to-end
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-03
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:f1b740a8-d601-4b63-8633-072c83a6de22
---

# DM Cold-Start E2E Verification & Bunker Receive

## E2E Verification Gap

DM cold-start receive on a fresh install is unproven end-to-end. The existing E2E test bypasses nmp_app_start and writes keys directly, and bunker receive is broken because inbox.rs requires raw Keys. [^f1b74-35]

## See Also

