---
title: NMP Performance Battery — Framework Optimization and Testing
slug: nmp-perf-battery
summary: Performance testing must focus on optimizing the NMP framework itself, not on optimizing any specific app like Chirp.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:c9ae5a7c-0f5e-44ec-94d6-d9b5e31d8991
---

# NMP Performance Battery — Framework Optimization and Testing

## Scope and Focus

Performance testing must focus on optimizing the NMP framework itself, not on optimizing any specific app like Chirp. [^c9ae5-1]


The completed performance battery is saved as structured data to docs/wiki/nmp-perf-battery.md. [^c9ae5-2]

## Agent Responsibilities

An Opus agent designs the performance test battery and reviews every performance optimization proposal before it proceeds. Haiku agents drive interactions with the iOS simulator and handle coding implementation tasks. [^c9ae5-3]

## Baseline Capture

Baseline performance capture runs three parallel agents: a Rust agent for ingest perf, an iOS code agent for snapshot/SwiftUI/FlatBuffers auditing, and a simulator agent to capture live apply_us/first_ms numbers. [^c9ae5-4]
## See Also

