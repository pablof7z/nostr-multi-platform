---
title: Proof-of-Work Batching API
slug: pow-batching
topic: kernel-boundary
summary: Proof-of-work mining must use a batch-based API (start, count) rather than a blocking loop, so the caller controls parallelization and the library stays platfor
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-12
updated: 2026-06-12
verified: 2026-06-12
compiled-from: conversation
sources:
  - session:954c56b2-d292-4021-8b55-977d3fd8df4d
---

# Proof-of-Work Batching API

## Batch-based Mining API

Proof-of-work mining must use a batch-based API (start, count) rather than a blocking loop, so the caller controls parallelization and the library stays platform-agnostic. <!-- [^954c5-22] -->
