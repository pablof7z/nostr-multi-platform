---
title: CI Disk Space and Infrastructure
slug: ci-disk-space
topic: mobile-ci
summary: The CI runner disk-exhaustion bug (cargo test failing with "No space left on device") was fixed by PR #1030 adding a free-disk-space step to test.yml that remov
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-09
updated: 2026-06-09
verified: 2026-06-09
compiled-from: conversation
sources:
  - session:63af4b96-d3d3-45c3-ab96-9f899beafa1b
---

# CI Disk Space and Infrastructure

## CI Disk Space

The CI runner disk-exhaustion bug (cargo test failing with "No space left on device") was fixed by PR #1030 adding a free-disk-space step to test.yml that removes ~25GB of preinstalled SDKs. <!-- [^63af4-1] -->
