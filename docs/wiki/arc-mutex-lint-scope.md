---
title: Arc<Mutex> Lint Scope — D14 Enforcement on Core Kernel Structs Only
slug: arc-mutex-lint-scope
summary: D14 flags `Arc<Mutex<Vec<...>>>` only on `NmpApp`, `Kernel`, or `Actor*` structs, not test fixtures or mock signers.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-21
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
---

# Arc<Mutex> Lint Scope — D14 Enforcement on Core Kernel Structs Only

## Scope

D14 flags `Arc<Mutex<Vec<...>>>` only on `NmpApp`, `Kernel`, or `Actor*` structs, not test fixtures or mock signers. [^1c093-10]

## See Also

