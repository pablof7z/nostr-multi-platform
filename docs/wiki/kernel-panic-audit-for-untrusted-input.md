---
title: Kernel Panic Audit — Untrusted Relay Input Reachability
slug: kernel-panic-audit-for-untrusted-input
summary: The 65 `panic!` calls in `nmp-core` must be audited for any reachable from untrusted relay input.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-26
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:37e351ee-aa2b-43eb-9793-482de338f883
---

# Kernel Panic Audit — Untrusted Relay Input Reachability

## Audit Scope

The 65 `panic!` calls in `nmp-core` must be audited for any reachable from untrusted relay input. [^1c093-15]


The 65 `panic!` calls in `nmp-core` must be audited for any reachable from untrusted relay input. Kernel serialization must catch serde_json::to_value errors, increment update_frame_degradations_total, log an NMP_DEGRADATION message, and produce a minimal degraded JSON payload instead of panicking. [^37e35-8]

## Degradation Metrics

update_frame_degradations_total is a monotonic counter for the Kernel lifetime that tracks malformed or impossible value-shape drift during update frame encoding/decoding. [^37e35-9]
## See Also

