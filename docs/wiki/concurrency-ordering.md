---
title: Concurrency Ordering
slug: concurrency-ordering
topic: concurrency
summary: All Relaxed orderings on the cancel flag were replaced with Release/Acquire pairs for correctness on ARM.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
---

# Concurrency Ordering

## Cancel-Flag Ordering

All Relaxed orderings on the cancel flag were replaced with Release/Acquire pairs for correctness on ARM.

The v0.5.0 release includes an expiration index replacing the gc Phase-1 cursor (#1106, closing #1097) and removal of blocking sign_active (#1104, closing #972). <!-- [^da6b1-67] -->

<!-- citations: [^02745-33] [^02745-53] -->
