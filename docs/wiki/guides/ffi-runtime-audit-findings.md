---
title: FFI Runtime Audit Findings
slug: ffi-runtime-audit-findings
topic: ffi-runtime
summary: The hung-spinner finding (no async success terminal) is stale â success terminal was already implemented in reconcile.rs (PR #1211) after the audit was writte
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-19
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# FFI Runtime Audit Findings

## Finding 4: ExternalSignerCapabilityBridge transport selection & concurrent-Intent rejection

The hung-spinner finding (no async success terminal) is stale — success terminal was already implemented in reconcile.rs (PR #1211) after the audit was written.

<!-- citations: [^11850-158] [^11850-206] [^11850-207] [^11850-225] [^11850-226] -->
