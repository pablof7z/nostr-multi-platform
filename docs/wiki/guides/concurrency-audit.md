---
title: Concurrency Audit
slug: concurrency-audit
topic: ffi-runtime
summary: The workspace has 289 total locks (181 Mutex, 1 RwLock, 107 Atomic) and zero compiler warnings or deprecated items
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
  - session:47203d35-d7c9-4c12-bc47-a40773d7acc2
---

# Concurrency Audit

## Lock Distribution

The workspace has 289 total locks (181 Mutex, 1 RwLock, 107 Atomic) and zero compiler warnings or deprecated items. nmp-core dominates the workspace, containing 160 of those locks (126 Mutex, 1 RwLock, 33 Atomic); no parking_lot usage is detected. <!-- [^1c093-2] -->

## Legitimate Lock-Free Primitives

pending_mls_autopublish (AtomicBool) and actor_queue_depth (Arc<AtomicU64>) are both legitimate lock-free primitives, not actor-state leaks. <!-- [^1c093-3] -->

## Agent Concurrency

The agent concurrency cap is 10 agents. <!-- [^47203-2] -->
