---
title: Build Infrastructure
slug: build-infrastructure
topic: ci-gates
summary: Disk space has been a recurring constraint (ENOSPC stalled agents three times)
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:418d555f-8e77-4e56-8166-93d1fef9cfce
---

# Build Infrastructure

## Disk Space & Build Constraints

Disk space has been a recurring constraint (ENOSPC stalled agents three times). Keystone teams must run sequentially (K1 then K2 then K3) to cap simultaneous build pressure and avoid disk exhaustion, and K2 implementers must serialize cargo builds.

The broken `AI architecture signoff` GitHub Action and its orphaned driver script must be deleted from the repository. <!-- [^418d5-9] -->

<!-- citations: [^2e544-49] [^2e544-445] -->
