---
title: State Emission Bug
slug: state-emission-bug
topic: ffi-runtime
summary: maybe_emit_after_dispatch only emits snapshots when running == true; if running == false, state changes are silently swallowed and the UI never updates
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-06-18
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:27a9cbf3-1348-44f6-bc0f-95a0a9c6ad84
  - session:c5325e71-7d4e-451e-8c15-81cdae440f5f
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# State Emission Bug

## State Emission Bug

maybe_emit_after_dispatch only emits snapshots when running == true; if running == false, state changes are silently swallowed and the UI never updates. A nextTimeline != modularTimeline equality check prevents spurious SwiftUI re-renders from the per-tick snapshot refresh. Finding A (hung-spinner / no async success terminal) is stale — PR #1211 already landed the success-terminal logic (it moved from runtime.rs into reconcile.rs after the audit was written).

<!-- citations: [^27a9c-8] [^c5325-3] [^11850-23] [^11850-87] -->
